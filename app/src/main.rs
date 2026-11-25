#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod biaxial;
mod shear2d;
mod torsion;
mod demand;
mod compare;
mod design;
mod materials;
mod mcheck;
mod mcp;
mod model;
mod nine;
mod path;
mod pool;
mod plots;
mod rsp;
mod section_page;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut shot: Option<std::path::PathBuf> = None;
    let mut open: Option<std::path::PathBuf> = None;
    let mut solve: Option<String> = None;
    if args.len() >= 2 && args[1] == "--mcp" {
        // MCP server on stdin/stdout (mcp.rs), for AI assistants and other MCP clients
        mcp::serve();
        return Ok(());
    }
    if args.len() >= 2 && args[1] == "--mcp-worker" {
        // one tool call of the MCP server, in a process of its own (mcp.rs)
        mcp::worker();
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--report" {
        report(&args[2]);
        return Ok(());
    }
    if args.len() >= 4 && args[1] == "--quick-sample" {
        // a Quick Define default section with stirrup type args[2], saved to args[3]
        let mut q = model::QuickDefine::default();
        q.stirrup_type = args[2].parse().unwrap_or(3);
        if args[2] == "column" {
            // the wizard's column defaults: 400 x 400, 3 x 25M top and bottom, 1 x 25M a side
            q.shape = 4;
            q.dims[0] = 400.0;
            q.dims[1] = 400.0;
            q.top_n = 3;
            q.top_bar = "25M".into();
            q.side_n = 1;
            q.side_bar = "25M".into();
            q.st_space = 300.0;
        }
        if args[2] == "column16" {
            // a bigger column: 600 x 600, 5 x 30M a face and 3 x 30M a side (16 bars)
            q.shape = 4;
            q.dims[0] = 600.0;
            q.dims[1] = 600.0;
            q.top_n = 5;
            q.top_bar = "30M".into();
            q.bot_n = 5;
            q.bot_bar = "30M".into();
            q.side_n = 3;
            q.side_bar = "30M".into();
            q.st_space = 300.0;
        }
        match model::Section::from_quick(&q) {
            Ok(s) => {
                let _ = std::fs::write(&args[3], rsp::save(&s));
            }
            Err(e) => println!("{}", e),
        }
        return Ok(());
    }
    if args.len() >= 4 && args[1] == "--cases" {
        // --cases <file.rsp> <table.txt> [aci|aashto]: D/C ratios of the load cases in the table
        let text = std::fs::read_to_string(&args[2]).unwrap_or_default();
        let Some(mut sec) = rsp::load(&text).ok().and_then(|v| v.into_iter().next()) else {
            println!("cannot read {}", args[2]);
            return Ok(());
        };
        match args.get(4).map(|s| s.as_str()) {
            Some("aci") => sec.aci = Some(model::AciFactors::default()),
            Some("aashto") => sec.aci = Some(model::AciFactors::aashto()),
            _ => {}
        }
        let table = std::fs::read_to_string(&args[3]).unwrap_or_default();
        let cases = demand::parse(&table);
        let t = std::time::Instant::now();
        match demand::evaluate(&sec, &cases, &|_| {}) {
            Ok(rows) => print!("{}", demand::to_tsv(&rows)),
            Err(e) => println!("{}", e),
        }
        println!("({} cases, {:?})", cases.len(), t.elapsed());
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--shear2d" {
        // --shear2d <file.rsp> [N kN] [M/V m] [directions]: the Vx-Vy shear capacity diagram
        let text = std::fs::read_to_string(&args[2]).unwrap_or_default();
        let Some(sec) = rsp::load(&text).ok().and_then(|v| v.into_iter().next()) else {
            println!("cannot read {}", args[2]);
            return Ok(());
        };
        let n: f64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let mv: f64 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let dirs: usize = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(8);
        let t = std::time::Instant::now();
        let d = shear2d::diagram(&sec, n, mv, dirs, &|_| {});
        for r in &d.runs {
            println!(
                "theta {:6.1}: V {:7.1} kN (Vx {:7.1}, Vy {:7.1}), M {:7.1} kNm, ties {:6.1} mm2, {:?}; {}",
                r.theta,
                r.v,
                r.vx(),
                r.vy(),
                r.m,
                r.av,
                r.mech,
                r.reason.chars().take(70).collect::<String>()
            );
        }
        println!("({:?})", t.elapsed());
        return Ok(());
    }
    if args.len() >= 3 && args[1] == "--biaxial" {
        // --biaxial <file.rsp> [N kN] [angle deg] [directions]: the Mx-My capacity diagram
        let text = std::fs::read_to_string(&args[2]).unwrap_or_default();
        let Some(sec) = rsp::load(&text).ok().and_then(|v| v.into_iter().next()) else {
            println!("cannot read {}", args[2]);
            return Ok(());
        };
        let n: f64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let al: f64 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let dirs: usize = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(16);
        let t = std::time::Instant::now();
        match biaxial::diagram(&sec, n, al, dirs, &|_| {}) {
            Ok(d) => {
                for r in &d.runs {
                    if let Some(p) = r.peak_point() {
                        println!(
                            "alpha {:6.1}: M {:8.1} kNm (Mx {:8.1}, My {:8.1}) at {:6.2} rad/km, neutral axis beta {:7.2}; {}",
                            r.alpha, p.m, p.mx, p.my, p.kappa, p.beta, r.stop
                        );
                    }
                }
                if let Some(p) = d.chosen.peak_point() {
                    println!("chosen alpha {}: M {:.1} kNm, {} points", al, p.m, d.chosen.points.len());
                }
                if std::env::var("SR_BI_DUMP").is_ok() {
                    for p in d.chosen.points.iter().step_by(6) {
                        println!(
                            "  k {:7.2} M {:8.1} Mx {:8.1} My {:7.1} beta {:7.2} e0 {:8.5} ec {:8.5} et {:8.5}",
                            p.kappa, p.m, p.mx, p.my, p.beta, p.e0, p.ec_min, p.et_max
                        );
                    }
                }
                println!("({:?})", t.elapsed());
            }
            Err(e) => println!("{}", e),
        }
        return Ok(());
    }
    if args.len() >= 2 && args[1] == "--check-quick" {
        for (name, worst, bad, notes) in mcheck::quick_cases() {
            println!("{}: {} mismatches, largest height difference {:.3} mm", name, bad, worst);
            for n in notes {
                println!("    {}", n);
            }
        }
        return Ok(());
    }
    if args.len() >= 2 && args[1] == "--check-mechanics" {
        mcheck::run(args.get(2).map(|s| s.as_str()).unwrap_or("mechanics-check.json"));
        return Ok(());
    }
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--solve" && i + 1 < args.len() {
            solve = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        if args[i] == "--screenshot" && i + 1 < args.len() {
            shot = Some(args[i + 1].clone().into());
            i += 2;
            continue;
        }
        if !args[i].starts_with("--") {
            open = Some(args[i].clone().into());
        }
        i += 1;
    }
    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([1500.0, 860.0])
        .with_min_inner_size([900.0, 560.0])
        .with_title("Section Response - Newbeam");
    if shot.is_some() {
        vp = vp.with_active(false);
        #[cfg(windows)]
        {
            vp = vp.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport: vp, ..Default::default() };
    run(options, shot, open, solve)
}

