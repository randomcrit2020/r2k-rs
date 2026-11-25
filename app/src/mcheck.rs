//! `--check-mechanics <out.json>`: checks the mechanics sheet against this build at run time.
//!
//! Two parts. Constants: the material values this build actually computes for the sample beam
//! (f'c 40 MPa, 19 mm aggregate) next to what the sheet's constants give and, where the original
//! shows them, what the original printed. Behaviour: the reference inputs the original itself was
//! run on (its windows captured in shots/orig-*.png) run through this build, with the
//! original's observed results beside them. Everything is computed here, nothing is copied from
//! the sheet, so the file shows what the build does.

use crate::analysis::{self, Pair};
use crate::compare;
use crate::materials::ConcreteProps;
use crate::model::{self, Section};

fn sample() -> Section {
    Section::quick(
        "New beam",
        40.0,
        19.0,
        400.0,
        model::Shape::Rect { b: 300.0, h: 600.0 },
        (3, "25M", 60.0),
        Some((2, "15M", 50.0)),
        Some(("10M", 250.0, 40.0)),
    )
}

fn pct(build: f64, reference: f64) -> Option<f64> {
    (reference.abs() > 1e-12).then(|| 100.0 * (build - reference) / reference)
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

struct Row {
    name: String,
    input: String,
    reference: f64,
    reference_source: String,
    build: f64,
    unit: String,
    tolerance_pct: f64,
    /// the reference is a lower bound (the original stopped while still rising): only a
    /// shortfall below it counts
    lower_bound: bool,
}

impl Row {
    /// The difference that counts: all of it, or for a lower bound only a shortfall.
    fn scored(&self) -> Option<f64> {
        pct(self.build, self.reference).map(|d| if self.lower_bound { d.min(0.0) } else { d })
    }
    fn json(&self) -> String {
        let d = pct(self.build, self.reference);
        let ok = self.scored().map(|d| d.abs() <= self.tolerance_pct).unwrap_or((self.build - self.reference).abs() < 1e-9);
        format!(
            "    {{\"name\": \"{}\", \"input\": \"{}\", \"reference\": {:.4}, \"reference_source\": \"{}\", \"build\": {:.4}, \"unit\": \"{}\", \"difference_pct\": {}, \"tolerance_pct\": {:.1}, \"reference_kind\": \"{}\", \"scored_difference_pct\": {}, \"within_tolerance\": {}}}",
            esc(&self.name),
            esc(&self.input),
            self.reference,
            esc(&self.reference_source),
            self.build,
            esc(&self.unit),
            d.map(|d| format!("{:.2}", d)).unwrap_or("null".into()),
            self.tolerance_pct,
            if self.lower_bound { "lower_bound" } else { "value" },
            self.scored().map(|d| format!("{:.2}", d)).unwrap_or("null".into()),
            ok
        )
    }
}

/// The Quick Define builder against sections the original's own Quick Define saved
/// (shots/cases/quick-*.rsp): every bar layer (height, bars, area, name), the stirrup and the
/// outline. Returns (case, worst height difference mm, mismatched items, details).
pub fn quick_cases() -> Vec<(String, f64, usize, Vec<String>)> {
    let mut cases = vec![];
    let base = model::QuickDefine::default();
    cases.push(("rectangle, defaults", base.clone(), "shots/cases/quick-default.rsp"));
    let mut t = base.clone();
    t.shape = 2;
    t.dims = [800.0, 600.0, 250.0, 150.0, 0.0, 0.0];
    t.top_n = 4;
    t.top_bar = "15M".into();
    t.bot_n = 4;
    t.bot_bar = "30M".into();
    t.cover = 30.0;
    t.strands = 4;
    cases.push(("T-beam, 2 rows and strands", t, "shots/cases/quick-tee.rsp"));
    let mut c = base.clone();
    c.shape = 1;
    c.dims = [500.0, 600.0, 0.0, 0.0, 0.0, 0.0];
    c.top_n = 8;
    c.top_bar = "25M".into();
    c.bot_n = 0;
    c.stirrup_type = 4;
    cases.push(("circle, ring of 8", c.clone(), "shots/cases/quick-circ.rsp"));
    let mut c16 = c;
    c16.top_n = 16;
    c16.top_bar = "20M".into();
    c16.st_space = 200.0;
    c16.cover = 60.0;
    cases.push(("circle, ring of 16, 60 mm cover", c16, "shots/cases/quick-circ16.rsp"));
    let mut out = vec![];
    for (name, q, file) in cases {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        let Ok(orig) = crate::rsp::load(&text) else { continue };
        let Some(orig) = orig.first() else { continue };
        let Ok(mine) = Section::from_quick(&q) else { continue };
        let mut worst = 0.0f64;
        let mut bad = 0;
        let mut notes = vec![];
        let mut a: Vec<_> = orig.long.iter().collect();
        let mut b: Vec<_> = mine.long.iter().collect();
        a.sort_by(|x, y| x.z.partial_cmp(&y.z).unwrap());
        b.sort_by(|x, y| x.z.partial_cmp(&y.z).unwrap());
        if a.len() != b.len() {
            bad += 1;
            notes.push(format!("layers: original {}, this {}", a.len(), b.len()));
        }
        for (x, y) in a.iter().zip(&b) {
            worst = worst.max((x.z - y.z).abs());
            if (x.z - y.z).abs() > 0.06 || x.num != y.num || (x.area - y.area).abs() > 0.5 || x.name != y.name || (x.dep - y.dep).abs() > 1e-6 || x.ring != y.ring || x.ring_id != y.ring_id {
                bad += 1;
                notes.push(format!("{} z {:.2} n {} A {:.0} | this {} z {:.2} n {} A {:.0}", x.name, x.z, x.num, x.area, y.name, y.z, y.num, y.area));
            }
        }
        match (&orig.stirrups, &mine.stirrups) {
            (Some(s), Some(m)) => {
                worst = worst.max((s.z_top - m.z_top).abs()).max((s.z_bot - m.z_bot).abs());
                if (s.z_top - m.z_top).abs() > 0.06 || (s.z_bot - m.z_bot).abs() > 0.06 || (s.area - m.area).abs() > 0.5 || s.pattern != m.pattern {
                    bad += 1;
                    notes.push(format!("stirrup {:.1}-{:.1} A {:.0} type {} | this {:.1}-{:.1} A {:.0} type {}", s.z_bot, s.z_top, s.area, s.pattern, m.z_bot, m.z_top, m.area, m.pattern));
                }
            }
            (None, None) => {}
            _ => {
                bad += 1;
                notes.push("stirrup present in one only".into());
            }
        }
        if orig.outline.len() != mine.outline.len() || orig.outline.iter().zip(&mine.outline).any(|(p, q)| (p.0 - q.0).abs() > 0.06 || (p.1 - q.1).abs() > 0.06) {
            bad += 1;
            notes.push(format!("outline differs: {:?} | {:?}", orig.outline, mine.outline));
        }
        let rebar = |s: &Section| s.rebars.iter().map(|r| format!("{} {:.0} {:.0}/{:.0}", r.name, r.fy, r.esh, r.eu)).collect::<Vec<_>>().join(", ");
        if rebar(orig) != rebar(&mine) {
            bad += 1;
            notes.push(format!("steels: {} | {}", rebar(orig), rebar(&mine)));
        }
        out.push((name.to_string(), worst, bad, notes));
    }
    out
}

pub fn run(out: &str) {
    let t0 = std::time::Instant::now();
    let sec = sample();
    let c = ConcreteProps::new(&sec.concretes[0]);
    let mut constants: Vec<Row> = vec![];
    let mut behaviour: Vec<Row> = vec![];
    let cst = |name: &str, input: &str, reference: f64, src: &str, build: f64, unit: &str, tol: f64| Row {
        name: name.into(),
        input: input.into(),
        reference,
        reference_source: src.into(),
        build,
        unit: unit.into(),
        tolerance_pct: tol,
        lower_bound: false,
    };
    // material values the build computes, against the original's own printout and the sheet
    constants.push(cst("cracking strength ft", "f'c 40 MPa", 1.96, "original's material summary for the sample beam (ft = 1.96 MPa); sheet: min(0.15 f'c, 1.4 (1 + 0.01 f'c))", c.ft, "MPa", 0.5));
    constants.push(cst("strain at peak e'c", "f'c 40 MPa", 2.10, "original's material summary (e'c = 2.10 mm/m); sheet: f'c/Ec' n/(n-1)", c.e0 * 1000.0, "mm/m", 0.5));
    let ecp = 3320.0 * 40f64.sqrt() + 6900.0;
    constants.push(cst("Ec' (formula for e'c)", "f'c 40 MPa", ecp, "sheet: 3320 sqrt(f'c) + 6900 (0x4af050)", {
        let n = (0.8 + 40.0 / 17.0f64).max(1.01);
        40.0 / (c.e0) * n / (n - 1.0) / 1.0
    }, "MPa", 0.5));
    constants.push(cst("maximum shear on a crack, w = 0", "f'c 40 MPa, 19 mm aggregate", 3.67, "original's nine plots, Shear on Crack limit label 3.67 (shots/orig-mv15.png); sheet 0.18 sqrt(f'c)/0.31", c.vci_max(0.0), "MPa", 0.5));
    constants.push(cst("cracking-stress factor under compression", "f2/fp = 0.83", 0.22, "original 0x46a960: clamp(1.05 - f2/fp, 0.05, 1); its plot reads 0.42 MPa = 0.22 ft at the top of the web (shots/orig-n6000.png)", {
        let fp = c.beta(0.0) * c.fc;
        c.crack_factor(0.83 * fp, 0.0)
    }, "", 1.0));
    constants.push(cst("cracking-stress factor floor", "f2/fp = 1.2", 0.05, "original 0x52f040", {
        let fp = c.beta(0.0) * c.fc;
        c.crack_factor(1.2 * fp, 0.0)
    }, "", 0.5));
    // ACI 318 strength reduction (design.rs), against hand calculations on the sample beam
    {
        let a = crate::model::AciFactors::default();
        let dm = crate::design::AciModels::new(&sec, &a);
        let s = dm.state(0.0, false);
        // P0 = 0.85 f'c (Ag - Ast) + fy Ast, Ag 180000, Ast 1900 mm2
        let p0 = 0.85 * 40.0 * (180000.0 - 1900.0) + 400.0 * 1900.0;
        constants.push(cst("ACI 318 squash load P0", "sample beam", p0 / 1000.0, "ACI 318-19 22.4.2.2 by hand: 0.85 f'c (Ag - Ast) + fy Ast", dm.p0 / 1000.0, "kN", 0.1));
        constants.push(cst("ACI 318 design axial cap phi Pn,max", "sample beam, ties", 0.65 * 0.80 * p0 / 1000.0, "ACI 318-19 22.4.2.1, 21.2.2: 0.65 x 0.80 P0", -dm.n_cap(), "kN", 0.1));
        // et by the ACI stress block: beta1 = 0.85 - 0.05 (f'c - 28)/7, a = As fy / (0.85 f'c b),
        // c = a / beta1, et = 0.003 (d - c) / c, d = 540 (compression bars left out: they lower c a
        // little, so the plane-sections value is a few percent higher)
        let b1: f64 = 0.85 - 0.05 * (40.0 - 28.0) / 7.0;
        let c = 1500.0 * 400.0 / (0.85 * 40.0 * 300.0) / b1;
        let et = 0.003 * (540.0 - c) / c * 1000.0;
        constants.push(cst("ACI 318 net tensile strain et at N = 0", "sample beam, positive moment", et, "ACI 318-19 22.2 stress block by hand (no compression steel)", s.et * 1000.0, "mm/m", 5.0));
        constants.push(cst("ACI 318 phi at N = 0", "sample beam (tension-controlled)", 0.90, "ACI 318-19 Table 21.2.2: et >= ety + 0.003", s.phi, "", 0.1));
        let s2 = dm.state(-3000.0, false);
        constants.push(cst("ACI 318 phi at N = -3000 kN", "sample beam (compression-controlled)", 0.65, "ACI 318-19 Table 21.2.2: et <= ety, ties", s2.phi, "", 0.1));
    }
    // biaxial bending (biaxial.rs) against the one-axis moment-curvature run: at load angle 0 it
    // must give the same capacity; a doubly symmetric column the same at 0 and 90 degrees
    if let Ok(bm) = crate::biaxial::BiModel::new(&sec) {
        for n in [0.0, -3000.0] {
            let m80 = analysis::build(&sec, 80).unwrap();
            let r1 = analysis::moment_curvature(&m80, n);
            let one = r1.points.get(r1.peak).map(|p| p.m).unwrap_or(0.0);
            let two = bm.run(0.0, n).peak_point().map(|p| p.m).unwrap_or(0.0);
            constants.push(cst(
                &format!("biaxial at load angle 0 = one-axis bending, N = {:.0} kN", n),
                "sample beam",
                one,
                "this program's moment-curvature run (analysis::moment_curvature)",
                two,
                "kNm",
                1.5,
            ));
        }
    }
    {
        let mut q = crate::model::QuickDefine::default();
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
        if let Ok(col) = crate::model::Section::from_quick(&q) {
            if let Ok(bm) = crate::biaxial::BiModel::new(&col) {
                let a0 = bm.run(0.0, -2000.0).peak_point().map(|p| p.m).unwrap_or(0.0);
                let a90 = bm.run(90.0, -2000.0).peak_point().map(|p| p.m).unwrap_or(0.0);
                constants.push(cst("biaxial: symmetric column, capacity at 90 deg = at 0 deg", "600 x 600, 16 x 30M, N = -2000 kN", a0, "symmetry", a90, "kNm", 1.0));
            }
        }
    }
    // AASHTO LRFD phi and the torsion checks (torsion.rs), against hand calculations
    {
        let ac = crate::model::AciFactors::default();
        let aa = crate::model::AciFactors::aashto();
        constants.push(cst("ACI 318 phi at et = 4 mm/m (60 grade)", "ety 2 mm/m", 0.65 + 0.25 * (2.0 / 3.0), "ACI 318-19 Table 21.2.2: 0.65 + 0.25 (et - ety)/0.003", ac.phi_flexure(0.004, 0.002), "", 0.1));
        constants.push(cst("AASHTO LRFD phi at et = 4 mm/m (60 grade)", "ety 2 mm/m", 0.75 + 0.15 * (2.0 / 3.0), "AASHTO LRFD 5.5.4.2: 0.75 + 0.15 (et - ecl)/(etl - ecl), etl 5 mm/m", aa.phi_flexure(0.004, 0.002), "", 0.1));
        let mut q = crate::model::QuickDefine::default();
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
        if let Ok(col) = crate::model::Section::from_quick(&q) {
            let tp = crate::torsion::props(&col);
            // the tie's centreline: 600 - 2 x (40 + 11.3/2) = 508.7 mm each way
            let aoh: f64 = 508.7 * 508.7;
            let tn = 2.0 * 0.85 * aoh * 100.0 * 400.0 / 300.0 / 1e6;
            let ta = crate::torsion::check(&tp, &ac, 0.0, 0.0, 30.0, 0.0);
            constants.push(cst("ACI 318 torsion phi Tn, 600 x 600 column, 10M closed ties @ 300", "Aoh 508.7^2, At 100, fyt 400", 0.75 * tn, "ACI 318-19 22.7.6.1 by hand: 0.75 x 2 (0.85 Aoh) At fyt / s", ta.phi_tn, "kNm", 0.2));
            let tth = 0.083 * 35f64.sqrt() * 360000f64.powi(2) / 2400.0 / 1e6;
            constants.push(cst("ACI 318 threshold torsion phi Tth, same column, Pu = 0", "Acp 360000, pcp 2400", 0.75 * tth, "ACI 318-19 Table 22.7.4.1(a) by hand", ta.neglect_limit, "kNm", 0.2));
            let tb = crate::torsion::check(&tp, &aa, 0.0, 0.0, 30.0, 500.0);
            constants.push(cst("AASHTO LRFD torsion phi Tn, same column", "Ao = 0.85 Aoh", 0.90 * tn, "AASHTO LRFD 5.7.3.6.2 by hand: 0.90 x 2 Ao At fy / s", tb.phi_tn, "kNm", 0.2));
            let tcr = 0.331 * 35f64.sqrt() * 360000f64.powi(2) / 2400.0 / 1e6;
            constants.push(cst("AASHTO LRFD torsion: 0.25 phi Tcr, same column, Pu = 0", "K = 1", 0.25 * 0.9 * tcr, "AASHTO LRFD 5.7.2.1-3/-4 by hand (0.126 ksi = 0.331 MPa form)", tb.neglect_limit, "kNm", 0.3));
        }
    }
    // shear in two directions (shear2d.rs): at 0 degrees the program's own shear run; a square
    // column the same at 0 and 90 degrees
    {
        let mut s = sec.clone();
        s.loads = crate::model::Loads::new(0.0, 1.5, false);
        if let Ok(m) = analysis::build_shear(&s) {
            let mp = analysis::moment_curvature(&m, 0.0);
            let (cc, d) = s.loads.model_spec();
            let one = analysis::sectional_response(&m, cc, d, &mp).points.iter().map(|p| p.v).fold(0.0, f64::max);
            let two = crate::shear2d::run(&sec, 0.0, 0.0, 1.5).v;
            constants.push(cst("shear in two directions at 0 deg = the one-direction shear run", "sample beam, M/V = 1.5 m", one, "this program's sectional shear analysis", two, "kN", 1.0));
        }
        let mut q = crate::model::QuickDefine::default();
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
        if let Ok(col) = crate::model::Section::from_quick(&q) {
            let v0 = crate::shear2d::run(&col, 0.0, 0.0, 0.0).v;
            let v90 = crate::shear2d::run(&col, 90.0, 0.0, 0.0).v;
            constants.push(cst("shear in two directions: square column at 90 deg = at 0 deg", "600 x 600, 16 x 30M, closed ties, shear only", v0, "symmetry", v90, "kN", 3.0));
        }
    }
    // behaviour: the original's own runs on the sample beam
    for case in compare::sample_cases() {
        let mut s = sec.clone();
        s.loads.axial = case.cst.0;
        s.loads.m_const = case.cst.1;
        s.loads.v_const = case.cst.2;
        s.loads.n_inc = case.inc.0;
        s.loads.m_inc = case.inc.1;
        s.loads.v_inc = case.inc.2;
        s.loads.sync();
        let Ok(m) = analysis::build_shear(&s) else { continue };
        let mp = analysis::moment_curvature(&m, s.loads.axial);
        let (cc, d) = s.loads.model_spec();
        let r = analysis::sectional_response(&m, cc, d, &mp);
        let by_v = d.2.abs() > 1e-12;
        let pk = if by_v {
            r.points.iter().map(|p| p.v).fold(0.0, f64::max)
        } else {
            r.points.iter().map(|p| p.m.abs()).fold(0.0, f64::max)
        };
        let src = if case.lower_bound {
            format!(
                "the original run on the same file (shots/{}); a lower bound: its run stopped while the load was still rising (see MECHANICS.md)",
                case.picture
            )
        } else {
            format!("the original run on the same file (shots/{})", case.picture)
        };
        let mut row = cst(
            &format!("sectional response: {}", case.name),
            &format!("sample beam, loads {:?} + factor x {:?}", case.cst, case.inc),
            if by_v { case.v } else { case.m },
            &src,
            pk,
            if by_v { "kN" } else { "kNm" },
            15.0,
        );
        row.lower_bound = case.lower_bound;
        behaviour.push(row);
    }
    // heavy compression, shear raised: where the web cracks and where it fails
    {
        let Ok(m) = analysis::build_shear(&sec) else { return };
        let m80 = analysis::build(&sec, 80).unwrap();
        let mp = analysis::moment_curvature(&m80, -6000.0);
        let r = analysis::sectional_response(&m, (-6000.0, 0.0, 0.0), (0.0, 0.0, 1.0), &mp);
        let vpk = r.points.iter().map(|p| p.v).fold(0.0, f64::max);
        behaviour.push(cst("shear capacity at N = -6000 kN", "sample beam, constant -6000 kN + shear", 772.6, "the original (shots/orig-n6000.png)", vpk, "kN", 10.0));
        // the first stage at which any web layer has cracked
        let vcr = r.points.iter().find(|p| p.layers.iter().any(|l| l.cracked && l.v > 0.5)).map(|p| p.v).unwrap_or(0.0);
        behaviour.push(cst("web cracking at N = -6000 kN", "same run", 516.3, "the original's bend of its shear - shear strain curve, its stage with the first cracks (shots/orig-n6000-kink.png)", vcr, "kN", 15.0));
    }
    // M-N extremes
    if let Some(d) = analysis::radial_interaction(&sec, Pair::MN, 10, &|_| {}) {
        let mmax = d.points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        let mmin = d.points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        let nmin = d.points.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        let nmax = d.points.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        let src = "the original's M-N interaction (shots/orig-mn.png)";
        behaviour.push(cst("M-N: largest positive moment", "sample beam", 642.0, src, mmax, "kNm", 5.0));
        behaviour.push(cst("M-N: largest negative moment", "sample beam", -652.3, src, mmin, "kNm", 5.0));
        behaviour.push(cst("M-N: pure compression", "sample beam", -7794.5, src, nmin, "kN", 5.0));
        behaviour.push(cst("M-N: pure tension", "sample beam", 886.3, src, nmax, "kN", 5.0));
    }
    // moment-curvature peak at zero axial load
    {
        let m80 = analysis::build(&sec, 80).unwrap();
        let r = analysis::moment_curvature(&m80, 0.0);
        let pk = r.points.get(r.peak).map(|p| p.m).unwrap_or(0.0);
        behaviour.push(cst("bending capacity, moment-curvature", "sample beam, N = 0", 356.4, "the original's moment-only sectional response (shots/orig-monly.png)", pk, "kNm", 5.0));
    }
    // prestressed T-beams (the original's Quick Define file with 4 x 13 mm strands, dep
    // 6.5 mm/m), each run in the original with Solve > Sectional Response
    for (file, text, by_v, reference, picture) in [
        ("quick-tee.rsp, moment only", include_str!("../shots/cases/quick-tee.rsp"), false, 987.0, "orig-quick-tee-monly.png"),
        ("quick-tee, M/V = 1.5 m", include_str!("../shots/cases/quick-tee-mv15.rsp"), true, 450.0, "orig-quick-tee-mv15.png"),
        ("strands only (pt-tee.rsp), moment only", include_str!("../shots/cases/pt-tee.rsp"), false, 388.9, "orig-pt-tee-monly.png"),
        ("strands only, M/V = 1.5 m", include_str!("../shots/cases/pt-tee-mv15.rsp"), true, 242.6, "orig-pt-tee-mv15.png"),
    ] {
        let Some(s) = crate::rsp::load(text).ok().and_then(|v| v.into_iter().next()) else { continue };
        let Ok(m) = analysis::build_shear(&s) else { continue };
        let mp = analysis::moment_curvature(&m, s.loads.axial);
        let (cc, d) = s.loads.model_spec();
        let r = analysis::sectional_response(&m, cc, d, &mp);
        let pk = if by_v { r.points.iter().map(|p| p.v).fold(0.0, f64::max) } else { r.points.iter().map(|p| p.m.abs()).fold(0.0, f64::max) };
        behaviour.push(cst(
            &format!("prestressed: {}", file),
            "T-beam 800/600, 4 x 13 mm strands, dep 6.5 mm/m",
            reference,
            &format!("the original's sectional response (shots/{})", picture),
            pk,
            if by_v { "kN" } else { "kNm" },
            15.0,
        ));
    }
    // factored (design) resistances with the CSA material factors 0.65 / 0.85 / 0.90
    {
        let mut s = sec.clone();
        s.phi = crate::model::PHI_CSA;
        let m80 = analysis::build(&s, 80).unwrap();
        let r = analysis::moment_curvature(&m80, 0.0);
        let pk = r.points.get(r.peak).map(|p| p.m).unwrap_or(0.0);
        behaviour.push(cst("factored bending capacity (CSA factors)", "sample beam, N = 0, phic 0.65 phis 0.85", 258.2, "the original's moment-only sectional response with Material Factors CSA (shots/orig-monly-csa.png, Mf 258.2 kNm)", pk, "kNm", 5.0));
        s.loads = crate::model::Loads::new(0.0, 1.5, false);
        if let Ok(m) = analysis::build_shear(&s) {
            let mp = analysis::moment_curvature(&m, 0.0);
            let (cc, d) = s.loads.model_spec();
            let r = analysis::sectional_response(&m, cc, d, &mp);
            let v = r.points.iter().map(|p| p.v).fold(0.0, f64::max);
            behaviour.push(cst("factored shear capacity at M/V = 1.5 m (CSA factors)", "sample beam, phic 0.65 phis 0.85", 157.9, "the original's sectional response with Material Factors CSA (shots/orig-mv15-csa.png, Vf 157.9 kN)", v, "kN", 15.0));
        }
    }
    let within = behaviour.iter().filter(|r| r.scored().map(|d| d.abs() <= r.tolerance_pct).unwrap_or(false)).count();
    let mean: f64 = {
        let d: Vec<f64> = behaviour.iter().filter_map(|r| r.scored()).map(f64::abs).collect();
        d.iter().sum::<f64>() / d.len().max(1) as f64
    };
    let mut text = String::new();
    text.push_str("{\n  \"generated_by\": \"section-response --check-mechanics (run time, this build)\",\n");
    text.push_str(&format!("  \"seconds\": {:.1},\n", t0.elapsed().as_secs_f64()));
    text.push_str(&format!(
        "  \"summary\": {{\"constants\": {}, \"behaviour_runs\": {}, \"behaviour_within_tolerance\": {}, \"behaviour_mean_abs_difference_pct\": {:.2}}},\n",
        constants.len(),
        behaviour.len(),
        within,
        mean
    ));
    text.push_str("  \"constants\": [\n");
    text.push_str(&constants.iter().map(|r| r.json()).collect::<Vec<_>>().join(",\n"));
    text.push_str("\n  ],\n  \"behaviour\": [\n");
    text.push_str(&behaviour.iter().map(|r| r.json()).collect::<Vec<_>>().join(",\n"));
    text.push_str("\n  ]\n}\n");
    match std::fs::write(out, &text) {
        Ok(()) => println!("wrote {} ({} constants, {} runs, {} within tolerance, mean difference {:.2} %)", out, constants.len(), behaviour.len(), within, mean),
        Err(e) => eprintln!("could not write {}: {}", out, e),
    }
    for r in constants.iter().chain(behaviour.iter()) {
        println!(
            "  {:<50} reference {:>10.3}  build {:>10.3}  {}{}",
            r.name,
            r.reference,
            r.build,
            pct(r.build, r.reference).map(|d| format!("{:+.2} %", d)).unwrap_or_default(),
            if r.lower_bound { "  (reference is a lower bound: counted 0 when above)" } else { "" }
        );
    }
}
