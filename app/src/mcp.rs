//! MCP (Model Context Protocol) server: `section-response --mcp` speaks JSON-RPC 2.0 over stdin
//! and stdout (one message a line, the MCP stdio transport) so that an AI assistant or any other
//! MCP client can define sections and run this program's analyses as tools.
//!
//! Tools: quick_define, describe_section, moment_curvature, sectional_response,
//! interaction_diagram, biaxial_bending, shear_2d, check_load_cases. Each runs the same routines
//! the window does (ui.rs, the analysis thread), with the same defaults.
//!
//! Stdout carries protocol messages only; anything else the program prints goes to stderr.
//! Each tool call runs in a worker process of its own (`section-response --mcp-worker`), watched
//! by a thread of the server, so pings and further calls are answered while a long analysis runs,
//! and an internal error (a panic: the release build aborts on one), a cancellation or the time
//! limit ends only that call's process. A client's progress token gets notifications/progress
//! from the analyses that report it.
//!
//! Worker protocol: the server writes one line, {"name", "arguments"}, and keeps the worker's
//! stdin open (the worker exits when it closes, so a worker never outlives its server); the
//! worker writes {"progress": 0-100} lines and finally {"result": ...} or {"error": "..."}.

use crate::model::{self, AciFactors, QuickDefine, Section, Shape};
use crate::{analysis, biaxial, demand, design, rsp, shear2d};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// Protocol revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Sectional analysis of reinforced and prestressed concrete members (the \
Modified Compression Field Theory, in the manner of Response-2000). Units: mm, MPa, kN, kNm; \
curvature rad/km; strains mm/m. Axial load in `loads` and in the analysis results is tension \
positive; in check_load_cases Pu is compression positive. Start with quick_define (a section from \
a few dimensions and bar counts; it returns the section as .rsp text) or describe_section on a \
.rsp file, then pass the same `section` to the analysis tools. Relative file paths resolve against \
the server's working directory. moment_curvature takes about a second; sectional_response, \
interaction_diagram, shear_2d and check_load_cases with shear run full MCFT analyses and can take \
from seconds to minutes.";

struct Out {
    w: Mutex<std::io::Stdout>,
    /// requests the client cancelled (their ids, as JSON text): no response is sent for them
    cancelled: Mutex<HashSet<String>>,
    /// the worker process of each running call (by request id, as JSON text)
    running: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
}

impl Out {
    fn send(&self, v: &Value) {
        let mut w = self.w.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(w, "{}", v);
        let _ = w.flush();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Progress of one tool call, 0 to 100, passed on only when it grows.
struct Progress {
    sink: Box<dyn Fn(i32) + Send + Sync>,
    last: Mutex<i32>,
}

impl Progress {
    fn new(sink: Box<dyn Fn(i32) + Send + Sync>) -> Self {
        Progress { sink, last: Mutex::new(-1) }
    }
    fn report(&self, p: f32) {
        let pct = (p.clamp(0.0, 1.0) * 100.0).round() as i32;
        // held while passing it on, so the values go out in increasing order
        let mut last = lock(&self.last);
        if pct <= *last {
            return;
        }
        *last = pct;
        (self.sink)(pct);
    }
}

/// The longest a tool call may run (SR_MCP_TIMEOUT_S, seconds; 0 = no limit; default 30 min).
fn time_limit() -> Option<Duration> {
    let s = std::env::var("SR_MCP_TIMEOUT_S").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(1800);
    (s > 0).then(|| Duration::from_secs(s))
}

/// Serve until stdin closes; calls still running then are finished and answered.
pub fn serve() {
    let out = Arc::new(Out { w: Mutex::new(std::io::stdout()), cancelled: Mutex::new(HashSet::new()), running: Mutex::new(HashMap::new()) });
    let mut workers: Vec<std::thread::JoinHandle<()>> = vec![];
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                out.send(&error(Value::Null, -32700, &format!("Parse error: {}", e)));
                continue;
            }
        };
        let msgs = match msg {
            Value::Array(a) => a,
            m => vec![m],
        };
        for m in msgs {
            if let Some(h) = handle(&out, m) {
                workers.push(h);
            }
            workers.retain(|h| !h.is_finished());
        }
    }
    for h in workers {
        let _ = h.join();
    }
}

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

fn reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// One incoming message; a tool call is handed to a thread (returned).
fn handle(out: &Arc<Out>, m: Value) -> Option<std::thread::JoinHandle<()>> {
    let Some(method) = m.get("method").and_then(Value::as_str) else {
        // a response to a request of ours (we send none) or junk
        if let Some(id) = m.get("id") {
            if m.get("result").is_none() && m.get("error").is_none() {
                out.send(&error(id.clone(), -32600, "Invalid request: no method"));
            }
        }
        return None;
    };
    let id = m.get("id").cloned();
    let params = m.get("params").cloned().unwrap_or(json!({}));
    match (method, id) {
        ("initialize", Some(id)) => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
            out.send(&reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "section-response",
                        "title": "Section Response",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": INSTRUCTIONS
                }),
            ));
        }
        ("ping", Some(id)) => out.send(&reply(id, json!({}))),
        ("tools/list", Some(id)) => out.send(&reply(id, json!({ "tools": tools() }))),
        ("tools/call", Some(id)) => {
            let Some(name) = params.get("name").and_then(Value::as_str).map(String::from) else {
                out.send(&error(id, -32602, "tools/call needs a tool name"));
                return None;
            };
            if !TOOL_NAMES.contains(&name.as_str()) {
                out.send(&error(id, -32602, &format!("Unknown tool: {}", name)));
                return None;
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let token = params.pointer("/_meta/progressToken").cloned();
            let out = out.clone();
            return Some(std::thread::spawn(move || {
                let po = out.clone();
                let prog = Progress::new(Box::new(move |pct| {
                    if let Some(t) = &token {
                        po.send(&json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/progress",
                            "params": { "progressToken": t, "progress": pct, "total": 100 }
                        }));
                    }
                }));
                let res = run_call(&out, &id, &name, &args, &prog);
                if lock(&out.cancelled).remove(&id.to_string()) {
                    return;
                }
                let result = match res {
                    Ok(v) => json!({
                        "content": [{ "type": "text", "text": v.to_string() }],
                        "structuredContent": v,
                        "isError": false
                    }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                };
                out.send(&reply(id, result));
            }));
        }
        ("notifications/cancelled", None) => {
            // only a call still running: its worker is stopped and its response dropped
            if let Some(r) = params.get("requestId").map(|r| r.to_string()) {
                let child = lock(&out.running).get(&r).cloned();
                if let Some(c) = child {
                    lock(&out.cancelled).insert(r);
                    let _ = lock(&c).kill();
                }
            }
        }
        (_, None) => {} // other notifications (initialized, ...): nothing to do
        (other, Some(id)) => out.send(&error(id, -32601, &format!("Method not found: {}", other))),
    }
    None
}

// ---------------------------------------------------------------------------------------------
// tool definitions

