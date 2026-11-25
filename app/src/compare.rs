//! Comparison of this program's M-V strength envelope with results from the original program.
//!
//! The reference points are either read from a text file (one "moment, shear" pair per line, in
//! kNm and kN, e.g. typed off the original's M-V plot or copied from its output), or the built-in
//! readings for the 300 x 600 sample beam, taken off the original's own M-V Interaction plot
//! (see MECHANICS.md).

use crate::model::Section;

#[derive(Clone, Debug)]
pub struct Reference {
    pub name: String,
    /// (moment kNm, shear kN), in order along the curve from negative to positive moment
    pub points: Vec<(f64, f64)>,
    /// points the original computed (false: on a straight chord the original drew to its
    /// flexural capacity, with no computed points in between)
    pub computed: Vec<bool>,
}

/// The original's envelope for the sample beam (300 x 600, f'c 40, 3-25M bottom, 2-15M top,
/// 10M stirrups @ 250), read off its M-V Interaction plot.
pub fn sample_original() -> Reference {
    let pts = vec![
        (-117.0, 0.0, false),
        (-80.0, 188.0, true),
        (0.0, 290.0, true),
        (68.0, 370.0, true),
        (105.0, 390.0, true),
        (157.0, 345.0, true),
        (205.0, 320.0, true),
        (338.0, 0.0, false),
    ];
    Reference {
        name: "Original program, sample beam (read off its M-V plot)".into(),
        points: pts.iter().map(|p| (p.0, p.1)).collect(),
        computed: pts.iter().map(|p| p.2).collect(),
    }
}

/// One sectional analysis the original was run for on the sample beam (Solve > Sectional
/// Response, loads typed into its file; the result read off its status bar "Current Loads" at
/// the peak, which its control plots mark).
#[derive(Clone, Debug)]
pub struct Case {
    pub name: &'static str,
    /// constant N, M, V and increments dN, dM, dV (kN, kNm), as in the original's Define Loading
    pub cst: (f64, f64, f64),
    pub inc: (f64, f64, f64),
    /// the original's moment and shear at the peak (kNm, kN)
    pub m: f64,
    pub v: f64,
    pub picture: &'static str,
    /// the original's run stopped while the load was still rising (its last state is its
    /// largest): its value is a lower bound on its capacity, not a failure point. Only a
    /// shortfall below it counts as a difference (see `scored_pct`).
    pub lower_bound: bool,
}

impl Case {
    /// Difference of this program's result from the original's, percent (signed), and the part
    /// of it that counts: all of it, or for a lower bound only a shortfall below it.
    pub fn scored_pct(&self, m: f64, v: f64) -> (f64, f64) {
        let d = if self.inc.2.abs() > 1e-12 { 100.0 * (v - self.v) / self.v } else { 100.0 * (m - self.m) / self.m };
        (d, if self.lower_bound { d.min(0.0) } else { d })
    }
}

/// The original's own sectional results for the sample beam (run on 2026-11-21 with r2k.exe;
/// pictures in shots/orig-*.png).
pub fn sample_cases() -> Vec<Case> {
    vec![
        Case { name: "Shear only (M/V = 0)", cst: (0.0, 0.0, 0.0), inc: (0.0, 0.0, 1.0), m: 0.0, v: 319.3, picture: "orig-mv0.png", lower_bound: false },
        Case { name: "M/V = 0.6 m", cst: (0.0, 0.0, 0.0), inc: (0.0, 0.6, 1.0), m: 181.0, v: 301.8, picture: "orig-mv06.png", lower_bound: false },
        Case { name: "M/V = 1.5 m (the file's loading)", cst: (0.0, 0.0, 0.0), inc: (0.0, 1.5, 1.0), m: 304.3, v: 203.1, picture: "orig-mv15.png", lower_bound: false },
        Case { name: "M/V = 4 m", cst: (0.0, 0.0, 0.0), inc: (0.0, 4.0, 1.0), m: 349.3, v: 87.4, picture: "orig-mv4.png", lower_bound: false },
        Case { name: "Constant moment 100 kNm, shear raised", cst: (0.0, 100.0, 0.0), inc: (0.0, 0.0, 1.0), m: 100.0, v: 387.9, picture: "orig-m100.png", lower_bound: false },
        Case { name: "Constant shear 100 kN, moment raised", cst: (0.0, 0.0, 100.0), inc: (0.0, 1.0, 0.0), m: 328.1, v: 100.0, picture: "orig-v100.png", lower_bound: true },
        Case { name: "Moment only (the original's default)", cst: (0.0, 0.0, 0.0), inc: (0.0, 1.0, 0.0), m: 356.4, v: 0.0, picture: "orig-monly.png", lower_bound: false },
    ]
}

