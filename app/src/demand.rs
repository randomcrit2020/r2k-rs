//! Load cases and demand/capacity ratios (this program's addition; the original analyses one
//! loading at a time): each case gives Pu, Mx, My, Vx, Vy and Tu; the program finds the
//! section's capacities for it and reports D/C for each action and the largest.
//!
//! Signs and units: Pu in kN, compression positive (as design tables usually give it; the
//! analysis itself takes tension +), moments in kNm, shears in kN, torque in kNm. Mx bends about
//! the horizontal axis (+ compresses the top), My about the vertical axis (+ compresses the right
//! side), Vy is the vertical shear (with Mx), Vx the sideways one (with My), as the biaxial and
//! two-direction shear pages.
//!
//! For each case:
//! * axial load and bending (P-Mx-My): the biaxial run (biaxial.rs) at N = -Pu along the
//!   moment's direction gives Mn at that axial load; D/C = Mu / (phi Mn), phi from the net
//!   tensile strain at ecu (ACI 318 / AASHTO LRFD) or 1 (nominal, or CSA's factored
//!   materials). Axial compression is also checked against phi Pn,max = phi alpha P0 (codes) or
//!   P0 (nominal); a load beyond what the section can carry gives no balanced state and D/C
//!   "beyond capacity";
//! * shear (Vx-Vy): the two-direction shear run (shear2d.rs) at the shear's direction, at
//!   N = -Pu and M/V = the moment along that direction over the shear; D/C = Vu / (phi_v Vn);
//! * torsion (ACI 318 / AASHTO LRFD only, torsion.rs): Tu / (phi_t Tn), and shear with torsion
//!   (ACI's stress check, AASHTO's Vu,eff / (phi_v Vn));
//! * the governing D/C is the largest.

use crate::biaxial::{self, BiModel};
use crate::model::*;
use crate::{design, shear2d, torsion};

#[derive(Clone, Debug, Default)]
pub struct LoadCase {
    pub name: String,
    /// kN (compression +), kNm, kNm, kN, kN, kNm
    pub pu: f64,
    pub mx: f64,
    pub my: f64,
    pub vx: f64,
    pub vy: f64,
    pub tu: f64,
}

#[derive(Clone, Debug, Default)]
pub struct CaseResult {
    pub case: LoadCase,
    /// capacities: moment along the case's direction (kNm), shear along its direction (kN),
    /// design torsion (kNm)
    pub mn: f64,
    pub phi_m: f64,
    pub vn: f64,
    pub phi_tn: f64,
    /// D/C ratios (NaN when the action is absent or not checked)
    pub dc_pm: f64,
    pub dc_v: f64,
    pub dc_t: f64,
    pub dc_vt: f64,
    pub dc_max: f64,
    pub governs: String,
    pub notes: String,
}

/// Read a pasted table: one case a line, values separated by tabs, commas, semicolons or
/// spaces; an optional first column that isn't a number is the case's name; missing trailing
/// values are 0; header and blank lines (no number) are skipped. Column order: Pu Mx My Vx Vy
/// Tu.
pub fn parse(text: &str) -> Vec<LoadCase> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        let toks: Vec<&str> = line.split(|c: char| c == '\t' || c == ',' || c == ';' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
        if toks.is_empty() {
            continue;
        }
        let num = |t: &str| t.trim().replace('\u{2212}', "-").parse::<f64>().ok();
        let (name, rest): (String, &[&str]) = if num(toks[0]).is_none() { (toks[0].to_string(), &toks[1..]) } else { (format!("case {}", out.len() + 1), &toks[..]) };
        let vals: Vec<f64> = rest.iter().filter_map(|t| num(t)).collect();
        if vals.is_empty() {
            continue; // a header
        }
        let g = |k: usize| vals.get(k).copied().unwrap_or(0.0);
        let _ = i;
        out.push(LoadCase { name, pu: g(0), mx: g(1), my: g(2), vx: g(3), vy: g(4), tu: g(5) });
    }
    out
}