const TOOL_NAMES: [&str; 8] = [
    "quick_define",
    "describe_section",
    "moment_curvature",
    "sectional_response",
    "interaction_diagram",
    "biaxial_bending",
    "shear_2d",
    "check_load_cases",
];

fn bars_schema(what: &str) -> Value {
    json!({
        "type": "object",
        "description": what,
        "properties": {
            "n": { "type": "integer", "minimum": 0, "description": "number of bars" },
            "bar": { "type": "string", "description": "CSA bar designation: 10M, 15M, 20M, 25M, 30M, 35M, 45M or 55M" },
            "area": { "type": "number", "description": "area of one bar (mm2), instead of a designation" }
        }
    })
}

fn quick_schema() -> Value {
    json!({
        "type": "object",
        "description": "A section built like the original's Quick Define wizard. Defaults: rectangle 250 x 600, f'c 35, fy 400, 1 x 20M top, 3 x 25M bottom, closed 10M stirrups @ 250, cover 40. A circle defaults to 16 x 25M on the outer ring with a hoop; a column to 400 x 400 with 3 x 25M top and bottom, 1 x 25M a side face and ties @ 300.",
        "properties": {
            "title": { "type": "string" },
            "shape": { "type": "string", "enum": ["rectangle", "circle", "tee", "ibeam", "column"] },
            "b": { "type": "number", "description": "width (rectangle, column), mm" },
            "h": { "type": "number", "description": "overall height (rectangle, column, tee, ibeam), mm" },
            "d": { "type": "number", "description": "diameter (circle), mm" },
            "btop": { "type": "number", "description": "top flange width (tee, ibeam), mm" },
            "ttop": { "type": "number", "description": "top flange thickness (tee, ibeam), mm" },
            "bweb": { "type": "number", "description": "web width (tee, ibeam), mm" },
            "bbot": { "type": "number", "description": "bottom flange width (ibeam), mm" },
            "tbot": { "type": "number", "description": "bottom flange thickness (ibeam), mm" },
            "fc": { "type": "number", "description": "concrete strength f'c, MPa" },
            "fy": { "type": "number", "description": "longitudinal steel yield, MPa" },
            "fyt": { "type": "number", "description": "transverse steel yield, MPa" },
            "top": bars_schema("top bars (circle: the outer ring)"),
            "bottom": bars_schema("bottom bars (circle: the inner ring)"),
            "side": bars_schema("bars on each side face, between the top and bottom rows (not circles)"),
            "stirrup": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["none", "single leg", "open", "closed", "hoop", "t headed", "interlocking hoops"] },
                    "bar": { "type": "string", "description": "bar designation, e.g. 10M" },
                    "area": { "type": "number", "description": "area of one bar (mm2), instead of a designation" },
                    "spacing": { "type": "number", "description": "mm" }
                }
            },
            "cover": { "type": "number", "description": "clear cover to the stirrup, mm" },
            "strand_type": { "type": "integer", "minimum": 0, "maximum": 4, "description": "0 none, 1 1860 MPa low-relaxation 13 mm, 2 stress-relieved 13 mm, 3 low-relaxation 15 mm, 4 stress-relieved 15 mm" },
            "strands": { "type": "integer", "minimum": 0, "description": "number of bottom strands" },
            "prestrain": { "type": "number", "description": "strands' locked-in strain Delta-epsilon-p, mm/m (default 6.5)" }
        }
    })
}

fn section_schema() -> Value {
    json!({
        "type": "object",
        "description": "The cross-section: give one of file, rsp or quick.",
        "properties": {
            "file": { "type": "string", "description": "path to a .rsp section file" },
            "rsp": { "type": "string", "description": ".rsp file text (as quick_define returns it)" },
            "quick": quick_schema(),
            "index": { "type": "integer", "minimum": 0, "description": "which section of a file holding several (default 0)" }
        }
    })
}

fn loads_schema() -> Value {
    json!({
        "type": "object",
        "description": "Overrides of the section's loading (loads = constant + factor x increment, the factor raised until failure). Omitted values keep the section's own.",
        "properties": {
            "axial_kn": { "type": "number", "description": "constant axial load, kN, tension positive" },
            "m_const_knm": { "type": "number", "description": "constant moment, kNm, + puts the bottom in tension" },
            "v_const_kn": { "type": "number", "description": "constant shear, kN" },
            "n_inc_kn": { "type": "number", "description": "axial increment, kN" },
            "m_inc_knm": { "type": "number", "description": "moment increment, kNm" },
            "v_inc_kn": { "type": "number", "description": "shear increment, kN" },
            "mv_ratio_m": { "type": "number", "description": "shortcut: increment of M/V (m) with a 1 kN shear increment" },
            "negative": { "type": "boolean", "description": "negative moment (top in tension)" },
            "moment_only": { "type": "boolean", "description": "shortcut: increase the moment only, no shear" }
        }
    })
}

fn code_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["file", "nominal", "aci", "aashto", "csa"],
        "description": "Design basis: the section's own setting (file, default), nominal (no factors), ACI 318-19 or AASHTO LRFD strength reduction factors on the nominal response, or CSA A23.3 factored materials (0.65, 0.85, 0.90)."
    })
}

fn max_points_schema() -> Value {
    json!({ "type": "integer", "minimum": 2, "maximum": 1000, "description": "largest number of curve points returned (default 40)" })
}

