//! Biaxial bending (this program's addition; the original analyses bending about one axis):
//! plane sections in two directions with the same uniaxial materials as the moment-curvature
//! analysis, no shear.
//!
//! * The section is cut into horizontal layers (as `analysis::build`) and each layer into strips
//!   across its width: concrete fibres at (x, z). Bars sit where the drawing puts them
//!   (section_page::bar_xs), each with its layer's steel, prestrain and rupture-at-crack strain.
//! * Strain e(x, z) = e0 - kx (z - zc) - ky (x - xc), tension positive; (xc, zc) the gross
//!   section's centroid. Mx = -sum f A (z - zc) (positive compresses the top, the program's
//!   usual positive moment), My = -sum f A (x - xc) (positive compresses the +x side, the right
//!   in the drawing).
//! * A load angle alpha fixes the moment's direction: Mx = M cos(alpha), My = M sin(alpha). For
//!   each curvature size the neutral axis is tilted (curvature direction beta) until the
//!   resulting moment points along alpha, with e0 found for the axial load; for a section
//!   symmetric about both axes beta = alpha, otherwise the neutral axis turns away from it.
//! * Each run goes on until the most compressed fibre reaches -12 mm/m or the moment falls below
//!   60 % of its peak (the moment-curvature run's stops); its peak is the
//!   capacity in that direction. The Mx-My diagram at an axial load is the peaks of runs at
//!   evenly spaced angles.

use crate::analysis;
use crate::materials::{ConcreteProps, SteelProps};
use crate::model::*;

#[derive(Clone, Debug)]
struct Fibre {
    x: f64,
    z: f64,
    a: f64,
    m_bond: f64,
}

#[derive(Clone, Debug)]
struct BiBar {
    x: f64,
    z: f64,
    a: f64,
    steel: SteelProps,
    pre: f64,
}

#[derive(Clone, Debug)]
pub struct BiModel {
    fibres: Vec<Fibre>,
    bars: Vec<BiBar>,
    conc: ConcreteProps,
    pub zc: f64,
    pub xc: f64,
    pub h: f64,
    pub b: f64,
}

/// One state of a run.
#[derive(Clone, Debug)]
pub struct BiPoint {
    /// curvature size (rad/km)
    pub kappa: f64,
    /// moment along the load angle, and its parts (kNm)
    pub m: f64,
    pub mx: f64,
    pub my: f64,
    /// direction of the curvature vector (degrees; the neutral axis is perpendicular to the
    /// line it points along, as for alpha)
    pub beta: f64,
    pub e0: f64,
    /// most compressed concrete fibre and most stretched bar (plain strain), and that bar's
    /// yield strain
    pub ec_min: f64,
    pub et_max: f64,
    pub ety: f64,
}

#[derive(Clone, Debug)]
pub struct BiRun {
    pub alpha: f64,
    pub n: f64,
    pub points: Vec<BiPoint>,
    pub peak: usize,
    pub stop: String,
}

impl BiRun {
    pub fn peak_point(&self) -> Option<&BiPoint> {
        self.points.get(self.peak)
    }
}

#[derive(Clone, Debug)]
pub struct BiDiagram {
    /// axial load (kN)
    pub n: f64,
    /// capacity runs at evenly spaced angles (0 to 360)
    pub runs: Vec<BiRun>,
    /// the run at the chosen angle (its moment-curvature curve)
    pub chosen: BiRun,
}

impl BiDiagram {
    /// (Mx, My) at each direction's peak, closed.
    pub fn contour(&self) -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = self.runs.iter().filter_map(|r| r.peak_point().map(|p| (p.mx, p.my))).collect();
        if let Some(f) = v.first().copied() {
            v.push(f);
        }
        v
    }
}

