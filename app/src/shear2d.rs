//! Shear in any direction, Vx and Vy together (this program's addition; the original takes
//! shear in one direction only, down the page with the moment about the horizontal axis).
//!
//! The shear at angle theta (from the vertical towards +x: Vy = V cos theta, the usual vertical
//! shear that goes with Mx; Vx = V sin theta, the sideways shear that goes with My) is analysed by
//! turning the section so that direction points down the page and running the program's own
//! sectional shear analysis (MCFT) on it, with the moment at the same angle (M/V given):
//! * the outline is turned about the gross centroid and read back as a width at each height (the
//!   total length of the cut, 160 levels), so any shape works;
//! * the bars (at their drawing positions, section_page::bar_xs) are turned with it and
//!   regrouped into layers by their new height (same steel and prestrain);
//! * the stirrups: a rectangular tie's legs are counted by their projection on the shear
//!   direction, Av(theta) = a1 (n_vertical |cos theta| + n_horizontal |sin theta|) (a closed tie
//!   has 2 and 2 legs, an open one 2 and 1, a single leg 1 and 0); a hoop is the same in every
//!   direction; the tie's height range is its turned outline's;
//! * theta = 0 is the section itself, unchanged.
//! The capacity at each direction is the peak shear of that run; the Vx-Vy diagram is the peaks
//! at evenly spaced directions.

use crate::analysis::{self, Mechanism};
use crate::model::*;

#[derive(Clone, Debug)]
pub struct VRun {
    /// direction (degrees from the vertical towards +x)
    pub theta: f64,
    /// peak shear (kN) and the moment with it (kNm)
    pub v: f64,
    pub m: f64,
    pub mech: Option<Mechanism>,
    pub reason: String,
    /// effective tie area in this direction (mm2, all legs)
    pub av: f64,
}

impl VRun {
    pub fn vx(&self) -> f64 {
        self.v * self.theta.to_radians().sin()
    }
    pub fn vy(&self) -> f64 {
        self.v * self.theta.to_radians().cos()
    }
}

#[derive(Clone, Debug)]
pub struct VDiagram {
    /// axial load (kN), M/V (m)
    pub n: f64,
    pub mv: f64,
    pub runs: Vec<VRun>,
}

impl VDiagram {
    /// Capacity (kN) along direction theta (degrees), interpolated between the runs (polar).
    pub fn capacity_at(&self, theta: f64) -> f64 {
        let t = theta.rem_euclid(360.0);
        let n = self.runs.len();
        if n == 0 {
            return 0.0;
        }
        for i in 0..n {
            let a = &self.runs[i];
            let b = &self.runs[(i + 1) % n];
            let (ta, mut tb) = (a.theta, b.theta);
            if tb <= ta {
                tb += 360.0;
            }
            let tt = if t < ta { t + 360.0 } else { t };
            if tt >= ta && tt <= tb {
                let f = (tt - ta) / (tb - ta).max(1e-9);
                return a.v + f * (b.v - a.v);
            }
        }
        self.runs[0].v
    }
    /// (Vx, Vy) of the capacity points, closed.
    pub fn contour(&self) -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = self.runs.iter().map(|r| (r.vx(), r.vy())).collect();
        if let Some(f) = v.first().copied() {
            v.push(f);
        }
        v
    }
}