fn tool(name: &str, title: &str, description: &str, props: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": { "title": title, "readOnlyHint": read_only, "idempotentHint": true, "openWorldHint": false }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "quick_define",
            "Quick Define a section",
            "Build a cross-section from a shape, dimensions, bars and stirrups (the Quick Define wizard). Returns the section's summary and its .rsp text, which can be passed to the other tools as section.rsp; save_to also writes it to a file.",
            json!({
                "quick": quick_schema(),
                "save_to": { "type": "string", "description": "write the section to this .rsp file" },
                "include_rsp": { "type": "boolean", "description": "return the .rsp text (default true)" }
            }),
            &["quick"],
            false,
        ),
        tool(
            "describe_section",
            "Describe a section",
            "Read a section and report its geometry, materials, reinforcement layers, stirrups, loading and design basis. With loads, code and save_to it also edits and saves the section.",
            json!({
                "section": section_schema(),
                "loads": loads_schema(),
                "code": code_schema(),
                "save_to": { "type": "string", "description": "write the (edited) section to this .rsp file" },
                "include_rsp": { "type": "boolean", "description": "return the .rsp text (default false)" }
            }),
            &["section"],
            false,
        ),
        tool(
            "moment_curvature",
            "Moment-curvature",
            "Moment-curvature response at a constant axial load (plane sections, no shear): peak moment, cracking and first-yield moments, the designer's concrete and steel strain-limit points, and the curve.",
            json!({
                "section": section_schema(),
                "loads": loads_schema(),
                "code": code_schema(),
                "layers": { "type": "integer", "minimum": 8, "maximum": 400, "description": "concrete layers (default 80)" },
                "max_points": max_points_schema()
            }),
            &["section"],
            true,
        ),
        tool(
            "sectional_response",
            "Sectional response (M-V-N)",
            "Full sectional analysis with shear (MCFT) under the section's loading: constant loads plus an increment raised to failure. Reports the peak shear and moment, what governed (flexure, yield, web shear, crushing), whether failure was confirmed, the state at the peak, and the load path.",
            json!({
                "section": section_schema(),
                "loads": loads_schema(),
                "code": code_schema(),
                "max_points": max_points_schema()
            }),
            &["section"],
            true,
        ),
        tool(
            "interaction_diagram",
            "Interaction diagram",
            "Strength interaction diagram from a series of full sectional analyses. M-V: moment-shear at the constant axial load (both moment directions); M-N: moment-axial load; N-V: axial load-shear at the constant moment; M-V-centred: M-V with the original program's run layout. With a code, the design (phi-factored) curve too.",
            json!({
                "section": section_schema(),
                "type": { "type": "string", "enum": ["M-V", "M-N", "N-V", "M-V-centred"], "description": "default M-V" },
                "points": { "type": "integer", "minimum": 4, "maximum": 40, "description": "runs per side (default 12)" },
                "loads": loads_schema(),
                "code": code_schema()
            }),
            &["section"],
            true,
        ),
        tool(
            "biaxial_bending",
            "Biaxial bending (Mx-My)",
            "Mx-My capacity contour at a constant axial load: a moment-curvature run in each direction, the peak of each, and the full run at one chosen moment direction. With ACI/AASHTO, phi from the net tensile strain.",
            json!({
                "section": section_schema(),
                "axial_kn": { "type": "number", "description": "axial load, kN, tension positive (default 0)" },
                "angle_deg": { "type": "number", "description": "moment direction of the detailed run, degrees (default 0: Mx, top in compression)" },
                "directions": { "type": "integer", "minimum": 8, "maximum": 72, "description": "directions round the contour (default 36)" },
                "code": code_schema(),
                "max_points": max_points_schema()
            }),
            &["section"],
            true,
        ),
        tool(
            "shear_2d",
            "Shear in two directions (Vx-Vy)",
            "Vx-Vy shear capacity diagram: a full sectional shear analysis in each direction at a constant axial load and M/V ratio. Optionally checks a demand (vux_kn, vuy_kn) against it.",
            json!({
                "section": section_schema(),
                "axial_kn": { "type": "number", "description": "axial load, kN, tension positive (default 0)" },
                "mv_ratio_m": { "type": "number", "description": "moment-to-shear ratio M/V, m (default 0)" },
                "directions": { "type": "integer", "minimum": 4, "maximum": 72, "description": "directions round the diagram (default 16)" },
                "vux_kn": { "type": "number", "description": "demand shear along x, kN" },
                "vuy_kn": { "type": "number", "description": "demand shear along y, kN" },
                "code": code_schema()
            }),
            &["section"],
            true,
        ),
        tool(
            "check_load_cases",
            "Check load cases (D/C)",
            "Demand/capacity ratios for a list of load cases: axial load with biaxial bending, two-direction shear and (with ACI/AASHTO) torsion. Pu is compression positive; Mx bends about the horizontal axis (+ compresses the top), My about the vertical axis (+ compresses the right side); Vy goes with Mx, Vx with My.",
            json!({
                "section": section_schema(),
                "cases": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "pu_kn": { "type": "number" },
                            "mx_knm": { "type": "number" },
                            "my_knm": { "type": "number" },
                            "vx_kn": { "type": "number" },
                            "vy_kn": { "type": "number" },
                            "tu_knm": { "type": "number" }
                        }
                    }
                },
                "table": { "type": "string", "description": "instead of cases: one case a line, 'name Pu Mx My Vx Vy Tu' separated by tabs, commas or spaces" },
                "code": code_schema()
            }),
            &["section"],
            true,
        ),
    ]
}

fn call(name: &str, a: &Value, p: &Progress) -> Result<Value, String> {
    let pf = |x: f32| p.report(x);
    match name {
        "quick_define" => t_quick(a),
        "describe_section" => t_describe(a),
        "moment_curvature" => t_mphi(a),
        "sectional_response" => t_shear(a),
        "interaction_diagram" => t_inter(a, &pf),
        "biaxial_bending" => t_biaxial(a, &pf),
        "shear_2d" => t_shear2d(a, &pf),
        "check_load_cases" => t_cases(a, &pf),
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------------------------
// worker processes

/// A tool call in a worker process; in this process only when no worker can be started.
fn run_call(out: &Arc<Out>, id: &Value, name: &str, args: &Value, prog: &Progress) -> Result<Value, String> {
    let in_process = || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(name, args, prog)))
            .unwrap_or_else(|_| Err("The analysis stopped on an internal error (a panic); try other settings.".into()))
    };
    let Ok(exe) = std::env::current_exe() else { return in_process() };
    let mut cmd = Command::new(exe);
    cmd.arg("--mcp-worker").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(mut child) = cmd.spawn() else { return in_process() };
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return in_process();
    };
    let request = json!({ "name": name, "arguments": args });
    if writeln!(stdin, "{}", request).and_then(|_| stdin.flush()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("The analysis process could not be started.".into());
    }
    let child = Arc::new(Mutex::new(child));
    let key = id.to_string();
    lock(&out.running).insert(key.clone(), child.clone());
    // the worker's stderr: kept to its last lines, for the message of a crash
    let err = std::thread::spawn(move || {
        let mut tail: VecDeque<String> = VecDeque::new();
        for l in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 40 {
                tail.pop_front();
            }
            tail.push_back(l);
        }
        tail
    });
    // the time limit
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let timed_out = Arc::new(AtomicBool::new(false));
    let limit = time_limit();
    let watchdog = limit.map(|lim| {
        let (child, timed_out) = (child.clone(), timed_out.clone());
        std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(lim) {
                timed_out.store(true, Ordering::SeqCst);
                let _ = lock(&child).kill();
            }
        })
    });
    let mut result: Option<Result<Value, String>> = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(p) = v.get("progress").and_then(Value::as_f64) {
            prog.report(p as f32 / 100.0);
        } else if let Some(r) = v.get("result") {
            result = Some(Ok(r.clone()));
        } else if let Some(e) = v.get("error") {
            result = Some(Err(e.as_str().map(String::from).unwrap_or_else(|| e.to_string())));
        }
    }
    drop(stdin);
    let _ = done_tx.send(());
    if let Some(w) = watchdog {
        let _ = w.join();
    }
    let status = lock(&child).wait();
    lock(&out.running).remove(&key);
    let tail = err.join().unwrap_or_default();
    if let Some(r) = result {
        return r;
    }
    if timed_out.load(Ordering::SeqCst) {
        return Err(format!(
            "The analysis was stopped after {} s, the time limit (environment variable SR_MCP_TIMEOUT_S, 0 for none). Try fewer points or directions.",
            limit.map(|l| l.as_secs()).unwrap_or(0)
        ));
    }
    Err(crash_message(status.ok(), &tail))
}