/// The original's sectional results as M-V points (ordered by moment), for the plot overlay.
pub fn sample_original_runs() -> Reference {
    let mut pts: Vec<(f64, f64)> = sample_cases().iter().filter(|c| !c.lower_bound).map(|c| (c.m, c.v)).collect();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    Reference {
        name: "Original program, its own sectional runs on the sample beam (exact values)".into(),
        computed: vec![true; pts.len()],
        points: pts,
    }
}

/// Whether a section looks like the sample beam the built-in readings belong to.
pub fn is_sample_beam(sec: &Section) -> bool {
    let h = sec.height();
    let fc = sec.concretes.first().map(|c| c.fc).unwrap_or(0.0);
    (h - 600.0).abs() < 1.0 && (fc - 40.0).abs() < 0.5 && sec.title.to_lowercase().contains("beam")
}

/// Read "moment, shear" pairs (kNm, kN) from text: one pair per line, separated by commas,
/// semicolons, tabs or spaces; lines that don't start with two numbers (headings) are skipped.
pub fn parse(name: &str, text: &str) -> Option<Reference> {
    let mut points = vec![];
    for line in text.lines() {
        let nums: Vec<f64> = line
            .split(|c: char| c == ',' || c == ';' || c == '\t' || c == ' ')
            .filter(|s| !s.trim().is_empty())
            .map_while(|s| s.trim().parse::<f64>().ok())
            .collect();
        if nums.len() >= 2 {
            points.push((nums[0], nums[1]));
        }
    }
    if points.len() < 2 {
        return None;
    }
    let computed = vec![true; points.len()];
    Some(Reference { name: name.into(), points, computed })
}

/// The largest shear on a curve at moment m (linear between its points), if the curve reaches m.
pub fn v_at(curve: &[(f64, f64)], m: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        if m < lo.0 - 1e-9 || m > hi.0 + 1e-9 {
            continue;
        }
        let v = if (hi.0 - lo.0).abs() < 1e-9 { lo.1.max(hi.1) } else { lo.1 + (hi.1 - lo.1) * (m - lo.0) / (hi.0 - lo.0) };
        best = Some(best.map_or(v, |x: f64| x.max(v)));
    }
    best
}

#[derive(Clone, Debug)]
pub struct Row {
    pub what: String,
    pub original: f64,
    pub ours: Option<f64>,
    /// (ours - original) / original, in percent
    pub diff: Option<f64>,
    pub computed: bool,
}

/// Point-by-point comparison: the shear at each reference point's moment, and the two flexural
/// capacities (moments at zero shear).
pub fn compare(r: &Reference, ours: &[(f64, f64)]) -> Vec<Row> {
    let mut rows = vec![];
    let pct = |o: f64, x: f64| if o.abs() > 1e-9 { Some(100.0 * (x - o) / o) } else { None };
    for (k, p) in r.points.iter().enumerate() {
        if p.1 <= 0.0 {
            continue;
        }
        let v = v_at(ours, p.0);
        rows.push(Row {
            what: format!("V at M = {:.0} kNm", p.0),
            original: p.1,
            ours: v,
            diff: v.and_then(|v| pct(p.1, v)),
            computed: r.computed.get(k).copied().unwrap_or(true),
        });
    }
    let ends = |pts: &[(f64, f64)]| -> (Option<f64>, Option<f64>) {
        let zero: Vec<f64> = pts.iter().filter(|p| p.1.abs() < 1e-6).map(|p| p.0).collect();
        (zero.iter().cloned().reduce(f64::min), zero.iter().cloned().reduce(f64::max))
    };
    let (ro_neg, ro_pos) = ends(&r.points);
    let (us_neg, us_pos) = ends(ours);
    for (label, o, u) in [("Bending capacity, negative (kNm)", ro_neg, us_neg), ("Bending capacity, positive (kNm)", ro_pos, us_pos)] {
        let neg = label.contains("negative");
        if let Some(o) = o.filter(|o| if neg { *o < 0.0 } else { *o > 0.0 }) {
            rows.push(Row { what: label.into(), original: o, ours: u, diff: u.and_then(|u| pct(o, u)), computed: true });
        }
    }
    rows
}

/// Mean absolute difference (percent) over the rows the original actually computed.
pub fn mean_abs_diff(rows: &[Row]) -> Option<f64> {
    let d: Vec<f64> = rows.iter().filter(|r| r.computed).filter_map(|r| r.diff.map(f64::abs)).collect();
    (!d.is_empty()).then(|| d.iter().sum::<f64>() / d.len() as f64)
}