/// The section turned so that the direction theta (degrees from the vertical towards +x)
/// points up the page (the compressed side of the moment that goes with the shear), and the
/// effective tie area in that direction.
pub fn rotated_section(sec: &Section, theta_deg: f64) -> (Section, f64, Vec<TieLine>) {
    let av0 = sec.stirrups.as_ref().map(|s| s.area).unwrap_or(0.0);
    if theta_deg.rem_euclid(360.0).abs() < 1e-9 && std::env::var("SR_ROT0").is_err() {
        return (sec.clone(), av0, vec![]);
    }
    let th = theta_deg.to_radians();
    let (c, s) = (th.cos(), th.sin());
    let h = sec.height();
    // gross centroid
    let (mut a, mut az) = (0.0, 0.0);
    for i in 0..400 {
        let z = (i as f64 + 0.5) * h / 400.0;
        let w = sec.width_at(z);
        a += w;
        az += w * z;
    }
    let zc = if a > 0.0 { az / a } else { h / 2.0 };
    let rot = |x: f64, z: f64| (x * c - (z - zc) * s, x * s + (z - zc) * c);
    // the outline as a polygon (right side up, left side down), turned
    let nz = 200;
    let mut poly = vec![];
    for i in 0..=nz {
        let z = h * i as f64 / nz as f64;
        let w = sec.width_at(z.clamp(h * 1e-6, h * (1.0 - 1e-6)));
        poly.push(rot(w / 2.0, z));
    }
    for i in (0..=nz).rev() {
        let z = h * i as f64 / nz as f64;
        let w = sec.width_at(z.clamp(h * 1e-6, h * (1.0 - 1e-6)));
        poly.push(rot(-w / 2.0, z));
    }
    let zmin = poly.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let zmax = poly.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let h2 = zmax - zmin;
    // width at each new height: the total length of the polygon's cut
    let levels = 160;
    let mut outline = vec![];
    for k in 0..=levels {
        let zl = zmin + h2 * (k as f64 / levels as f64).clamp(1e-6, 1.0 - 1e-6);
        let mut xs = vec![];
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            if (p.1 - zl) * (q.1 - zl) < 0.0 || (p.1 == zl && q.1 != zl) {
                let t = (zl - p.1) / (q.1 - p.1);
                xs.push(p.0 + t * (q.0 - p.0));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let w: f64 = xs.chunks(2).filter(|c| c.len() == 2).map(|c| c[1] - c[0]).sum();
        outline.push((h2 * k as f64 / levels as f64, w.max(0.0)));
    }
    // bars, regrouped by their new height
    let mut groups: Vec<(f64, String, f64, String, u32, f64)> = vec![]; // z, steel, dep, title, n, area
    for l in &sec.long {
        let xs = crate::section_page::bar_xs(sec, l);
        let k = xs.len().max(1) as f64;
        for x in xs {
            let key = rot(x, l.z).1 - zmin;
            if let Some(g) = groups.iter_mut().find(|g| (g.0 - key).abs() < 0.3 && g.1 == l.steel && (g.2 - l.dep).abs() < 1e-9) {
                g.4 += 1;
                g.5 += l.area / k;
            } else {
                groups.push((key, l.steel.clone(), l.dep, l.bartitle.clone(), 1, l.area / k));
            }
        }
    }
    groups.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let long: Vec<LongLayer> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| LongLayer {
            name: format!("r{}", i + 1),
            z: g.0,
            num: g.4,
            area: g.5,
            steel: g.1.clone(),
            bartitle: g.3.clone(),
            dep: g.2,
            ring: 0,
            ring_id: 0,
            from_top: false,
        })
        .collect();
    // ties: a hoop is the same in every direction; a rectangular tie's sides are turned with the
    // section and each layer later counts the sides it crosses (tie_rho)
    let mut av = 0.0;
    let mut lines = vec![];
    let stirrups = sec.stirrups.as_ref().and_then(|st| {
        let zt = st.z_top.max(st.z_bot);
        let zb = st.z_top.min(st.z_bot);
        let db = st.db.max(1.0);
        match st.pattern {
            4 | 6 => {
                // one hoop: a circle on the middle; interlocking: two circles, one against each
                // cover (radius as Quick Define fits them), turned with the section
                let cover = (zb - db / 2.0).max(0.0);
                let r = if st.pattern == 4 {
                    0.5 * (zt - zb)
                } else {
                    (0.5 * (zt - zb)).min(sec.width_at(0.5 * (zt + zb)) / 2.0 - cover - db / 2.0)
                };
                let centres = if st.pattern == 4 { vec![0.5 * (zt + zb)] } else { vec![zt - r, zb + r] };
                let zs: Vec<f64> = centres.iter().map(|z| rot(0.0, *z).1 - zmin).collect();
                let lo = zs.iter().copied().fold(f64::INFINITY, f64::min) - r;
                let hi = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max) + r;
                av = st.area;
                Some(Stirrups { name: "ties".into(), area: st.area, steel: st.steel.clone(), spacing: st.spacing, z_top: hi, z_bot: lo, db: st.db, bartitle: st.bartitle.clone(), pattern: 3 })
            }
            p => {
                let edges = tie_edges(sec, st);
                let a1 = st.area / stirrup_legs(p).max(1.0);
                for (a, b) in &edges {
                    let (pa, pb) = (rot(a.0, a.1), rot(b.0, b.1));
                    lines.push(((pa.0, pa.1 - zmin), (pb.0, pb.1 - zmin), a1));
                }
                let lo = lines.iter().map(|l| l.0 .1.min(l.1 .1)).fold(f64::INFINITY, f64::min);
                let hi = lines.iter().map(|l| l.0 .1.max(l.1 .1)).fold(f64::NEG_INFINITY, f64::max);
                // reported: the average over the middle third of the tie's turned height
                av = (0..21).map(|i| tie_area_at(&lines, lo + (hi - lo) * (1.0 / 3.0 + i as f64 / 60.0))).sum::<f64>() / 21.0;
                if lines.is_empty() || hi <= lo {
                    return None;
                }
                Some(Stirrups { name: "ties".into(), area: st.area, steel: st.steel.clone(), spacing: st.spacing, z_top: hi, z_bot: lo, db: st.db, bartitle: st.bartitle.clone(), pattern: 3 })
            }
        }
    });    let mut out = sec.clone();
    out.shape = Shape::Custom;
    out.outline = outline;
    out.long = long;
    out.stirrups = stirrups;
    (out, av, lines)
}