/// What to tell the client when a worker ended without a result: the panic message from its
/// stderr when there is one ("thread '..' panicked at file:line:col:" and the message after it).
fn crash_message(status: Option<std::process::ExitStatus>, tail: &VecDeque<String>) -> String {
    let lines: Vec<&String> = tail.iter().collect();
    if let Some(i) = lines.iter().position(|l| l.contains("panicked at")) {
        let at = lines[i].split("panicked at").nth(1).unwrap_or("").trim().trim_end_matches(':').trim();
        // older Rust puts the message on the same line: "panicked at 'msg', file:line:col"
        let msg: Vec<&str> = lines[i + 1..]
            .iter()
            .take_while(|l| !l.starts_with("note:") && !l.starts_with("stack backtrace"))
            .map(|l| l.as_str())
            .collect();
        let msg = msg.join(" ");
        return if msg.trim().is_empty() {
            format!("The analysis stopped on an internal error: {}. Try other settings.", at)
        } else {
            format!("The analysis stopped on an internal error: {} (at {}). Try other settings.", msg.trim(), at)
        };
    }
    let code = status.and_then(|s| s.code()).map(|c| format!(" (exit code {:#x})", c as u32)).unwrap_or_default();
    let last = lines.last().map(|l| format!(": {}", l.trim())).unwrap_or_default();
    format!("The analysis process ended without a result{}{}.", code, last)
}

/// `section-response --mcp-worker`: run one tool call for the server (see the top of the file).
pub fn worker() {
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return;
    }
    // the server keeps this stdin open while it waits: when it closes (the server ended, or
    // gave up on this call), stop
    std::thread::spawn(|| {
        let mut buf = [0u8; 256];
        let mut s = std::io::stdin();
        loop {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => std::process::exit(3),
                Ok(_) => {}
            }
        }
    });
    let emit = |v: &Value| {
        let mut w = std::io::stdout().lock();
        if writeln!(w, "{}", v).and_then(|_| w.flush()).is_err() {
            std::process::exit(3); // nobody is listening
        }
    };
    let req: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(e) => {
            emit(&json!({ "error": format!("Bad worker request: {}", e) }));
            return;
        }
    };
    let name = req.get("name").and_then(Value::as_str).unwrap_or("");
    let args = req.get("arguments").cloned().unwrap_or(json!({}));
    if std::env::var("SR_MCP_PANIC").is_ok_and(|v| v == name) {
        // for testing the server's handling of a crashed analysis
        panic!("SR_MCP_PANIC: test panic in {}", name);
    }
    let prog = Progress::new(Box::new(move |pct| emit(&json!({ "progress": pct }))));
    // no catch_unwind: a panic ends this process, and the server reports its message
    let out = match call(name, &args, &prog) {
        Ok(v) => json!({ "result": v }),
        Err(e) => json!({ "error": e }),
    };
    emit(&out);
}

// ---------------------------------------------------------------------------------------------
// arguments

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn int(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u64)))
}

fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn flag(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

/// A number rounded to 5 significant digits (null when not finite).
fn f(v: f64) -> Value {
    if !v.is_finite() {
        return Value::Null;
    }
    if v == 0.0 {
        return json!(0.0);
    }
    let d = (4 - v.abs().log10().floor() as i32).clamp(0, 12);
    let p = 10f64.powi(d);
    // (+ 0.0 turns a rounded -0.0 into 0.0)
    json!((v * p).round() / p + 0.0)
}

fn fo(v: Option<f64>) -> Value {
    v.map(f).unwrap_or(Value::Null)
}

/// Up to `max` indices spread evenly over 0..n (first and last included), plus `keep`.
fn sample(n: usize, max: usize, keep: Option<usize>) -> Vec<usize> {
    let max = max.max(2);
    let mut v: Vec<usize> = if n <= max { (0..n).collect() } else { (0..max).map(|i| i * (n - 1) / (max - 1)).collect() };
    if let Some(k) = keep.filter(|k| *k < n) {
        v.push(k);
    }
    v.sort_unstable();
    v.dedup();
    v
}

fn max_points(a: &Value) -> usize {
    int(a, "max_points").unwrap_or(40).clamp(2, 1000) as usize
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", absolute(path), e))
}

fn absolute(path: &str) -> String {
    std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string())
}

/// The section a tool works on, with the code and loading overrides applied.
fn section_arg(a: &Value) -> Result<Section, String> {
    let s = a.get("section").ok_or("Give a section: {\"file\": path}, {\"rsp\": text} or {\"quick\": {...}}.")?;
    let pick = |v: Vec<Section>| -> Result<Section, String> {
        let i = int(s, "index").unwrap_or(0) as usize;
        let n = v.len();
        v.into_iter().nth(i).ok_or_else(|| format!("The file holds {} section(s); there is no index {}.", n, i))
    };
    let mut sec = if let Some(p) = s.as_str().or_else(|| text(s, "file")) {
        pick(rsp::load(&read_file(p)?)?)?
    } else if let Some(t) = text(s, "rsp") {
        pick(rsp::load(t)?)?
    } else if let Some(q) = s.get("quick") {
        Section::from_quick(&quick_from(q)?)?
    } else {
        return Err("The section needs one of: file, rsp, quick.".into());
    };
    apply_code(&mut sec, a)?;
    apply_loads(&mut sec, a)?;
    Ok(sec)
}

fn apply_code(sec: &mut Section, a: &Value) -> Result<(), String> {
    match text(a, "code").map(|c| c.to_lowercase()).as_deref() {
        None | Some("file") => {}
        Some("nominal") => {
            sec.aci = None;
            sec.phi = [1.0; 3];
        }
        Some("aci") => {
            sec.aci = Some(AciFactors::default());
            sec.phi = [1.0; 3];
        }
        Some("aashto") => {
            sec.aci = Some(AciFactors::aashto());
            sec.phi = [1.0; 3];
        }
        Some("csa") => {
            sec.aci = None;
            sec.phi = model::PHI_CSA;
        }
        Some(o) => return Err(format!("Unknown code '{}': use file, nominal, aci, aashto or csa.", o)),
    }
    Ok(())
}