/// The cases' results as a tab-separated table (for pasting into a spreadsheet).
pub fn to_tsv(rows: &[CaseResult]) -> String {
    let f = |v: f64| if v.is_nan() { String::new() } else if v.is_infinite() { "beyond".into() } else { format!("{:.3}", v) };
    let mut s = String::from("Case\tPu (kN)\tMx (kNm)\tMy (kNm)\tVx (kN)\tVy (kN)\tTu (kNm)\tphi Mn (kNm)\tphi Vn (kN)\tphi Tn (kNm)\tD/C P-M\tD/C V\tD/C T\tD/C V+T\tD/C max\tgoverns\tnotes\n");
    for r in rows {
        let c = &r.case;
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.name,
            c.pu,
            c.mx,
            c.my,
            c.vx,
            c.vy,
            c.tu,
            r.phi_m * r.mn,
            r.vn,
            r.phi_tn,
            f(r.dc_pm),
            f(r.dc_v),
            f(r.dc_t),
            f(r.dc_vt),
            f(r.dc_max),
            r.governs,
            r.notes
        ));
    }
    s
}

/// The shear run a case needs: its direction (degrees, the run's frame), axial load (kN, tension
/// +) and M/V (m); None when the case needs no shear capacity.
fn shear_job(c: &LoadCase, aci: Option<&AciFactors>) -> Option<(f64, f64, f64)> {
    let vu = c.vx.hypot(c.vy);
    if !(vu > 1e-9 || (c.tu.abs() > 1e-9 && aci.map(|a| a.code == 1).unwrap_or(false))) {
        return None;
    }
    let th = if vu > 1e-9 { c.vx.atan2(c.vy).to_degrees() } else { 0.0 };
    let m_th = c.mx * th.to_radians().cos() + c.my * th.to_radians().sin();
    // the moment with the shear compresses the side the run takes as its top: a moment the
    // other way is the run 180 degrees round
    let (th_run, mv) = if m_th >= 0.0 { (th, if vu > 0.0 { m_th / vu } else { 0.0 }) } else { (th + 180.0, if vu > 0.0 { -m_th / vu } else { 0.0 }) };
    Some((th_run.rem_euclid(360.0), -c.pu, mv))
}

/// One job of the pool: a case's bending run or one distinct shear run.
enum Job {
    Bending(usize),
    Shear(f64, f64, f64),
}

enum JobOut {
    Bending(Option<(f64, f64)>),
    Shear(f64),
}

/// Evaluate the cases with one worker per processor core (pool.rs). Each case is split into its
/// bending run and its shear run; shear runs (the slow ones, a full sectional analysis each) are
/// handed out first and identical ones (same direction, axial load and M/V) run once.
pub fn evaluate(sec: &Section, cases: &[LoadCase], progress: &(dyn Fn(f32) + Sync)) -> Result<Vec<CaseResult>, String> {
    let bm = BiModel::new(sec)?;
    let aci = sec.aci.as_ref().map(|a| a.for_section(sec));
    let dm = sec.aci.as_ref().map(|a| design::AciModels::new(sec, a));
    // nominal squash load (compression +, kN) for the axial check without a code
    let p0 = design::AciModels::new(sec, &AciFactors::default()).p0 / 1000.0;
    let tp = torsion::props(sec);
    // the jobs: distinct shear runs first, then the bending runs
    let mut jobs: Vec<Job> = vec![];
    let mut shear_of: Vec<Option<usize>> = vec![None; cases.len()];
    let key = |j: &(f64, f64, f64)| ((j.0 * 1000.0).round() as i64, (j.1 * 1000.0).round() as i64, (j.2 * 1e6).round() as i64);
    let mut seen: std::collections::HashMap<(i64, i64, i64), usize> = std::collections::HashMap::new();
    for (i, c) in cases.iter().enumerate() {
        if let Some(j) = shear_job(c, aci.as_ref()) {
            let k = key(&j);
            let idx = *seen.entry(k).or_insert_with(|| {
                jobs.push(Job::Shear(j.0, j.1, j.2));
                jobs.len() - 1
            });
            shear_of[i] = Some(idx);
        }
    }
    let mut bend_of: Vec<Option<usize>> = vec![None; cases.len()];
    for (i, c) in cases.iter().enumerate() {
        if c.mx.hypot(c.my) > 1e-9 {
            jobs.push(Job::Bending(i));
            bend_of[i] = Some(jobs.len() - 1);
        }
    }
    let outs = crate::pool::map(
        &jobs,
        None,
        &|j| match *j {
            Job::Shear(th, n, mv) => {
                let t0 = std::time::Instant::now();
                let v = shear2d::run(sec, th, n, mv).v;
                if std::env::var("SR_TIME").is_ok() {
                    eprintln!("shear job theta {:.1} N {:.0} M/V {:.3}: {:.1} kN in {:.1} s", th, n, mv, v, t0.elapsed().as_secs_f64());
                }
                JobOut::Shear(v)
            }
            Job::Bending(i) => {
                let c = &cases[i];
                let run = bm.run(c.my.atan2(c.mx).to_degrees(), -c.pu);
                JobOut::Bending(match run.peak_point() {
                    Some(p) if p.m > 0.0 && run.points.len() > 2 => {
                        let phi = aci.as_ref().and_then(|a| biaxial::aci_phi(&run, a).map(|x| x.0)).unwrap_or(1.0);
                        Some((p.m, phi))
                    }
                    _ => None,
                })
            }
        },
        progress,
    );
    let rows = cases
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let bending = bend_of[i].map(|k| match &outs[k] {
                JobOut::Bending(b) => *b,
                _ => None,
            });
            let shear = shear_of[i].map(|k| match &outs[k] {
                JobOut::Shear(v) => *v,
                _ => 0.0,
            });
            one(aci.as_ref(), dm.as_ref(), p0, &tp, c, bending, shear)
        })
        .collect();
    Ok(rows)
}

