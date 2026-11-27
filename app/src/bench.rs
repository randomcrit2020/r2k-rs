//! `--bench-mv`: the M-V interaction diagram of a set of sections, measured for what the program
//! is meant to give: speed (wall time), stability (a symmetric section must give the same curve
//! for positive and negative moment; the envelope should have no dents) and evidence (how many
//! points ended without proof of failure). Run before and after any change to the solvers,
//! together with `--check-mechanics` (accuracy against the original).

use crate::analysis::{self, Interaction, Mechanism};
use crate::model::Section;

/// What one diagram measured.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub name: String,
    pub secs: f64,
    pub points: usize,
    pub unconfirmed: usize,
    pub inside: usize,
    /// largest dent in the envelope: how far a point falls short of the chord between its
    /// neighbours, measured along its own direction from the origin (%, 0 for a convex curve)
    pub dent_max: f64,
    /// all dents added up (%)
    pub dent_sum: f64,
    /// symmetric sections: largest and mean difference between the positive side and the
    /// mirrored negative side, along each positive point's direction (%)
    pub asym_max: Option<f64>,
    pub asym_mean: Option<f64>,
    pub vmax: f64,
    pub mpos: f64,
    pub mneg: f64,
    pub mechs: String,
}

fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// Where the ray from the origin in direction `u` crosses the polyline: the distance from the
/// origin (the farthest crossing), if any.
fn ray_hit(line: &[(f64, f64)], u: (f64, f64)) -> Option<f64> {
    let mut best: Option<f64> = None;
    for w in line.windows(2) {
        let (p, q) = (w[0], w[1]);
        let d = (q.0 - p.0, q.1 - p.1);
        let den = cross(u, d);
        if den.abs() < 1e-14 {
            continue;
        }
        // p + s d = t u
        let t = cross(p, d) / den;
        let s = cross(p, u) / den;
        if t > 0.0 && (-1e-9..=1.0 + 1e-9).contains(&s) && best.map(|b| t > b).unwrap_or(true) {
            best = Some(t);
        }
    }
    best
}

/// The measures of one finished diagram.
pub fn measure(name: &str, it: &Interaction, secs: f64, symmetric: bool) -> Row {
    let env = it.envelope();
    let vmax = env.iter().map(|p| p.1).fold(0.0, f64::max).max(1e-9);
    let mpos = env.iter().map(|p| p.0).fold(0.0, f64::max).max(1e-9);
    let mneg = env.iter().map(|p| -p.0).fold(0.0, f64::max).max(1e-9);
    let sc = |p: &(f64, f64)| (if p.0 >= 0.0 { p.0 / mpos } else { p.0 / mneg }, p.1 / vmax);
    let pts: Vec<(f64, f64)> = env.iter().map(sc).collect();
    let (mut dent_max, mut dent_sum) = (0.0f64, 0.0f64);
    for w in pts.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let r = (b.0 * b.0 + b.1 * b.1).sqrt();
        if b.1 < 1e-6 || r < 1e-9 {
            continue;
        }
        if let Some(t) = ray_hit(&[a, c], (b.0 / r, b.1 / r)) {
            let short = 100.0 * (1.0 - r / t);
            if short > 1.0 && std::env::var("SR_BENCH_DENTS").is_ok() {
                let k = pts.iter().position(|p| *p == b).unwrap_or(0);
                eprintln!("   dent {:.2} % at M {:.1} kNm V {:.1} kN (neighbours V {:.1} and {:.1})", short, env[k].0, env[k].1, env[k.saturating_sub(1)].1, env[(k + 1).min(env.len() - 1)].1);
            }
            if short > 0.0 {
                dent_max = dent_max.max(short);
                dent_sum += short;
            }
        }
    }
    let (mut asym_max, mut asym_mean) = (None, None);
    if symmetric {
        let pos: Vec<(f64, f64)> = pts.iter().copied().filter(|p| p.0 >= 0.0).collect();
        let mut neg: Vec<(f64, f64)> = pts.iter().filter(|p| p.0 <= 0.0).map(|p| (-p.0, p.1)).collect();
        neg.reverse();
        let (mut worst, mut sum, mut k) = (0.0f64, 0.0f64, 0usize);
        for (a, b) in [(&pos, &neg), (&neg, &pos)] {
            for p in a.iter() {
                let r = (p.0 * p.0 + p.1 * p.1).sqrt();
                if r < 1e-9 {
                    continue;
                }
                if let Some(t) = ray_hit(b, (p.0 / r, p.1 / r)) {
                    let d = 100.0 * (r / t - 1.0).abs();
                    worst = worst.max(d);
                    sum += d;
                    k += 1;
                }
            }
        }
        if k > 0 {
            asym_max = Some(worst);
            asym_mean = Some(sum / k as f64);
        }
    }
    let mut mechs: Vec<(Mechanism, usize)> = vec![];
    for i in &it.info {
        match mechs.iter_mut().find(|m| m.0 == i.mech) {
            Some(m) => m.1 += 1,
            None => mechs.push((i.mech, 1)),
        }
    }
    Row {
        name: name.into(),
        secs,
        points: it.points.len(),
        unconfirmed: it.info.iter().filter(|i| !i.confirmed).count(),
        inside: it.inside.iter().filter(|i| **i).count(),
        dent_max,
        dent_sum,
        asym_max,
        asym_mean,
        vmax,
        mpos,
        mneg,
        mechs: mechs.iter().map(|(m, n)| format!("{} {}", n, short(*m))).collect::<Vec<_>>().join(", "),
    }
}