fn apply_loads(sec: &mut Section, a: &Value) -> Result<(), String> {
    let Some(l) = a.get("loads") else { return Ok(()) };
    if !l.is_object() {
        return Err("'loads' must be an object.".into());
    }
    let ld = &mut sec.loads;
    if let Some(v) = num(l, "axial_kn") {
        ld.axial = v;
    }
    if let Some(v) = num(l, "m_const_knm") {
        ld.m_const = v;
    }
    if let Some(v) = num(l, "v_const_kn") {
        ld.v_const = v;
    }
    let neg = flag(l, "negative");
    if ["n_inc_kn", "m_inc_knm", "v_inc_kn"].iter().any(|k| l.get(*k).is_some()) {
        if let Some(v) = num(l, "n_inc_kn") {
            ld.n_inc = v;
        }
        if let Some(v) = num(l, "m_inc_knm") {
            ld.m_inc = v;
        }
        if let Some(v) = num(l, "v_inc_kn") {
            ld.v_inc = v;
        }
    } else if flag(l, "moment_only") == Some(true) {
        let n = neg.unwrap_or(ld.negative);
        ld.m_inc = if n { -1.0 } else { 1.0 };
        ld.v_inc = 0.0;
    } else if let Some(mv) = num(l, "mv_ratio_m") {
        let n = neg.unwrap_or(ld.negative);
        ld.m_inc = if n { -mv.abs() } else { mv.abs() };
        ld.v_inc = 1.0;
    } else if let Some(n) = neg {
        // only the direction: turn the moment increment (or the constant moment) round
        if ld.m_inc.abs() > 1e-12 {
            ld.m_inc = if n { -ld.m_inc.abs() } else { ld.m_inc.abs() };
        } else {
            ld.m_const = if n { -ld.m_const.abs() } else { ld.m_const.abs() };
        }
    }
    ld.sync();
    if let Some(n) = neg {
        ld.negative = n;
    }
    Ok(())
}

fn quick_from(q: &Value) -> Result<QuickDefine, String> {
    let mut d = QuickDefine::default();
    let shape = text(q, "shape").unwrap_or("rectangle").to_lowercase().replace(['-', '_', ' '], "");
    d.shape = match shape.as_str() {
        "rectangle" | "rect" | "rectangular" => 0,
        "circle" | "circular" | "round" => 1,
        "tee" | "t" | "tbeam" => 2,
        "ibeam" | "i" => 3,
        "column" => 4,
        o => return Err(format!("Unknown shape '{}': use rectangle, circle, tee, ibeam or column.", o)),
    };
    // the wizard's defaults for the shape (ui.rs, Quick Define step 2)
    match d.shape {
        1 => {
            d.top_n = 16;
            d.top_bar = "25M".into();
            d.bot_n = 0;
            d.bot_bar.clear();
            d.stirrup_type = 4;
        }
        4 => {
            d.dims[0] = 400.0;
            d.dims[1] = 400.0;
            d.top_n = 3;
            d.top_bar = "25M".into();
            d.bot_n = 3;
            d.bot_bar = "25M".into();
            d.side_n = 1;
            d.side_bar = "25M".into();
            d.st_space = 300.0;
        }
        _ => {}
    }
    let names: &[&str] = match d.shape {
        1 => &["d"],
        2 => &["btop", "h", "bweb", "ttop"],
        3 => &["btop", "h", "bweb", "ttop", "tbot", "bbot"],
        _ => &["b", "h"],
    };
    for (k, n) in names.iter().enumerate() {
        if let Some(v) = num(q, n) {
            d.dims[k] = v;
        }
    }
    if let Some(t) = text(q, "title") {
        d.title = t.into();
    }
    if let Some(v) = num(q, "fc") {
        d.fc = v;
    }
    if let Some(v) = num(q, "fy") {
        d.fy = v;
    }
    if let Some(v) = num(q, "fyt") {
        d.fyt = v;
    }
    let bars = |key: &str, alt: &str, n: &mut u32, bar: &mut String, by_area: &mut bool, area: &mut f64| {
        if let Some(b) = q.get(key).or_else(|| q.get(alt)) {
            if let Some(v) = int(b, "n") {
                *n = v as u32;
            }
            if let Some(t) = text(b, "bar") {
                *bar = t.into();
                *by_area = false;
            }
            if let Some(v) = num(b, "area") {
                *area = v;
                *by_area = true;
            }
        }
    };
    bars("top", "outer", &mut d.top_n, &mut d.top_bar, &mut d.top_by_area, &mut d.top_area);
    bars("bottom", "inner", &mut d.bot_n, &mut d.bot_bar, &mut d.bot_by_area, &mut d.bot_area);
    bars("side", "side", &mut d.side_n, &mut d.side_bar, &mut d.side_by_area, &mut d.side_area);
    if let Some(s) = q.get("stirrup") {
        if let Some(t) = s.get("type") {
            d.stirrup_type = match t.as_u64() {
                Some(i) if (i as usize) < model::STIRRUP_TYPES.len() => i as usize,
                Some(i) => return Err(format!("Stirrup type {} is out of range (0 to 6).", i)),
                None => {
                    let t = t.as_str().unwrap_or("").to_lowercase();
                    let key = t.replace(['-', '_'], " ");
                    match key.trim() {
                        "none" | "" => 0,
                        "single" | "single leg" => 1,
                        "open" | "open stirrup" => 2,
                        "closed" | "closed stirrup" | "tie" | "ties" => 3,
                        "hoop" | "spiral" => 4,
                        "t headed" | "t headed single leg" | "headed" => 5,
                        "interlocking" | "interlocking hoops" => 6,
                        _ => return Err(format!("Unknown stirrup type '{}'.", t)),
                    }
                }
            };
        }
        if let Some(t) = text(s, "bar") {
            d.st_bar = t.into();
            d.st_by_area = false;
        }
        if let Some(v) = num(s, "area") {
            d.st_area = v;
            d.st_by_area = true;
        }
        if let Some(v) = num(s, "spacing") {
            d.st_space = v;
        }
    }
    if let Some(v) = num(q, "cover") {
        d.cover = v;
    }
    if let Some(v) = int(q, "strand_type") {
        if v as usize >= model::STRAND_TYPES.len() {
            return Err("strand_type is 0 to 4.".into());
        }
        d.strand_type = v as usize;
    }
    if let Some(v) = int(q, "strands") {
        d.strands = v as u32;
    }
    if let Some(v) = num(q, "prestrain") {
        d.dep = v;
    }
    Ok(d)
}

// ---------------------------------------------------------------------------------------------
// results

fn design_basis(sec: &Section) -> String {
    match &sec.aci {
        Some(a) => format!("{} strength reduction factors on the nominal response", a.name()),
        None if model::is_factored(&sec.phi) => format!("factored materials (phi_c {}, phi_s {}, phi_p {})", sec.phi[0], sec.phi[1], sec.phi[2]),
        None => "nominal (unfactored)".into(),
    }
}