impl BiModel {
    pub fn new(sec: &Section) -> Result<BiModel, String> {
        let mut s = sec.clone();
        s.loads.negative = false;
        let nl = 80;
        let m = analysis::build(&s, nl)?;
        let nx = 24;
        let mut fibres = vec![];
        for l in &m.layers {
            if l.b <= 0.0 {
                continue;
            }
            let dx = l.b / nx as f64;
            for i in 0..nx {
                fibres.push(Fibre { x: -l.b / 2.0 + (i as f64 + 0.5) * dx, z: l.z, a: dx * l.t, m_bond: l.m_bond });
            }
        }
        let mut bars = vec![];
        for (b, l) in m.bars.iter().zip(&sec.long) {
            let xs = crate::section_page::bar_xs(sec, l);
            let k = xs.len().max(1) as f64;
            for x in xs {
                bars.push(BiBar { x, z: b.z, a: b.area / k, steel: b.steel.clone(), pre: b.pre });
            }
        }
        let b = m.layers.iter().map(|l| l.b).fold(0.0, f64::max);
        Ok(BiModel { fibres, bars, conc: m.conc.clone(), zc: m.zc, xc: 0.0, h: m.h, b })
    }

    fn strain(&self, e0: f64, kx: f64, ky: f64, x: f64, z: f64) -> f64 {
        e0 - kx * (z - self.zc) - ky * (x - self.xc)
    }

    /// (N in N, Mx and My in N mm)
    fn forces(&self, e0: f64, kx: f64, ky: f64) -> (f64, f64, f64) {
        let (mut n, mut mx, mut my) = (0.0, 0.0, 0.0);
        for f in &self.fibres {
            let e = self.strain(e0, kx, ky, f.x, f.z);
            let s = self.conc.uniaxial(e, f.m_bond) * f.a;
            n += s;
            mx -= s * (f.z - self.zc);
            my -= s * (f.x - self.xc);
        }
        for b in &self.bars {
            let e = self.strain(e0, kx, ky, b.x, b.z);
            // the bar's own stress, less the concrete it displaces (as the 1-D analysis)
            let s = (b.steel.stress(e + b.pre) - self.conc.uniaxial(e, 100.0).min(0.0)) * b.a;
            n += s;
            mx -= s * (b.z - self.zc);
            my -= s * (b.x - self.xc);
        }
        (n, mx, my)
    }