fn short(m: Mechanism) -> &'static str {
    match m {
        Mechanism::Flexure => "flexure",
        Mechanism::LongYield => "bars yielded",
        Mechanism::LongHardening => "bars hardening",
        Mechanism::WebShear => "web shear",
        Mechanism::WebCrushing => "web crushing",
        Mechanism::Crushing => "crushing (axial)",
        Mechanism::Unclear => "unclear",
    }
}

/// The benchmark's own sections when none are named: the sample beam, a symmetric tied column,
/// a 600 x 750 column with side bars (the top of its diagram came in steps before the
/// continuation between ratios), the original's Quick Define circle and its prestressed T-beam.
pub const DEFAULT_CASES: [&str; 5] = ["shots/sample-beam.rsp", "shots/cases/col600.rsp", "shots/cases/col750.rsp", "shots/cases/quick-circ16.rsp", "shots/cases/quick-tee.rsp"];

pub fn run(files: &[String], points: usize) {
    let files: Vec<String> = if files.is_empty() { DEFAULT_CASES.iter().map(|s| s.to_string()).collect() } else { files.to_vec() };
    println!(
        "{:<22} {:>7} {:>4} {:>5} {:>6} {:>8} {:>8} {:>9} {:>9}  {:>7} {:>7} {:>7}  mechanisms",
        "section", "time s", "pts", "unsure", "inside", "dent max", "dent sum", "asym max", "asym mean", "Vmax", "M+", "M-"
    );
    let mut total = 0.0;
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let sec: Section = match crate::rsp::load(&text) {
            Ok(mut s) if !s.is_empty() => s.remove(0),
            _ => {
                println!("{:<22} could not be read", f);
                continue;
            }
        };
        let t0 = std::time::Instant::now();
        let Some(it) = analysis::interaction_both(&sec, sec.loads.axial, points, &|_| {}) else {
            println!("{:<22} no diagram", f);
            continue;
        };
        let secs = t0.elapsed().as_secs_f64();
        total += secs;
        let name = std::path::Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or(f);
        let r = measure(name, &it, secs, sec.is_symmetric());
        let pct = |v: Option<f64>| v.map(|x| format!("{:.2} %", x)).unwrap_or("-".into());
        println!(
            "{:<22} {:>7.1} {:>4} {:>5} {:>6} {:>6.2} % {:>6.2} % {:>9} {:>9}  {:>7.1} {:>7.1} {:>7.1}  {}",
            r.name,
            r.secs,
            r.points,
            r.unconfirmed,
            r.inside,
            r.dent_max,
            r.dent_sum,
            pct(r.asym_max),
            pct(r.asym_mean),
            r.vmax,
            r.mpos,
            r.mneg,
            r.mechs
        );
    }
    println!("total {:.1} s, {} points a side", total, points);
}