fn summary(sec: &Section) -> Value {
    let h = sec.height();
    let shape = match &sec.shape {
        Shape::Rect { b, h } => json!({ "type": "rectangle", "b": b, "h": h }),
        Shape::Tee { bf, hf, bw, h } => json!({ "type": "tee", "btop": bf, "ttop": hf, "bweb": bw, "h": h }),
        Shape::IBeam { bt, ht, bw, bb, hb, h } => json!({ "type": "ibeam", "btop": bt, "ttop": ht, "bweb": bw, "bbot": bb, "tbot": hb, "h": h }),
        Shape::Circle { d } => json!({ "type": "circle", "d": d }),
        Shape::Custom => json!({ "type": "custom", "outline_z_width_mm": sec.outline }),
    };
    let layers: Vec<Value> = sec
        .long
        .iter()
        .map(|l| {
            let pre = sec.find_prestress(&l.steel).is_some();
            json!({
                "name": l.name,
                "z_mm": f(l.z),
                "depth_from_top_mm": f(h - l.z),
                "bars": l.num,
                "bar": l.bartitle,
                "area_mm2": f(l.area),
                "steel": l.steel,
                "prestressing": pre,
                "prestrain_mm_per_m": f(l.dep)
            })
        })
        .collect();
    let ld = &sec.loads;
    json!({
        "title": sec.title,
        "done_by": sec.done_by,
        "shape": shape,
        "height_mm": f(h),
        "max_width_mm": f(sec.max_width()),
        "gross_area_mm2": f(sec.gross_area()),
        "concretes": sec.concretes.iter().map(|c| json!({
            "name": c.name, "fc_mpa": f(c.fc), "ft_mpa": fo(c.ft), "e0_mm_per_m": fo(c.e0),
            "aggregate_mm": f(c.agg), "ecu_limit_mm_per_m": f(c.ecu)
        })).collect::<Vec<_>>(),
        "rebars": sec.rebars.iter().map(|r| json!({
            "name": r.name, "fy_mpa": f(r.fy), "fu_mpa": f(r.fu), "es_mpa": f(r.es),
            "esh_mm_per_m": f(r.esh), "eu_mm_per_m": f(r.eu)
        })).collect::<Vec<_>>(),
        "prestressing_steels": sec.prestress.iter().map(|p| json!({
            "name": p.name, "fpu_mpa": f(p.fpu), "ep_mpa": f(p.ep), "eu_mm_per_m": f(p.eu)
        })).collect::<Vec<_>>(),
        "longitudinal": layers,
        "longitudinal_area_mm2": f(sec.long.iter().map(|l| l.area).sum()),
        "stirrups": sec.stirrups.as_ref().map(|s| json!({
            "name": s.name,
            "type": model::STIRRUP_TYPES.get(s.pattern as usize).copied().unwrap_or("?"),
            "bar": s.bartitle,
            "area_all_legs_mm2": f(s.area),
            "spacing_mm": f(s.spacing),
            "z_top_mm": f(s.z_top),
            "z_bot_mm": f(s.z_bot),
            "steel": s.steel
        })),
        "loads": {
            "axial_kn": f(ld.axial),
            "m_const_knm": f(ld.m_const),
            "v_const_kn": f(ld.v_const),
            "n_inc_kn": f(ld.n_inc),
            "m_inc_knm": f(ld.m_inc),
            "v_inc_kn": f(ld.v_inc),
            "mv_ratio_m": f(ld.mv_ratio),
            "negative": ld.negative
        },
        "member": sec.member.as_ref().map(|m| json!({
            "length_mm": f(m.length), "left_support_mm": f(m.left_sup), "right_support_mm": f(m.right_sup), "load_at_mm": f(m.load_at)
        })),
        "design_basis": design_basis(sec)
    })
}

fn save_if_asked(sec: &Section, a: &Value, out: &mut Value) -> Result<(), String> {
    let text = rsp::save(sec);
    if let Some(p) = text_arg(a, "save_to") {
        std::fs::write(p, &text).map_err(|e| format!("Cannot write {}: {}", absolute(p), e))?;
        out["saved_to"] = json!(absolute(p));
    }
    if flag(a, "include_rsp").unwrap_or(a.get("quick").is_some()) {
        out["rsp"] = json!(text);
    }
    Ok(())
}

fn text_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    text(a, k).filter(|s| !s.trim().is_empty())
}

fn t_quick(a: &Value) -> Result<Value, String> {
    let q = a.get("quick").cloned().unwrap_or(json!({}));
    let sec = Section::from_quick(&quick_from(&q)?)?;
    let mut out = json!({ "section": summary(&sec) });
    save_if_asked(&sec, a, &mut out)?;
    Ok(out)
}

fn t_describe(a: &Value) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let mut out = json!({ "section": summary(&sec) });
    if let Some(c) = &sec.aci {
        let dm = design::AciModels::new(&sec, c);
        out["design"] = json!(design::describe(&dm, sec.loads.axial));
    }
    save_if_asked(&sec, a, &mut out)?;
    Ok(out)
}

fn moment_sign(sec: &Section) -> &'static str {
    if sec.loads.negative { "negative (top in tension)" } else { "positive (bottom in tension)" }
}

fn t_mphi(a: &Value) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let layers = int(a, "layers").unwrap_or(80).clamp(8, 400) as usize;
    let m = analysis::build(&sec, layers)?;
    let r = analysis::moment_curvature(&m, sec.loads.axial);
    if r.points.len() < 2 {
        return Err(format!("The section could not be analysed: {}", r.stop_reason));
    }
    let p = &r.points[r.peak];
    let (cl, sl) = analysis::mphi_limits(&m, &r);
    let lim = |x: Option<(f64, f64)>| x.map(|(k, mm)| json!({ "phi_rad_per_km": f(k), "m_knm": f(mm) }));
    let curve: Vec<Value> = sample(r.points.len(), max_points(a), Some(r.peak))
        .into_iter()
        .map(|i| {
            let q = &r.points[i];
            json!({
                "phi_rad_per_km": f(q.phi),
                "m_knm": f(q.m),
                "top_strain_mm_per_m": f(q.top * 1000.0),
                "max_steel_strain_mm_per_m": f(q.bot_steel * 1000.0)
            })
        })
        .collect();
    let mut out = json!({
        "section": sec.title,
        "axial_kn": f(sec.loads.axial),
        "moment": moment_sign(&sec),
        "design_basis": design_basis(&sec),
        "peak": {
            "m_knm": f(p.m),
            "phi_rad_per_km": f(p.phi),
            "top_strain_mm_per_m": f(p.top * 1000.0),
            "max_steel_strain_mm_per_m": f(p.bot_steel * 1000.0)
        },
        "cracking_moment_knm": fo(r.crack_m),
        "first_yield_moment_knm": fo(r.yield_m),
        "concrete_strain_limit": lim(cl),
        "steel_strain_limit": lim(sl),
        "stop_reason": r.stop_reason,
        "points_computed": r.points.len(),
        "curve": curve
    });
    if let Some(c) = &sec.aci {
        let dm = design::AciModels::new(&sec, c);
        let st = dm.state(sec.loads.axial, sec.loads.negative);
        out["design"] = json!({
            "code": dm.a.name(),
            "phi": f(st.phi),
            "net_tensile_strain_mm_per_m": f(st.et * 1000.0),
            "class": st.class(&dm.a),
            "phi_mn_knm": f(st.phi * p.m)
        });
    }
    Ok(out)
}