/// One direction: the section turned to theta, loaded with axial load n (kN) and shear with
/// moment M = mv V (mv in m), to its peak.
pub fn run(sec: &Section, theta_deg: f64, n: f64, mv: f64) -> VRun {
    let (mut s, av, lines) = rotated_section(sec, theta_deg);
    s.loads = Loads::new(n, mv, false);
    let fail = |reason: String| VRun { theta: theta_deg, v: 0.0, m: 0.0, mech: None, reason, av };
    let Ok(mut m) = analysis::build_shear(&s) else { return fail("The turned section could not be built.".into()) };
    // a turned rectangular tie: each layer's tie ratio from the sides it crosses
    if !lines.is_empty() {
        let sp = s.stirrups.as_ref().map(|st| st.spacing).unwrap_or(1.0).max(1.0);
        for ly in m.layers.iter_mut() {
            ly.rho_z = if ly.b > 0.0 { tie_area_at(&lines, ly.z) / (ly.b * sp) } else { 0.0 };
        }
    }
    let mp = analysis::moment_curvature(&m, n);
    let (cc, d) = s.loads.model_spec();
    let r = analysis::sectional_response(&m, cc, d, &mp);
    match r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap()) {
        Some(p) => VRun { theta: theta_deg, v: p.v, m: p.m, mech: r.mech, reason: r.reason.clone(), av },
        None => fail(r.reason.clone()),
    }
}

/// The Vx-Vy diagram: runs at `dirs` evenly spaced directions, in parallel.
pub fn diagram(sec: &Section, n: f64, mv: f64, dirs: usize, progress: &(dyn Fn(f32) + Sync)) -> VDiagram {
    // an even number of directions; the program's sections are symmetric about the vertical
    // axis (outline and bar rows), so theta and 360 - theta are mirror images: only 0 to 180
    // are run and the other half is mirrored (the shear solver can stop at slightly different
    // loads on a mirror image, which would show as a lopsided diagram)
    let dirs = (dirs.clamp(4, 72) / 2) * 2;
    let half: Vec<f64> = (0..=dirs / 2).map(|i| 360.0 * i as f64 / dirs as f64).collect();
    // one worker per core (pool.rs)
    let first: Vec<VRun> = crate::pool::map(&half, None, &|&th| run(sec, th, n, mv), progress);
    let mut runs = first.clone();
    for r in first.iter().rev() {
        if r.theta > 1e-9 && r.theta < 180.0 - 1e-9 {
            let mut m = r.clone();
            m.theta = 360.0 - r.theta;
            runs.push(m);
        }
    }
    VDiagram { n, mv, runs }
}