/// A case's D/C ratios from its runs: `bending` Some(None) when the bending run found no
/// balanced state, None when the case has no moment; `shear` the nominal shear capacity along
/// the case's shear direction, when it needed one.
fn one(aci: Option<&AciFactors>, dm: Option<&design::AciModels>, p0: f64, tp: &torsion::TorsionProps, c: &LoadCase, bending: Option<Option<(f64, f64)>>, shear: Option<f64>) -> CaseResult {
    let mut r = CaseResult { case: c.clone(), dc_pm: f64::NAN, dc_v: f64::NAN, dc_t: f64::NAN, dc_vt: f64::NAN, ..Default::default() };
    let mut notes: Vec<String> = vec![];
    let mut t_neglect = false;
    // axial load and bending
    let mu = c.mx.hypot(c.my);
    let dc_p = if c.pu > 0.0 {
        let cap = match dm {
            Some(d) => -d.n_cap(),
            None => p0,
        };
        c.pu / cap.max(1e-9)
    } else {
        0.0
    };
    let mut dc_pm = dc_p;
    match bending {
        Some(Some((mn, phi))) => {
            r.mn = mn;
            r.phi_m = phi;
            dc_pm = dc_pm.max(mu / (phi * mn));
        }
        Some(None) => {
            dc_pm = f64::INFINITY;
            notes.push("no balanced state at this axial load (beyond capacity)".into());
        }
        None => {}
    }
    if c.pu > 0.0 || mu > 1e-9 {
        r.dc_pm = dc_pm;
        if dc_p > 1.0 {
            notes.push(format!("axial load above {} ({:.2})", if dm.is_some() { "phi Pn,max" } else { "P0" }, dc_p));
        }
    }
    // shear in its direction, with the moment along it
    let vu = c.vx.hypot(c.vy);
    let phi_v = aci.map(|a| a.phi_shear).unwrap_or(1.0);
    let mut vn_dir = 0.0;
    if let Some(v) = shear {
        vn_dir = v;
        r.vn = phi_v * v;
        if vu > 1e-9 {
            r.dc_v = if v > 0.0 { vu / (phi_v * v) } else { f64::INFINITY };
        }
    }    // torsion (codes only)
    if c.tu.abs() > 1e-9 {
        match aci {
            Some(a) => {
                let t = torsion::check(tp, a, c.pu, vu, c.tu, vn_dir);
                r.phi_tn = t.phi_tn;
                r.dc_t = t.dc_t;
                r.dc_vt = t.dc_vt;
                notes.push(t.note.clone());
                t_neglect = t.neglect;
                if t.al_req > 0.0 && !t.neglect {
                    notes.push(format!("torsion needs Al = {:.0} mm² of longitudinal steel besides bending (section has {:.0} mm²)", t.al_req, t.al_have));
                }
            }
            None => notes.push("torsion: choose ACI 318 or AASHTO LRFD in Options > Design Factors".into()),
        }
    }
    let mut best = (f64::NEG_INFINITY, String::new());
    for (v, name) in [(r.dc_pm, "P-M"), (r.dc_v, "V"), (r.dc_t, "T"), (r.dc_vt, "V+T")] {
        // torsion below the code's threshold may be neglected: it is reported, not governing
        if t_neglect && (name == "T" || name == "V+T") {
            continue;
        }
        if !v.is_nan() && v > best.0 {
            best = (v, name.to_string());
        }
    }
    r.dc_max = if best.0.is_finite() || best.0 == f64::INFINITY { best.0 } else { f64::NAN };
    r.governs = best.1;
    r.notes = notes.join("; ");
    r
}