fn t_shear(a: &Value) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let s = analysis::build_shear(&sec)?;
    let mp = analysis::moment_curvature(&s, sec.loads.axial);
    let (c, d) = sec.loads.model_spec();
    let (r, alt) = analysis::sectional_response_m(&s, c, d, &mp);
    let m = alt.as_ref().unwrap_or(&s);
    if r.points.is_empty() {
        return Err(format!("No state could be found: {}", analysis::clean_reason(&r.reason)));
    }
    // the section is mirrored for negative moment: moments back to the person's frame
    let sg = if sec.loads.negative { -1.0 } else { 1.0 };
    let by_shear = d.2.abs() > 1e-12 || c.2.abs() > 1e-12;
    let key = |p: &analysis::SectionState| if by_shear { p.v } else { p.m.abs() };
    let ipk = (0..r.points.len()).max_by(|&i, &j| key(&r.points[i]).partial_cmp(&key(&r.points[j])).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);
    let pk = &r.points[ipk];
    let mech = analysis::mechanism(m, &r);
    let path: Vec<Value> = sample(r.points.len(), max_points(a), Some(ipk))
        .into_iter()
        .map(|i| {
            let q = &r.points[i];
            json!({
                "n_kn": f(q.n), "m_knm": f(sg * q.m), "v_kn": f(q.v),
                "phi_rad_per_km": f(q.phi), "gamma_avg_mm_per_m": f(q.gamma_avg)
            })
        })
        .collect();
    let crack = pk.layers.iter().map(|l| l.w).fold(0.0, f64::max);
    let fsz = pk.layers.iter().map(|l| l.fsz).fold(0.0, f64::max);
    let h = sec.height();
    let bars: Vec<Value> = pk
        .bars
        .iter()
        .map(|b| {
            // the model's z is from its own bottom (the tension face): back to the section's
            let z = if sec.loads.negative { h - b.0 } else { b.0 };
            json!({ "z_mm": f(z), "strain_mm_per_m": f(b.1 * 1000.0), "stress_mpa": f(b.2) })
        })
        .collect();
    Ok(json!({
        "section": sec.title,
        "design_basis": design_basis(&sec),
        "loading": {
            "constant": { "n_kn": f(sec.loads.axial), "m_knm": f(sec.loads.m_const), "v_kn": f(sec.loads.v_const) },
            "increment": { "n_kn": f(sec.loads.n_inc), "m_knm": f(sec.loads.m_inc), "v_kn": f(sec.loads.v_inc) },
            "moment": moment_sign(&sec)
        },
        "peak": {
            "v_kn": f(pk.v),
            "m_knm": f(sg * pk.m),
            "n_kn": f(pk.n),
            "phi_rad_per_km": f(pk.phi),
            "gamma_avg_mm_per_m": f(pk.gamma_avg),
            "max_crack_width_mm": f(crack),
            "max_stirrup_stress_mpa": f(fsz),
            "bars": bars
        },
        "mechanism": mech.label(),
        "failure_confirmed": r.confirmed,
        "stop_reason": analysis::clean_reason(&r.reason),
        "states_computed": r.points.len(),
        "path": path
    }))
}

fn finite_pairs(v: &[(f64, f64)], x: &str, y: &str) -> Vec<Value> {
    v.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).map(|p| json!({ x: f(p.0), y: f(p.1) })).collect()
}

fn thin(v: Vec<(f64, f64)>, max: usize) -> Vec<(f64, f64)> {
    sample(v.len(), max, None).into_iter().map(|i| v[i]).collect()
}

fn t_inter(a: &Value, pf: &(dyn Fn(f32) + Sync)) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let points = int(a, "points").unwrap_or(12).clamp(4, 40) as usize;
    let kind = text(a, "type").unwrap_or("M-V").to_uppercase().replace(['_', ' '], "-");
    let n = sec.loads.axial;
    let dm = sec.aci.as_ref().map(|c| design::AciModels::new(&sec, c));
    let mut out = json!({ "section": sec.title, "design_basis": design_basis(&sec) });
    match kind.as_str() {
        "M-V" | "MV" => {
            let it = analysis::interaction_both(&sec, n, points, pf).ok_or("The M-V interaction could not be computed.")?;
            let env = it.envelope();
            let pts: Vec<Value> = it
                .points
                .iter()
                .enumerate()
                .map(|(k, (m, v))| {
                    let inf = it.info.get(k);
                    json!({
                        "m_knm": f(*m),
                        "v_kn": f(*v),
                        "mechanism": inf.map(|i| i.mech.label()),
                        "confirmed": inf.map(|i| i.confirmed),
                        "suspect": inf.map(|i| i.suspect),
                        "inside_envelope": it.inside.get(k).copied().unwrap_or(false)
                    })
                })
                .collect();
            out["type"] = json!("M-V");
            out["axial_kn"] = f(n);
            out["envelope"] = json!(finite_pairs(&env, "m_knm", "v_kn"));
            out["points"] = json!(pts);
            out["first_cracking"] = json!(finite_pairs(&it.cracking, "m_knm", "v_kn"));
            out["first_long_yield"] = json!(finite_pairs(&it.long_yield, "m_knm", "v_kn"));
            out["first_stirrup_yield"] = json!(finite_pairs(&it.trans_yield, "m_knm", "v_kn"));
            if let Some(dm) = &dm {
                out["design_envelope"] = json!(finite_pairs(&dm.mv(&env, n), "phi_m_knm", "phi_v_kn"));
            }
        }
        "M-N" | "MN" | "N-V" | "NV" | "M-V-CENTRED" | "M-V-CENTERED" => {
            let (pair, xs, ys, label) = match kind.as_str() {
                "M-N" | "MN" => (analysis::Pair::MN, "m_knm", "n_kn", "M-N"),
                "N-V" | "NV" => (analysis::Pair::NV, "n_kn", "v_kn", "N-V"),
                _ => (analysis::Pair::MV, "m_knm", "v_kn", "M-V-centred"),
            };
            let d = analysis::radial_interaction(&sec, pair, points, pf).ok_or("The interaction diagram could not be computed.")?;
            let pts: Vec<Value> = d
                .points
                .iter()
                .map(|p| json!({ xs: f(p.x), ys: f(p.y), "mechanism": p.mech.label(), "confirmed": p.confirmed, "reason": analysis::clean_reason(&p.reason) }))
                .collect();
            out["type"] = json!(label);
            if pair == analysis::Pair::NV {
                out["constant_moment_knm"] = f(sec.loads.m_const);
            } else {
                out["axial_kn"] = f(n);
            }
            out["axial_sign"] = json!("tension positive");
            out["points"] = json!(pts);
            if let Some(dm) = &dm {
                let xy: Vec<(f64, f64)> = d.points.iter().map(|p| (p.x, p.y)).collect();
                let des = match pair {
                    analysis::Pair::MN => thin(dm.mn(&xy), 80),
                    analysis::Pair::NV => thin(dm.nv(&xy, sec.loads.negative), 80),
                    analysis::Pair::MV => dm.mv(&xy, n),
                };
                out["design_curve"] = json!(finite_pairs(&des, &format!("phi_{}", xs), &format!("phi_{}", ys)));
            }
        }
        o => return Err(format!("Unknown diagram type '{}': use M-V, M-N, N-V or M-V-centred.", o)),
    }
    Ok(out)
}