    fn solve_e0(&self, kx: f64, ky: f64, n: f64) -> Option<f64> {
        let span = (kx.abs() * self.h + ky.abs() * self.b) * 1.2;
        let (lo0, hi0) = (-0.006 - span, 0.03 + span);
        let g = |e0: f64| self.forces(e0, kx, ky).0 - n;
        if g(hi0) < 0.0 {
            return None;
        }
        // the force is not monotonic in e0 (concrete softens past its peak in deep compression):
        // scan down from the tension side for the first sign change, the stable branch
        let steps = 48;
        let mut prev = hi0;
        let mut bracket = None;
        for i in 1..=steps {
            let e = hi0 + (lo0 - hi0) * i as f64 / steps as f64;
            if g(e) <= 0.0 {
                bracket = Some((e, prev));
                break;
            }
            prev = e;
        }
        let (mut lo, mut hi) = bracket?;
        for _ in 0..70 {
            let mid = 0.5 * (lo + hi);
            if g(mid) < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }

    fn extremes(&self, e0: f64, kx: f64, ky: f64) -> (f64, f64, f64) {
        let ec_min = self.fibres.iter().map(|f| self.strain(e0, kx, ky, f.x, f.z)).fold(f64::INFINITY, f64::min);
        let mut et = f64::NEG_INFINITY;
        let mut ety = 0.002;
        for b in &self.bars {
            let e = self.strain(e0, kx, ky, b.x, b.z);
            if e > et {
                et = e;
                ety = match b.steel {
                    SteelProps::Prestress { .. } => 0.002,
                    _ => b.steel.fy() / b.steel.modulus(),
                };
            }
        }
        (ec_min, et, ety)
    }

    /// Moment-curvature at load angle alpha (degrees) and axial load n (kN, tension +).
    pub fn run(&self, alpha_deg: f64, n_kn: f64) -> BiRun {
        let a = alpha_deg.to_radians();
        let n = n_kn * 1000.0;
        let wrap = |d: f64| (d + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;
        let mut pts: Vec<BiPoint> = vec![];
        let mut beta = a;
        let mut kappa = 0.05e-6; // 1/mm (0.05 rad/km)
        let mut stop = String::from("Curvature limit reached.");
        let mut best = 0usize;
        for _ in 0..700 {
            // tilt the neutral axis until the moment points along alpha
            // secant iteration on the tilt: r(beta) = direction of the moment - alpha
            let mut sol = None;
            let eval = |beta: f64| -> Option<(f64, (f64, f64, f64, f64, f64))> {
                let (kx, ky) = (kappa * beta.cos(), kappa * beta.sin());
                let e0 = self.solve_e0(kx, ky, n)?;
                let (_, mx, my) = self.forces(e0, kx, ky);
                let r = if mx.hypot(my) > 1e-6 { wrap(my.atan2(mx) - a) } else { 0.0 };
                Some((r, (e0, kx, ky, mx, my)))
            };
            let mut prev: Option<(f64, f64)> = None;
            let mut best_r = f64::INFINITY;
            for _ in 0..40 {
                let Some((r, s)) = eval(beta) else { break };
                if r.abs() < best_r {
                    best_r = r.abs();
                    sol = Some((beta, s));
                }
                if r.abs() < 1e-5 {
                    break;
                }
                let next = match prev {
                    Some((b0, r0)) if (r - r0).abs() > 1e-12 => beta - r * (beta - b0) / (r - r0),
                    _ => beta - r,
                };
                prev = Some((beta, r));
                // at most 20 degrees a step
                let lim = 20f64.to_radians();
                beta += (next - beta).clamp(-lim, lim);
            }
            if let Some((b, _)) = sol {
                beta = b;
            }
            let sol = sol.map(|(_, s)| s);
            let Some((e0, kx, ky, mx, my)) = sol else {
                stop = "No balanced state for this axial load at a larger curvature.".into();
                break;
            };
            let (ec_min, et_max, ety) = self.extremes(e0, kx, ky);
            let m = (mx * a.cos() + my * a.sin()) / 1e6;
            pts.push(BiPoint { kappa: kappa * 1e6, m, mx: mx / 1e6, my: my / 1e6, beta: beta.to_degrees(), e0, ec_min, et_max, ety });
            let k = pts.len() - 1;
            if m > pts[best].m {
                best = k;
            }
            // the same stops as the moment-curvature run (analysis::moment_curvature): the
            // most compressed fibre at -12 mm/m, or the moment below 60 % of its peak
            if ec_min < -0.012 {
                stop = "Concrete strain limit reached at the most compressed corner.".into();
                break;
            }
            if k > 10 && pts[best].m > 0.0 && m < 0.6 * pts[best].m {
                stop = "Moment dropped well below the peak.".into();
                break;
            }
            kappa *= 1.03;
        }
        BiRun { alpha: alpha_deg, n: n_kn, peak: best, points: pts, stop }
    }
}

/// The Mx-My diagram at axial load n (kN): runs at `dirs` evenly spaced angles, and the run at
/// the chosen angle. Runs in parallel.
pub fn diagram(sec: &Section, n_kn: f64, alpha_deg: f64, dirs: usize, progress: &(dyn Fn(f32) + Sync)) -> Result<BiDiagram, String> {
    let m = BiModel::new(sec)?;
    let dirs = dirs.clamp(8, 72);
    let angles: Vec<f64> = (0..dirs).map(|i| 360.0 * i as f64 / dirs as f64).collect();
    // one worker per core (pool.rs); the chosen angle's run is one more job
    let mut all = angles.clone();
    all.push(alpha_deg);
    let mut runs: Vec<BiRun> = crate::pool::map(&all, None, &|&al| m.run(al, n_kn), progress);
    let chosen = runs.pop().unwrap_or_else(|| m.run(alpha_deg, n_kn));
    Ok(BiDiagram { n: n_kn, runs, chosen })
}

/// ACI 318 design moment of a run: phi from the net tensile strain of the most stretched bar
/// when the most compressed concrete fibre reaches ecu (0.003), read along the run; phi times
/// the run's peak.
pub fn aci_phi(run: &BiRun, a: &AciFactors) -> Option<(f64, f64)> {
    let pts = &run.points;
    for w in pts.windows(2) {
        let (p, q) = (&w[0], &w[1]);
        if -p.ec_min < a.ecu && -q.ec_min >= a.ecu {
            let t = (-a.ecu - p.ec_min) / (q.ec_min - p.ec_min).min(-1e-12);
            let et = p.et_max + t.clamp(0.0, 1.0) * (q.et_max - p.et_max);
            return Some((a.phi_flexure(et, q.ety), et));
        }
    }
    // never reached ecu before the peak: use the last state
    pts.last().map(|p| (a.phi_flexure(p.et_max, p.ety), p.et_max))
}
