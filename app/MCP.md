# MCP server

`section-response --mcp` runs the program as a Model Context Protocol server on stdin/stdout
(JSON-RPC 2.0, one message a line), so AI assistants and other MCP clients can define sections and
run the analyses. No window opens. The code is in `src/mcp.rs`; each tool calls the same routines
the window's analysis thread does (`ui.rs`), with the same defaults.

## Setup

Build the program (`cargo build --release` in `app/`), then point the client at the binary:

* Claude Code: `.mcp.json` at the repository root already registers it (approve the server when
  asked). Section file paths are then relative to the repository root, e.g.
  `app/shots/cases/mv15.rsp`. It starts the server through `app/tools/mcp.cmd`, which runs a
  copy of the release exe (`app/target/mcp/`), so `cargo build --release` can replace the exe
  while the server is running; the copy is refreshed when the server next starts.
* Claude Desktop and other clients: add a server with command
  `C:\calcs\response-craft\app\target\release\section-response.exe` and args `["--mcp"]`.

## Tools

Every analysis tool takes a `section`: `{"file": "path.rsp"}`, `{"rsp": "<file text>"}` or
`{"quick": {...}}` (the Quick Define wizard's inputs), with `index` for a file of several sections.
Most also take `code` (`file`, `nominal`, `aci`, `aashto`, `csa`) and `loads` overrides
(`axial_kn`, `m_const_knm`, `v_const_kn`, `n_inc_kn`, `m_inc_knm`, `v_inc_kn`, or the shortcuts
`mv_ratio_m`, `negative`, `moment_only`).

| Tool | What it runs |
| --- | --- |
| `quick_define` | Builds a section (rectangle, circle, tee, ibeam, column); returns its summary and .rsp text; `save_to` writes a file |
| `describe_section` | Geometry, materials, bar layers, stirrups, loads, design basis; with `loads`/`code`/`save_to` edits and saves |
| `moment_curvature` | M-phi at the constant axial load: peak, cracking, first yield, strain-limit points, curve |
| `sectional_response` | Full MCFT run under constant + increasing loads: peak V and M, governing mechanism, state at the peak, load path |
| `interaction_diagram` | M-V (default), M-N, N-V or M-V-centred, with the phi-factored design curve under ACI/AASHTO |
| `biaxial_bending` | Mx-My contour at an axial load and the full run at one direction; phi from the net tensile strain under ACI/AASHTO |
| `shear_2d` | Vx-Vy shear capacity diagram at an axial load and M/V; checks a demand (`vux_kn`, `vuy_kn`) |
| `check_load_cases` | D/C ratios (P-M, shear, torsion) for a list of cases (`cases` or a pasted `table`) |

Units are mm, MPa, kN, kNm, rad/km and mm/m. Axial load is tension positive everywhere except
`check_load_cases`, where Pu is compression positive (as design tables give it).

Results come back both as `structuredContent` and as JSON text. Long calls send
`notifications/progress` when the client gives a progress token.

## Worker processes

Each tool call runs in a process of its own (`section-response --mcp-worker`, started by the
server), so the server keeps answering while analyses run and nothing that happens in one call can
take it down:

* **Internal errors.** A panic ends only the worker (the release build aborts on a panic); the
  call returns a tool error with the panic's message and source location, e.g. `The analysis
  stopped on an internal error: ... (at src\path.rs:123:9)`.
* **Cancellation.** `notifications/cancelled` kills the call's worker at once; no response is sent.
* **Time limit.** A call still running after `SR_MCP_TIMEOUT_S` seconds (default 1800; 0 for no
  limit) is stopped and returns an error.
* A worker exits when its server does (it watches its stdin), so none are left running.

If a worker cannot be started, the call runs inside the server as a fallback.

`SR_MCP_PANIC=<tool name>` makes the worker panic on that tool, for testing the above.