/// Text report of the analyses for a section file (for checking numbers without the window).
fn report(path: &str) {
    let secs = if path == "sample" {
        vec![model::Section::quick(
            "New beam",
            40.0,
            19.0,
            400.0,
            model::Shape::Rect { b: 300.0, h: 600.0 },
            (3, "25M", 60.0),
            Some((2, "15M", 50.0)),
            Some(("10M", 250.0, 40.0)),
        )]
    } else {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        match rsp::load(&text) {
            Ok(s) => s,
            Err(e) => {
                println!("error: {}", e);
                return;
            }
        }
    };
    if let Ok(out) = std::env::var("SR_SAVE") {
        let _ = std::fs::write(out, rsp::save(&secs[0]));
    }
    for sec in secs {
        println!("== {} (h = {:.0} mm, Ag = {:.0} mm2)", sec.title, sec.height(), sec.gross_area());
        let c = materials::ConcreteProps::new(&sec.concretes[0]);
        println!("   concrete fc {:.1} ft {:.2} e0 {:.3} mm/m Ec {:.0} MPa", c.fc, c.ft, c.e0 * 1000.0, c.ec);
        if sec.aci.is_some() || std::env::var("SR_ACI").is_ok() {
            let a = sec.aci.clone().unwrap_or_default();
            let dm = design::AciModels::new(&sec, &a);
            for n in [sec.loads.axial, -1000.0, -2000.0, -3000.0, 500.0] {
                println!("   {}", design::describe(&dm, n));
            }
            println!("   P0 {:.1} kN", dm.p0 / 1000.0);
        }
        if model::is_factored(&sec.phi) {
            println!("   factored: phic {:.2} phis {:.2} phip {:.2}", sec.phi[0], sec.phi[1], sec.phi[2]);
        }
        let n = sec.loads.axial;
        let t = std::time::Instant::now();
        let m = analysis::build(&sec, 80).unwrap();
        let r = analysis::moment_curvature(&m, n);
        let p = &r.points[r.peak];
        println!(
            "   M-phi: peak {:.1} kNm at {:.2} rad/km, crack {:?}, yield {:?}, stop: {} ({} pts, {:?})",
            p.m, p.phi, r.crack_m, r.yield_m, r.stop_reason, r.points.len(), t.elapsed()
        );
        if std::env::var("SR_DUMP").is_ok() {
            // forces at the peak: concrete in compression / tension, each bar
            let (ec, ph) = (p.ec, p.phi_raw);
            let (mut cc, mut cz, mut ct) = (0.0, 0.0, 0.0);
            for l in &m.layers {
                let e = m.strain(ec, ph, l.z);
                let f = m.conc.uniaxial(e, l.m_bond) * l.b * l.t;
                if f < 0.0 {
                    cc += f;
                    cz += f * l.z;
                } else {
                    ct += f;
                }
            }
            println!("   peak: ec {:.6} phi {:.3e}; concrete C {:.1} kN at z {:.1}; concrete T {:.1} kN", ec, ph, cc / 1e3, cz / cc.min(-1e-9), ct / 1e3);
            for b in &m.bars {
                let e = m.strain(ec, ph, b.z);
                println!("   bar z {:.0}: strain {:.5} stress {:.1} MPa force {:.1} kN", b.z, e, b.steel.stress(e + b.pre), b.steel.stress(e + b.pre) * b.area / 1e3);
            }
        }
        let t = std::time::Instant::now();
        let m40 = analysis::build_shear(&sec).unwrap();
        let mp = analysis::moment_curvature(&m40, n);
        if let Ok(v) = std::env::var("SR_RX") {
            // SR_RX="ec,phi(1/km)": the longitudinal reserve profile at that plane strain
            let a: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let (ec, phi) = (a.first().copied().unwrap_or(0.0), a.get(1).copied().unwrap_or(0.0) * 1e-6);
            let m80 = analysis::build(&sec, 80).unwrap();
            let (nn, mm) = m80.forces_uniaxial(ec, phi);
            println!("   uniaxial N {:.1} kN M {:.1} kNm", nn / 1000.0, mm / 1e6);
            if let Some(rx) = m80.long_reserve(ec, phi) {
                for (l, r) in m80.layers.iter().zip(&rx).step_by(8) {
                    println!("   rx z {:5.0} ex {:8.5} rx {:7.3} MPa", l.z, m80.strain(ec, phi, l.z), r);
                }
            }
            return;
        }
        if std::env::var("SR_BOND").is_ok() {
            for l in m40.layers.iter().step_by(2) {
                println!("   bond z {:5.0} M {:10.1} mm  k {:10.0}", l.z, l.m_bond, 3.6 * l.m_bond);
            }
        }
        if std::env::var("SR_LAYER").is_ok() {
            // SR_LAYER_I picks the layer (default: the middle one); SR_LAYER_EX the strains ex
            let i = std::env::var("SR_LAYER_I").ok().and_then(|v| v.parse().ok()).unwrap_or(m40.layers.len() / 2);
            let sz = m40.stirrup.as_ref().map(|s| s.1).unwrap_or(f64::INFINITY);
            let exs: Vec<f64> = std::env::var("SR_LAYER_EX").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or(vec![0.0, 0.001]);
            println!("   layer {} of {}: z {:.1} b {:.1} t {:.1} rho_z {:.5} ecr {:.6}", i, m40.layers.len(), m40.layers[i].z, m40.layers[i].b, m40.layers[i].t, m40.layers[i].rho_z, m40.conc.ecr);
            for ex in exs {
                let mut anchor = 0.0;
                for k in 0..60 {
                    let g = 0.00002 * k as f64 * (1.0 + k as f64 * 0.05);
                    let o = path::layer_solve(&m40, i, sz, ex, g, anchor, None);
                    anchor = o.ez;
                    println!("   layer ex {:.4} g {:.6} -> tau {:.3} ez {:.6} e1 {:.6} f1 {:.3} fsz {:.1} ok {}", ex, g, o.tau, o.ez, o.e1, o.f1, o.fsz, o.ok);
                }
            }
            return;
        }
        let mvr = std::env::var("SR_MV").ok().and_then(|v| v.parse().ok()).unwrap_or(sec.loads.mv_ratio);
        for (var, pair) in [("SR_MN", analysis::Pair::MN), ("SR_NV", analysis::Pair::NV), ("SR_MVC", analysis::Pair::MV)] {
            if std::env::var(var).is_ok() {
                let t0 = std::time::Instant::now();
                let pts = std::env::var("SR_POINTS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
                let d = if pair == analysis::Pair::MV {
                    analysis::centred_mv(&sec, pts, &|_| {})
                } else {
                    analysis::radial_interaction(&sec, pair, pts, &|_| {})
                };
                if let Some(d) = d {
                    let inside = if pair == analysis::Pair::MV {
                        let pts: Vec<(f64, f64)> = d.points.iter().map(|p| (p.x, p.y)).collect();
                        let sure: Vec<bool> = d.points.iter().map(|p| p.confirmed).collect();
                        analysis::inside_flags(&pts, &sure)
                    } else {
                        vec![false; d.points.len()]
                    };
                    for (p, ins) in d.points.iter().zip(&inside) {
                        println!(
                            "   {:?} point: {:8.1} {:8.1}  {}{}{}  [{}]",
                            pair,
                            p.x,
                            p.y,
                            p.mech.label(),
                            if p.confirmed { "" } else { "  UNCONFIRMED" },
                            if *ins { "  INSIDE" } else { "" },
                            p.reason
                        );
                    }
                }
                println!("   {:?} done in {:.1} s", pair, t0.elapsed().as_secs_f64());
            }
        }
        if std::env::var("SR_CASES").is_ok() {
            // the original's own runs on the sample beam, repeated with this program's solver
            for c in compare::sample_cases() {
                let t0 = std::time::Instant::now();
                let r = analysis::sectional_response(&m40, c.cst, c.inc, &mp);
                let pk = if c.inc.2.abs() > 1e-12 {
                    r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap())
                } else {
                    r.points.iter().max_by(|a, b| a.m.abs().partial_cmp(&b.m.abs()).unwrap())
                };
                let (m, v) = pk.map(|p| (p.m, p.v)).unwrap_or((0.0, 0.0));
                let (d, _) = c.scored_pct(m, v);
                println!(
                    "   case {:<42} original M {:6.1} V {:6.1}{} | this M {:6.1} V {:6.1} | {:+6.1} % ({:.1} s) {}",
                    c.name,
                    c.m,
                    c.v,
                    if c.lower_bound { " (lower bound)" } else { "" },
                    m,
                    v,
                    d,
                    t0.elapsed().as_secs_f64(),
                    r.reason
                );
            }
        }
        if std::env::var("SR_PROF").is_ok() {
            for (k, name) in path::PROF_NAMES.iter().enumerate() {
                println!("   prof {:<40} {:8.2} s", name, path::PROF[k].load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9);
            }
        }
        // SR_MV / SR_M0 override the file's loads with a plain ratio (and a constant moment);
        // SR_LOADS="N0,M0,V0,dN,dM,dV" gives the original's full constant + increment loading
        let s = if let Ok(t) = std::env::var("SR_LOADS") {
            let v: Vec<f64> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let g = |k: usize| v.get(k).copied().unwrap_or(0.0);
            // SR_MP80=1: the moment-curvature guide from 80 layers at the constant axial load, as
            // --check-mechanics builds it
            let mp = if std::env::var("SR_MP80").is_ok() { analysis::moment_curvature(&analysis::build(&sec, 80).unwrap(), g(0)) } else { mp.clone() };
            analysis::sectional_response(&m40, (g(0), g(1), g(2)), (g(3), g(4), g(5)), &mp)
        } else if std::env::var("SR_MV").is_ok() || std::env::var("SR_M0").is_ok() {
            let m0 = std::env::var("SR_M0").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            analysis::shear_response_const(&m40, n, mvr, m0, &mp)
        } else {
            let (c, d) = sec.loads.model_spec();
            analysis::sectional_response(&m40, c, d, &mp)
        };
        let vmax = s.points.iter().map(|p| p.v).fold(0.0, f64::max);
        println!(
            "   shear at M/V {:.2} m: Vmax {:.1} kN ({} steps, {:?}) reason: {}",
            mvr,
            vmax,
            s.points.len(),
            t.elapsed(),
            s.reason
        );
        if std::env::var("SR_INTER").is_ok() {
            let it = analysis::interaction_both(&sec, n, std::env::var("SR_POINTS").ok().and_then(|v| v.parse().ok()).unwrap_or(12), &|_| {}).unwrap_or_default();
            for (k, (mm, vv)) in it.points.iter().enumerate() {
                let inf = it.info.get(k);
                println!(
                    "   M-V point: M {:.1} kNm  V {:.1} kN  {}{}{}{}",
                    mm,
                    vv,
                    inf.map(|i| i.mech.label()).unwrap_or(""),
                    if inf.map(|i| !i.confirmed).unwrap_or(false) { "  UNCONFIRMED" } else { "" },
                    if inf.map(|i| i.suspect).unwrap_or(false) { "  SUSPECT" } else { "" },
                    if it.inside.get(k).copied().unwrap_or(false) { "  INSIDE" } else { "" }
                );
            }
            for (mm, vv) in it.envelope() {
                println!("   envelope: M {:.1} kNm  V {:.1} kN", mm, vv);
            }
            if compare::is_sample_beam(&sec) || std::env::var("SR_COMPARE").is_ok() {
                let r = compare::sample_original();
                let rows = compare::compare(&r, &it.envelope());
                println!("   Compared with: {}", r.name);
                for row in &rows {
                    println!(
                        "   {:<36} original {:7.1}  this {:>7}  diff {:>7}{}",
                        row.what,
                        row.original,
                        row.ours.map(|v| format!("{:.1}", v)).unwrap_or("-".into()),
                        row.diff.map(|d| format!("{:+.1} %", d)).unwrap_or("-".into()),
                        if row.computed { "" } else { "  (original's chord, not computed)" }
                    );
                }
                if let Some(d) = compare::mean_abs_diff(&rows) {
                    println!("   mean difference over the original's computed points: {:.1} %", d);
                }
            }
        }
        if let Some(last) = s.points.last() {
            println!("   last: V {:.1} M {:.1} gamma {:.2} mm/m", last.v, last.m, last.gamma_avg);
            if std::env::var("SR_DEBUG").is_ok() {
                for l in last.layers.iter().step_by(3) {
                    println!(
                        "     z {:4.0} ex {:7.3} ez {:7.3} e1 {:7.3} e2 {:7.3} th {:5.1} v {:5.2} f1 {:5.2} f2 {:6.2} fsz {:6.1} w {:5.2} sx {:6.2}",
                        l.z, l.ex * 1e3, l.ez * 1e3, l.e1 * 1e3, l.e2 * 1e3, l.theta.to_degrees(), l.v, l.f1, l.f2, l.fsz, l.w, l.sx
                    );
                }
                for b in &last.bars {
                    println!("     bar z {:.0} eps {:.3} fs {:.0}", b.0, b.1 * 1e3, b.2);
                }
            }
        }
    }
}

fn run(
    options: eframe::NativeOptions,
    shot: Option<std::path::PathBuf>,
    open: Option<std::path::PathBuf>,
    solve: Option<String>,
) -> eframe::Result<()> {
    eframe::run_native(
        "Section Response",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, shot, open, solve)))),
    )
}