fn t_biaxial(a: &Value, pf: &(dyn Fn(f32) + Sync)) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let n = num(a, "axial_kn").unwrap_or(0.0);
    let al = num(a, "angle_deg").unwrap_or(0.0);
    let dirs = int(a, "directions").unwrap_or(36).clamp(8, 72) as usize;
    let d = biaxial::diagram(&sec, n, al, dirs, pf)?;
    let ac = sec.aci.as_ref().map(|c| c.for_section(&sec));
    let run_json = |r: &biaxial::BiRun| -> Value {
        let mut v = json!({ "alpha_deg": f(r.alpha), "stop": r.stop });
        if let Some(p) = r.peak_point() {
            v["m_knm"] = f(p.m);
            v["mx_knm"] = f(p.mx);
            v["my_knm"] = f(p.my);
            v["kappa_rad_per_km"] = f(p.kappa);
            v["neutral_axis_beta_deg"] = f(p.beta);
        }
        if let (Some(c), Some(p)) = (&ac, r.peak_point()) {
            if let Some((phi, et)) = biaxial::aci_phi(r, c) {
                v["phi"] = f(phi);
                v["net_tensile_strain_mm_per_m"] = f(et * 1000.0);
                v["phi_m_knm"] = f(phi * p.m);
            }
        }
        v
    };
    let ch = &d.chosen;
    let curve: Vec<Value> = sample(ch.points.len(), max_points(a), Some(ch.peak))
        .into_iter()
        .map(|i| {
            let p = &ch.points[i];
            json!({ "kappa_rad_per_km": f(p.kappa), "m_knm": f(p.m), "mx_knm": f(p.mx), "my_knm": f(p.my), "beta_deg": f(p.beta) })
        })
        .collect();
    let mut chosen = run_json(ch);
    chosen["curve"] = json!(curve);
    Ok(json!({
        "section": sec.title,
        "design_basis": design_basis(&sec),
        "axial_kn": f(n),
        "axial_sign": "tension positive",
        "angle_convention": "alpha is the moment direction: 0 = Mx with the top in compression",
        "contour": d.runs.iter().map(run_json).collect::<Vec<_>>(),
        "chosen": chosen
    }))
}

fn t_shear2d(a: &Value, pf: &(dyn Fn(f32) + Sync)) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let n = num(a, "axial_kn").unwrap_or(0.0);
    let mv = num(a, "mv_ratio_m").unwrap_or(0.0);
    let dirs = int(a, "directions").unwrap_or(16).clamp(4, 72) as usize;
    let d = shear2d::diagram(&sec, n, mv, dirs, pf);
    let runs: Vec<Value> = d
        .runs
        .iter()
        .map(|r| {
            json!({
                "theta_deg": f(r.theta), "v_kn": f(r.v), "vx_kn": f(r.vx()), "vy_kn": f(r.vy()), "m_knm": f(r.m),
                "tie_area_mm2": f(r.av), "mechanism": r.mech.map(|m| m.label()), "reason": analysis::clean_reason(&r.reason)
            })
        })
        .collect();
    let mut out = json!({
        "section": sec.title,
        "design_basis": design_basis(&sec),
        "axial_kn": f(n),
        "mv_ratio_m": f(mv),
        "angle_convention": "theta is measured from the vertical (+y) towards +x",
        "runs": runs
    });
    if num(a, "vux_kn").is_some() || num(a, "vuy_kn").is_some() {
        let (vx, vy) = (num(a, "vux_kn").unwrap_or(0.0), num(a, "vuy_kn").unwrap_or(0.0));
        let (th, cap, ratio) = shear2d::check(&d, vx, vy);
        let mut chk = json!({ "theta_deg": f(th), "vn_kn": f(cap), "dc_nominal": f(ratio) });
        if let Some(c) = &sec.aci {
            let (rx, ry, gov, lim, ok) = shear2d::aci_two_way(&d, c.phi_shear, vx, vy);
            chk["code"] = json!(c.name());
            chk["phi_v"] = f(c.phi_shear);
            chk["dc_design"] = f(ratio / c.phi_shear);
            chk["two_way_rule"] = json!({ "ratio_x": f(rx), "ratio_y": f(ry), "governing": f(gov), "limit": f(lim), "passes": ok });
        }
        out["demand_check"] = chk;
    }
    Ok(out)
}

fn dc(v: f64) -> Value {
    if v.is_nan() {
        Value::Null
    } else if v.is_infinite() {
        json!("beyond capacity")
    } else {
        f(v)
    }
}

fn t_cases(a: &Value, pf: &(dyn Fn(f32) + Sync)) -> Result<Value, String> {
    let sec = section_arg(a)?;
    let cases: Vec<demand::LoadCase> = if let Some(arr) = a.get("cases").and_then(Value::as_array) {
        arr.iter()
            .enumerate()
            .map(|(i, c)| demand::LoadCase {
                name: text(c, "name").map(String::from).unwrap_or_else(|| format!("case {}", i + 1)),
                pu: num(c, "pu_kn").unwrap_or(0.0),
                mx: num(c, "mx_knm").unwrap_or(0.0),
                my: num(c, "my_knm").unwrap_or(0.0),
                vx: num(c, "vx_kn").unwrap_or(0.0),
                vy: num(c, "vy_kn").unwrap_or(0.0),
                tu: num(c, "tu_knm").unwrap_or(0.0),
            })
            .collect()
    } else if let Some(t) = text(a, "table") {
        demand::parse(t)
    } else {
        return Err("Give the load cases: 'cases' (a list) or 'table' (text).".into());
    };
    if cases.is_empty() {
        return Err("No load cases were given.".into());
    }
    let rows = demand::evaluate(&sec, &cases, pf)?;
    let gov = rows.iter().enumerate().filter(|(_, r)| !r.dc_max.is_nan()).max_by(|x, y| x.1.dc_max.partial_cmp(&y.1.dc_max).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i);
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            let c = &r.case;
            json!({
                "name": c.name,
                "pu_kn": f(c.pu), "mx_knm": f(c.mx), "my_knm": f(c.my), "vx_kn": f(c.vx), "vy_kn": f(c.vy), "tu_knm": f(c.tu),
                "mn_knm": f(r.mn),
                "phi_m": f(r.phi_m),
                "phi_mn_knm": f(r.phi_m * r.mn),
                "phi_vn_kn": f(r.vn),
                "phi_tn_knm": f(r.phi_tn),
                "dc_axial_bending": dc(r.dc_pm),
                "dc_shear": dc(r.dc_v),
                "dc_torsion": dc(r.dc_t),
                "dc_shear_torsion": dc(r.dc_vt),
                "dc_max": dc(r.dc_max),
                "governs": r.governs,
                "notes": r.notes
            })
        })
        .collect();
    Ok(json!({
        "section": sec.title,
        "design_basis": design_basis(&sec),
        "sign_convention": "Pu compression positive; Mx + compresses the top, My + compresses the right side",
        "governing_case": gov.map(|i| rows[i].case.name.clone()),
        "governing_dc": gov.map(|i| dc(rows[i].dc_max)),
        "cases": out
    }))
}