/// A demand (Vux, Vuy) against the diagram: (its direction in degrees, the capacity there, the
/// use ratio |Vu| / Vn).
pub fn check(d: &VDiagram, vux: f64, vuy: f64) -> (f64, f64, f64) {
    let th = vux.atan2(vuy).to_degrees().rem_euclid(360.0);
    let cap = d.capacity_at(th);
    let vu = vux.hypot(vuy);
    (th, cap, if cap > 0.0 { vu / cap } else { f64::INFINITY })
}

/// ACI 318-19's rule for shear in two directions, on the design strengths phi Vn of the two
/// principal directions (here the runs at 0/180 and 90/270 degrees, by the demand's signs):
/// either ratio alone when the other is at most 0.5, otherwise their sum at most 1.5. Returns
/// (ratio x, ratio y, governing value, limit, passes).
pub fn aci_two_way(d: &VDiagram, phi_v: f64, vux: f64, vuy: f64) -> (f64, f64, f64, f64, bool) {
    let cap = |th: f64| phi_v * d.capacity_at(th);
    let vnx = cap(if vux >= 0.0 { 90.0 } else { 270.0 });
    let vny = cap(if vuy >= 0.0 { 0.0 } else { 180.0 });
    let rx = if vnx > 0.0 { vux.abs() / vnx } else { f64::INFINITY };
    let ry = if vny > 0.0 { vuy.abs() / vny } else { f64::INFINITY };
    if rx <= 0.5 || ry <= 0.5 {
        let g = rx.max(ry);
        (rx, ry, g, 1.0, g <= 1.0)
    } else {
        (rx, ry, rx + ry, 1.5, rx + ry <= 1.5 && rx <= 1.0 && ry <= 1.0)
    }
}

/// A tie side in the turned section: its two ends (x, z) and the area of one leg (mm2).
pub type TieLine = ((f64, f64), (f64, f64), f64);

/// The sides of a rectangular, open or single-leg tie on its centreline, as the drawing lays them
/// (section_page::draw_steel): a closed tie's four sides, an open tie's two legs and bottom, a
/// single leg just left of the middle bar.
fn tie_edges(sec: &Section, st: &Stirrups) -> Vec<((f64, f64), (f64, f64))> {
    let zt = st.z_top.max(st.z_bot);
    let zb = st.z_top.min(st.z_bot);
    let db = st.db.max(1.0);
    match st.pattern {
        1 | 5 => {
            let off = sec.long.iter().filter(|l| l.num % 2 == 1).map(|l| l.draw_dia() / 2.0 + db / 2.0).fold(0.0, f64::max);
            vec![((-off, zb), (-off, zt))]
        }
        p => {
            let mid = 0.5 * (zt + zb);
            let wmin = [zb + 1.0, mid, zt - 1.0].iter().map(|z| sec.width_at(*z)).fold(f64::INFINITY, f64::min);
            let mut c = (zb - db / 2.0).max(0.0);
            if wmin < 2.0 * c + 2.0 * db {
                c = (wmin - 2.0 * db) / 3.0;
            }
            let xs = wmin / 2.0 - c - db / 2.0;
            let mut e = vec![((-xs, zb), (-xs, zt)), ((xs, zb), (xs, zt)), ((-xs, zb), (xs, zb))];
            if p != 2 {
                e.push(((-xs, zt), (xs, zt)));
            }
            e
        }
    }
}

/// Tie area crossing the height z, counted along the shear (vertical in the turned section):
/// each side it crosses adds one leg's area times the vertical part of its direction.
pub fn tie_area_at(lines: &[TieLine], z: f64) -> f64 {
    let mut a = 0.0;
    for &((x1, z1), (x2, z2), a1) in lines {
        let (lo, hi) = (z1.min(z2), z1.max(z2));
        if z > lo && z < hi {
            let len = (x2 - x1).hypot(z2 - z1);
            if len > 0.0 {
                a += a1 * (z2 - z1).abs() / len;
            }
        }
    }
    a
}