/// `--bench-sym file [points]`: every M/V ratio of a symmetric section run on one mesh in both
/// orientations (the section as given, and upside down for negative moment), side by side. The
/// two runs solve the same problem, so any difference is the solver's own sensitivity (to the
/// last digits of the layer and bar positions), with no re-meshing or refinement in the way.
pub fn run_sym(file: &str, points: usize) {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let Some(sec) = crate::rsp::load(&text).ok().and_then(|mut s| if s.is_empty() { None } else { Some(s.remove(0)) }) else {
        println!("{} could not be read", file);
        return;
    };
    if !sec.is_symmetric() {
        println!("{} is not symmetric about mid-height", file);
        return;
    }
    let n = sec.loads.axial;
    let mut neg = sec.clone();
    neg.loads.negative = true;
    let mut pos = sec.clone();
    pos.loads.negative = false;
    let (Ok(mp), Ok(mn)) = (analysis::build_shear(&pos), analysis::build_shear(&neg)) else { return };
    let (cp, cn) = (analysis::moment_curvature(&mp, n), analysis::moment_curvature(&mn, n));
    let m_cap = cp.points.get(cp.peak).map(|p| p.m).unwrap_or(1.0).max(1e-6);
    let t0 = std::time::Instant::now();
    let pure = crate::path::shear_path(&mp, n, 0.0, &cp);
    let v_cap = pure.points.iter().map(|p| p.v).fold(0.0, f64::max).max(1e-6);
    let mut ratios = analysis::mv_ratios(points, m_cap, v_cap);
    // SR_SYM_ONLY=k: only the k-th ratio (for tracing one pair with SR_PATH=1)
    if let Some(k) = std::env::var("SR_SYM_ONLY").ok().and_then(|v| v.parse::<usize>().ok()) {
        ratios = ratios.into_iter().skip(k).take(1).collect();
        eprintln!("=== ratio {:.9}", ratios.first().copied().unwrap_or(0.0));
    }
    let one = |a: &f64| {
        let t = std::time::Instant::now();
        let rp = crate::path::shear_path(&mp, n, *a, &cp);
        let tp = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let rn = crate::path::shear_path(&mn, n, *a, &cn);
        (rp, tp, rn, t.elapsed().as_secs_f64())
    };
    let res = crate::pool::map(&ratios, None, &one, &|_| {});
    let peak = |r: &analysis::ShearResult| r.points.iter().map(|p| p.v).fold(0.0, f64::max);
    if std::env::var("SR_LMSTAT").is_ok() {
        crate::path::lmstat_print();
    }
    let (mut worst, mut sum, mut cpu) = (0.0f64, 0.0f64, 0.0f64);
    println!("{:>8} {:>9} {:>9} {:>7} {:>6} {:>6}  how each ended", "M/V m", "V+ kN", "V- kN", "diff %", "s+", "s-");
    for (a, (rp, tp, rn, tn)) in ratios.iter().zip(&res) {
        let (vp, vn) = (peak(rp), peak(rn));
        let d = 100.0 * (vp - vn) / vp.max(vn).max(1e-9);
        worst = worst.max(d.abs());
        sum += d.abs();
        cpu += tp + tn;
        let end = |r: &analysis::ShearResult| r.reason.split(" Load factor").next().unwrap_or("").chars().take(46).collect::<String>();
        let same = end(rp) == end(rn);
        println!("{:>8.3} {:>9.1} {:>9.1} {:>7.2} {:>6.2} {:>6.2}  {}{}", a, vp, vn, d, tp, tn, end(rp), if same { String::new() } else { format!(" | {}", end(rn)) });
    }
    println!(
        "largest difference {:.2} %, mean {:.2} %; {} layers; solver time {:.1} s over {} runs, wall {:.1} s",
        worst,
        sum / ratios.len().max(1) as f64,
        mp.layers.len(),
        cpu,
        2 * ratios.len(),
        t0.elapsed().as_secs_f64()
    );
}

/// `--bench-sweep file a_max n [layers]`: the failure shear at n evenly spaced M/V ratios from
/// -a_max to +a_max (m; negative = the top in tension), on one mesh, to see the envelope's
/// shape next to pure shear and whether it holds from one mesh to another.
pub fn run_sweep(file: &str, a_max: f64, n: usize, layers: Option<usize>) {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let Some(sec) = crate::rsp::load(&text).ok().and_then(|mut s| if s.is_empty() { None } else { Some(s.remove(0)) }) else {
        println!("{} could not be read", file);
        return;
    };
    let nn = sec.loads.axial;
    let nl = layers.unwrap_or(40);
    let mut neg = sec.clone();
    neg.loads.negative = true;
    let mut pos = sec.clone();
    pos.loads.negative = false;
    let (Ok(mp), Ok(mn)) = (analysis::build(&pos, nl), analysis::build(&neg, nl)) else { return };
    let (cp, cn) = (analysis::moment_curvature(&mp, nn), analysis::moment_curvature(&mn, nn));
    let n = n.max(1);
    let ratios: Vec<f64> = (0..=2 * n).map(|k| a_max * (k as f64 - n as f64) / n as f64).collect();
    // SR_SWEEP_ONLY=a: that one ratio alone (for tracing with SR_PATH=1)
    let ratios: Vec<f64> = match std::env::var("SR_SWEEP_ONLY").ok().and_then(|v| v.parse::<f64>().ok()) {
        Some(a) => vec![a],
        None => ratios,
    };
    let one = |a: &f64| if *a < 0.0 { crate::path::shear_path(&mn, nn, -*a, &cn) } else { crate::path::shear_path(&mp, nn, *a, &cp) };
    let res = crate::pool::map(&ratios, None, &one, &|_| {});
    println!("{:>8} {:>9} {:>9}  how it ended ({} layers)", "M/V m", "M kNm", "V kN", nl);
    for (a, r) in ratios.iter().zip(&res) {
        let v = r.points.iter().map(|p| p.v).fold(0.0, f64::max);
        println!("{:>8.3} {:>9.1} {:>9.1}  {}", a, a * v, v, r.reason.split(" Load factor").next().unwrap_or(""));
    }
}

/// `--check-shares file N V [layers]`: an equilibrium check of the shear shares at shear V
/// (kN) under axial load N (kN, tension positive), no moment. The shear stress at a depth is
/// the rate at which the longitudinal force below it changes along the member, so two
/// neighbouring sections are solved in full - the same N and V, the moment a little apart - and
/// the difference of their longitudinal stresses gives the shear stresses that equilibrium
/// asks for. They are set beside the shear stresses the run was solved with (the rule in
/// force, SR_FLOW_SECANT). A rule is consistent where the two agree.
pub fn run_share_check(file: &str, n_kn: f64, v_kn: f64, layers: Option<usize>) {
    // SR_CHECK_M: the moment (kNm) the two sections are taken at (default 0)
    let m0 = std::env::var("SR_CHECK_M").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let Some(sec) = crate::rsp::load(&text).ok().and_then(|mut s| if s.is_empty() { None } else { Some(s.remove(0)) }) else {
        println!("{} could not be read", file);
        return;
    };
    let Ok(m) = analysis::build(&sec, layers.unwrap_or(40)) else { return };
    let mp = analysis::moment_curvature(&m, n_kn);
    // the shear applied and held, then the moment raised
    let r = crate::path::sectional_path(&m, (n_kn, m0, v_kn), (0.0, 1.0, 0.0), &mp);
    let Some(i0) = r.points.iter().rposition(|p| (p.m - m0).abs() < 1e-6 * (1.0 + m0.abs()) && (p.v - v_kn).abs() < 1e-3 * v_kn.abs().max(1.0)) else {
        println!("the section did not carry N {} kN with V {} kN: {}", n_kn, v_kn, r.reason);
        return;
    };
    let Some(b) = r.points.get(i0 + 1) else {
        println!("no state with the moment raised: {}", r.reason);
        return;
    };
    let a = &r.points[i0];
    let dl: Vec<f64> = m.layers.iter().enumerate().map(|(i, l)| (b.layers[i].sx - a.layers[i].sx) * l.b * l.t).collect();
    let db: Vec<f64> = m.bars.iter().enumerate().map(|(k, bar)| (b.bars[k].2 - a.bars[k].2) * bar.area).collect();
    let exact = analysis::flow_from_changes(&m, &dl, &db, v_kn);
    println!("N {} kN, V {} kN, the moment raised from {:.2} to {:.2} kNm between the two sections ({} layers)", n_kn, v_kn, a.m, b.m, m.layers.len());
    println!("{:>7} {:>9} {:>11} {:>8} {:>9}", "z mm", "used MPa", "equil. MPa", "ratio", "ex mm/m");
    let (mut err, mut tot, mut peak_used, mut peak_exact) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (i, l) in m.layers.iter().enumerate() {
        let used = a.layers[i].v;
        let ar = l.b * l.t;
        err += (used - exact[i]).abs() * ar;
        tot += exact[i] * ar;
        peak_used = peak_used.max(used);
        peak_exact = peak_exact.max(exact[i]);
        if i % 2 == 0 {
            println!("{:>7.0} {:>9.3} {:>11.3} {:>8.3} {:>9.3}", l.z, used, exact[i], if exact[i] > 1e-6 { used / exact[i] } else { 0.0 }, a.layers[i].ex * 1e3);
        }
    }
    println!(
        "shear placed differently from equilibrium: {:.1} % of V; largest shear stress used {:.3} MPa, by equilibrium {:.3} MPa ({:+.1} %)",
        100.0 * err / tot.max(1e-9) / 2.0,
        peak_used,
        peak_exact,
        100.0 * (peak_used / peak_exact.max(1e-9) - 1.0)
    );
}
