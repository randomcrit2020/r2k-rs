//! Strain-driven, path-following MCFT section solver for the shear analyses (designed with a
//! planning review, see MECHANICS.md).
//!
//! * Each concrete layer is given its longitudinal strain ex (plane sections) and its shear
//!   STRAIN gamma; it solves only its transverse strain ez from "no net transverse stress"
//!   (concrete + stirrups), a 1-D root found next to the previous answer. That has one
//!   continuous answer also after the layer's peak, unlike the old "given shear stress" form.
//! * The section unknowns are (ec, phi, gamma_1..gamma_n, V). Equations: axial force, moment
//!   M = (M/V) x V, and for every layer "carries its share q_i of V". The shares come from the
//!   change of longitudinal force between two neighbouring sections (dual section) and are
//!   converged to a fixed point inside every step.
//! * The equilibrium path is traced by pseudo-arc-length, so peaks, the bars' yield plateau and
//!   post-peak branches are followed without stalling. The capacity is the highest V on the path.

use crate::analysis::*;
use crate::materials::*;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
pub struct LayerOut {
    pub sx: f64,
    pub tau: f64,
    pub ez: f64,
    pub e1: f64,
    pub e2: f64,
    pub f1: f64,
    pub s2: f64,
    pub theta: f64,
    pub fsz: f64,
    pub w: f64,
    pub ok: bool,
}

fn stirrup_of(m: &SModel, i: usize) -> (Option<&SteelProps>, f64) {
    match &m.stirrup {
        Some((s, _)) if m.layers[i].rho_z > 0.0 => (Some(s), m.layers[i].rho_z),
        _ => (None, 0.0),
    }
}

/// Whether the original's reduction of the cracking stress under compression is applied.
fn crack_reduction_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SR_NO_CRACK_REDUCTION").is_err())
}

fn crack_mode_a() -> bool {
    static A: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var("SR_CRACK_MODE").map(|v| v == "A").unwrap_or(false))
}

/// Layer stresses at trial transverse strain `ez`; returns (net transverse stress, layer).
fn layer_point(m: &SModel, i: usize, sz: f64, ex: f64, g: f64, ez: f64, rx: Option<f64>) -> (f64, LayerOut) {
    let c = &m.conc;
    let ly = &m.layers[i];
    let (steel, rho) = stirrup_of(m, i);
    let ga = g.abs();
    let cm = 0.5 * (ex + ez);
    let r = ((0.5 * (ex - ez)).powi(2) + (0.5 * ga).powi(2)).sqrt();
    let e1 = cm + r;
    let e2 = cm - r;
    let psi = 0.5 * ga.atan2(ex - ez); // principal tension direction from x, 0..pi/2
    let theta = PI / 2.0 - psi; // strut angle from x
    let fsz = steel.map(|s| s.stress(ez)).unwrap_or(0.0);
    let s2 = if e2 < 0.0 { c.compression(e2, e1.max(0.0)) } else { c.tension_c(e2, ly.m_bond) };
    // cracking under compression, as the original: the cracking stress (and strain) is reduced
    // by clamp(1.05 - f2/fp, 0.05, 1) - concrete compressed near its strength cracks at a small
    // fraction of ft (SR_NO_CRACK_REDUCTION=1 turns it off, for comparison)
    let kct = if crack_reduction_on() && s2 < 0.0 { c.crack_factor(-s2, e1) } else { 1.0 };
    let ecr = c.ecr * kct;
    // mode A (SR_CRACK_MODE=A): the tension curve itself unreduced, only the crack check starting
    // at the reduced cracking strain (closer to the original's code); default: the whole curve
    // scaled
    let th = theta.clamp(1e-9, PI / 2.0 - 1e-9);
    let stheta = 1.0 / (th.sin() / ly.sx + th.cos() / sz);
    let w = e1.max(0.0) * stheta;
    let mut f1t = if crack_mode_a() { c.tension_c(e1, ly.m_bond) } else { c.tension_ck(e1, ly.m_bond, kct) };
    // after cracking the original keeps a floor under the stress across the crack:
    // ft' / (1 + 250 w), ft' the reduced cracking stress, w in mm (0x4ac920, 250000 per m at
    // 0x53b220). It is applied last in the crack check (0x46aaa0), after the cap, whether or not
    // compression reduced ft'. Opt-in (SR_FLOOR_LAST=1): measured, it matched the original no
    // better (M/V 1.5 m -5.1 to -2.8 %, but the strain-driven solver stalls at web cracking at
    // N = -6000 kN); default: the earlier reading, only under compression and before the cap
    static FLOOR_OLD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let floor_old = *FLOOR_OLD.get_or_init(|| std::env::var("SR_FLOOR_LAST").is_err());
    let post_floor = c.ft * kct / (1.0 + 250.0 * w);
    if floor_old && kct < 1.0 && e1 > ecr {
        f1t = -smin(-f1t, -post_floor, 0.02 * c.ft * kct);
    }
    // crack check, ramped in continuously below cracking
    // (smooth versions of max/min so the layer response has no corners for the path solver)
    let mut lim = c.vci_max(w) * th.tan() + c.ec * splus(ecr - e1, 0.1 * ecr);
    if let Some(s) = steel {
        lim += m.stirrup_reserve_factor() * rho * splus(s.fy() - fsz, 0.02 * s.fy());
    }
    // the longitudinal bars' side of the same check (the original's f1 <= rx + vci / tan theta,
    // see SModel::long_reserve), ramped in the same way below cracking
    let mut floor_f1 = 0.0;
    static FULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let full = *FULL.get_or_init(|| std::env::var("SR_LONG_RESERVE").map(|v| v == "full").unwrap_or(false));
    if let (Some(rx), false) = (rx, full) {
        let limx = rx.max(0.0) + c.vci_max(w) / th.tan() + c.ec * splus(ecr - e1, 0.1 * ecr);
        lim = smin(lim, limx, 0.02 * c.ft * kct);
    }
    if let (Some(rx), true) = (rx, full) {
        // 0x46aaa0 in full: the shear on the crack is at most what the two reserves' difference
        // allows, |rx - rz| sin cos; f1 <= min(rx + v/tan, rz + v tan)
        let rx = rx.max(0.0);
        let rz = steel.map(|s| m.stirrup_reserve_factor() * rho * splus(s.fy() - fsz, 0.02 * s.fy())).unwrap_or(0.0);
        let (sn, cs, tn) = (th.sin(), th.cos(), th.tan());
        let k = 0.02 * c.ft * kct;
        let vu = smin(c.vci_max(w), (rx - rz).abs() * sn * cs, k);
        let ramp = c.ec * splus(ecr - e1, 0.1 * ecr);
        lim = smin(rz + vu * tn, rx + vu / tn, k) + ramp;
        // Bentz tension stiffening (type 7) keeps a lower bound once cracked:
        // min(rx / 2, 0.85 ft tan + rz, ts min(0.25 ft / (0.31 + 24 w / (a + 16)) / tan, ft'))
        // (constants 0x52e5a0 = 0.25, 0x52e5d0 = 0.85; ft' the reduced cracking stress, 0x46a960)
        if c.tension_stiffening {
            let a = c.agg_eff;
            let lo = smin(0.25 * c.ft / (0.31 + 24.0 * w / (a + 16.0)) / tn, c.ft * kct, k) * c.ts;
            let fl = smin(0.5 * rx, smin(0.85 * c.ft * tn + rz, lo, k), k);
            let on = ((e1 - ecr) / (0.1 * ecr)).clamp(0.0, 1.0);
            floor_f1 = fl.max(0.0) * on * on * (3.0 - 2.0 * on);
        }
    }
    // (well below cracking the crack check cannot bind: use the plain value there)
    let f1 = if e1 < 0.5 * ecr {
        f1t
    } else {
        let raised = if floor_f1 > 0.0 { -smin(-f1t, -floor_f1, 0.02 * c.ft * kct) } else { f1t };
        let capped = smin(raised, lim.max(0.0), 0.02 * c.ft * kct).max(0.0);
        if floor_old || e1 <= ecr {
            capped
        } else {
            // ramped in over one cracking strain past cracking (a sharper ramp stalls the
            // strain-driven solver at web cracking under heavy compression)
            let on = ((e1 - ecr) / ecr).clamp(0.0, 1.0);
            let fl = post_floor * on * on * (3.0 - 2.0 * on);
            -smin(-capped, -fl, 0.02 * c.ft * kct)
        }
    };
    // stresses from the principal stresses without the angle (smooth as the strains vanish):
    // mean stress plus a secant stiffness k = (f1 - s2)/(e1 - e2) times the strain differences
    let k = if e1 - e2 > 1e-12 { (f1 - s2) / (e1 - e2) } else { c.ec };
    let mean = 0.5 * (f1 + s2);
    let sx = mean + 0.5 * k * (ex - ez);
    let szc = mean - 0.5 * k * (ex - ez);
    let tau = 0.5 * k * g;
    (szc + rho * fsz, LayerOut { sx, tau, ez, e1, e2, f1, s2, theta, fsz, w, ok: true })
}

/// Solve the layer's transverse strain next to `anchor` (the nearest crossing).
pub fn layer_solve(m: &SModel, i: usize, sz: f64, ex: f64, g: f64, anchor: f64, rx: Option<f64>) -> LayerOut {
    let (steel, _) = stirrup_of(m, i);
    let lo = -0.02;
    let hi = steel.map(|s| 0.98 * s.eu()).unwrap_or(0.2);
    let f = |ez: f64| layer_point(m, i, sz, ex, g, ez, rx);
    let a0 = anchor.clamp(lo, hi);
    let (fa0, s0) = f(a0);
    let tol = 1e-9 * m.conc.fc;
    if fa0.abs() < tol {
        return s0;
    }
    let dir = if fa0 < 0.0 { 1.0 } else { -1.0 };
    let mut d = 1e-6 + 0.02 * (g.abs() + (ex - a0).abs());
    let (mut x0, mut f0) = (a0, fa0);
    let bracket;
    loop {
        let x1 = (x0 + dir * d).clamp(lo, hi);
        let (f1, _) = f(x1);
        if (f1 <= 0.0) != (f0 <= 0.0) {
            bracket = (x0, f0, x1, f1);
            break;
        }
        if x1 <= lo || x1 >= hi {
            let (_, mut st) = f(x1);
            st.ok = false;
            return st;
        }
        x0 = x1;
        f0 = f1;
        d *= 2.0;
    }
    // Illinois (modified regula falsi)
    let (mut a, mut fa, mut b, mut fb) = bracket;
    let mut side = 0;
    let mut best = s0;
    for _ in 0..100 {
        let xc = if (fb - fa).abs() > 1e-300 { (a * fb - b * fa) / (fb - fa) } else { 0.5 * (a + b) };
        let (fc, st) = f(xc);
        best = st;
        if fc.abs() < tol || (b - a).abs() < 1e-13 {
            break;
        }
        if (fc <= 0.0) == (fb <= 0.0) {
            b = xc;
            fb = fc;
            if side == -1 {
                fa *= 0.5;
            }
            side = -1;
        } else {
            a = xc;
            fa = fc;
            if side == 1 {
                fb *= 0.5;
            }
            side = 1;
        }
    }
    best
}

pub struct SecEval {
    pub n: f64,
    pub mm: f64,
    pub outs: Vec<LayerOut>,
    pub bar_e: Vec<f64>,
    pub bar_s: Vec<f64>,
    /// per layer: d sx/d ex, d sx/d g, d tau/d ex, d tau/d g
    pub lin: Vec<[f64; 4]>,
    pub bar_kt: Vec<f64>,
    pub ok: bool,
    /// per layer: net transverse stress (concrete + stirrups), MPa
    pub trans: Vec<f64>,
}

fn section_eval(m: &SModel, sz: f64, ec: f64, phi: f64, g: &[f64], anchor: &[f64], tangents: bool) -> SecEval {
    let nl = m.layers.len();
    let mut s = SecEval { n: 0.0, mm: 0.0, outs: Vec::with_capacity(nl), bar_e: vec![], bar_s: vec![], lin: vec![], bar_kt: vec![], ok: true, trans: vec![] };
    let rxs = m.long_reserve(ec, phi);
    for i in 0..nl {
        let l = &m.layers[i];
        let ex = m.strain(ec, phi, l.z);
        let o = layer_solve(m, i, sz, ex, g[i], anchor[i], rxs.as_ref().map(|v| v[i]));
        if !o.ok {
            s.ok = false;
        }
        let f = o.sx * l.b * l.t;
        s.n += f;
        s.mm -= f * (l.z - m.zc);
        if tangents {
            let hx = 2e-7;
            let hg = 2e-7 + 1e-4 * g[i].abs();
            let ox = layer_solve(m, i, sz, ex + hx, g[i], o.ez, rxs.as_ref().map(|v| v[i]));
            let og = layer_solve(m, i, sz, ex, g[i] + hg, o.ez, rxs.as_ref().map(|v| v[i]));
            s.lin.push([(ox.sx - o.sx) / hx, (og.sx - o.sx) / hg, (ox.tau - o.tau) / hx, (og.tau - o.tau) / hg]);
        }
        s.outs.push(o);
    }
    for b in &m.bars {
        let e = m.strain(ec, phi, b.z) + b.pre;
        let st = b.steel.stress(e);
        s.n += st * b.area;
        s.mm -= st * b.area * (b.z - m.zc);
        if tangents {
            let h = 1e-7;
            s.bar_kt.push((b.steel.stress(e + h) - st) / h);
        }
        s.bar_e.push(e);
        s.bar_s.push(st);
    }
    s
}

/// Shares of V per layer (sum 1) from two neighbouring sections at curvatures phi +- h/2 with
/// the same shear-strain profile, each balanced for the axial force.
fn flow_shape(m: &SModel, sz: f64, ec: f64, phi: f64, g: &[f64], anchor: &[f64], nt: f64) -> Option<Vec<f64>> {
    let t0 = std::time::Instant::now();
    let out = flow_shape_inner(m, sz, ec, phi, g, anchor, nt);
    prof_add(2, t0);
    out
}

/// The shear-flow shape from a secant dual section (SR_FLOW_SECANT=1): as the original's routine
/// reads, "a linear strain change between two nearby sections times each node's current
/// stiffness" - here the secant stiffness (stress over strain) of every node: a concrete layer's
/// uniaxial curve (Ec at zero strain, small once cracked, still well above zero near the
/// compressive peak) and each bar's stress over its total strain (prestrain included). The
/// strain change is a unit curvature change with the centroid strain change that keeps the
/// axial force unchanged. Unlike the difference of two full solutions (the default, in effect
/// the tangent), a bar on its yield plateau keeps its secant stiffness, so the shear flow still
/// runs down the web to the bars after yield instead of staying in the compression zone.
/// Returns the change of longitudinal force per layer and per bar.
fn secant_changes(m: &SModel, ec: f64, phi: f64) -> (Vec<f64>, Vec<f64>) {
    let ecm = m.conc.ec;
    let e_layer: Vec<f64> = m
        .layers
        .iter()
        .map(|l| {
            let e = m.strain(ec, phi, l.z);
            if e.abs() < 1e-7 { ecm } else { (m.conc.uniaxial(e, l.m_bond) / e).clamp(0.0, ecm) }
        })
        .collect();
    let e_bar: Vec<f64> = m
        .bars
        .iter()
        .map(|b| {
            let e = m.strain(ec, phi, b.z) + b.pre;
            if e.abs() < 1e-7 { b.steel.modulus() } else { (b.steel.stress(e) / e).abs().min(b.steel.modulus()) }
        })
        .collect();
    // unit curvature change about the centroid, plus the uniform change that keeps N
    let (mut ea, mut eaz) = (0.0, 0.0);
    for (l, e) in m.layers.iter().zip(&e_layer) {
        ea += e * l.b * l.t;
        eaz += e * l.b * l.t * (l.z - m.zc);
    }
    for (b, e) in m.bars.iter().zip(&e_bar) {
        ea += e * b.area;
        eaz += e * b.area * (b.z - m.zc);
    }
    let d0 = if ea > 0.0 { -eaz / ea } else { 0.0 };
    // same orientation as the default (phi + h/2 minus phi - h/2), whose strain is ec - phi (z - zc)
    let de = |z: f64| -(d0 + (z - m.zc));
    let dl = m.layers.iter().zip(&e_layer).map(|(l, e)| e * de(l.z) * l.b * l.t).collect();
    let db = m.bars.iter().zip(&e_bar).map(|(b, e)| e * de(b.z) * b.area).collect();
    (dl, db)
}

fn flow_shape_inner(m: &SModel, sz: f64, ec: f64, phi: f64, g: &[f64], anchor: &[f64], nt: f64) -> Option<Vec<f64>> {
    // Default: the secant dual section once a reinforcing bar (not a strand) has yielded, the
    // difference of two full solutions before that (--check-mechanics 2.50 -> 2.20 %, see
    // MECHANICS.md). SR_FLOW_SECANT=off: the difference throughout (the earlier default);
    // SR_FLOW_SECANT=1: the secant dual section throughout.
    static SEC: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    let mode = *SEC.get_or_init(|| match std::env::var("SR_FLOW_SECANT").as_deref() {
        Ok("off") | Ok("0") => 0,
        Ok("1") | Ok("always") => 1,
        _ => 2,
    });
    let use_secant = match mode {
        1 => true,
        2 => m.bars.iter().any(|b| match b.steel {
            SteelProps::Rebar { fy, es, .. } => (m.strain(ec, phi, b.z) + b.pre).abs() >= 0.98 * fy / es,
            _ => false,
        }),
        _ => false,
    };
    if use_secant {
        let (dl, db) = secant_changes(m, ec, phi);
        let tau = flow_from_changes(m, &dl, &db, 1.0);
        return shares_from_tau(m, &tau);
    }
    let phic = 4.0 * m.conc.ecr / m.h;
    let hphi = 0.04 * (phi * phi + phic * phic).sqrt();
    let mut sides = vec![];
    for sgn in [1.0, -1.0] {
        let ph = phi + sgn * 0.5 * hphi;
        let mut e = ec;
        let mut ev = section_eval(m, sz, e, ph, g, anchor, false);
        for _ in 0..8 {
            let r = ev.n - nt;
            if r.abs() < 1e-6 * m.conc.fc * m.area + 1.0 {
                break;
            }
            let h = 1e-7;
            let ev2 = section_eval(m, sz, e + h, ph, g, anchor, false);
            let k = (ev2.n - ev.n) / h;
            if k.abs() < 1e-12 {
                break;
            }
            e -= r / k;
            ev = section_eval(m, sz, e, ph, g, anchor, false);
        }
        sides.push(ev);
    }
    let (p, q) = (&sides[0], &sides[1]);
    let dl: Vec<f64> = m.layers.iter().enumerate().map(|(i, l)| (p.outs[i].sx - q.outs[i].sx) * l.b * l.t).collect();
    let db: Vec<f64> = m.bars.iter().enumerate().map(|(k, b)| (p.bar_s[k] - q.bar_s[k]) * b.area).collect();
    let tau = flow_from_changes(m, &dl, &db, 1.0);
    shares_from_tau(m, &tau)
}

/// Shear shares (sum 1) from a shear-stress profile: a tiny graded imperfection and the cap.
fn shares_from_tau(m: &SModel, tau: &[f64]) -> Option<Vec<f64>> {
    // a tiny graded imperfection (0.1 % over the depth) so that mirror-image layers do not
    // crack at exactly the same instant (that would make the system singular at cracking)
    let nl = m.layers.len().max(1) as f64;
    let shares: Vec<f64> = m
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| tau[i] * l.b * l.t / 1000.0 * (1.0 + 1e-3 * (i as f64 / nl - 0.5)))
        .collect();
    let sum: f64 = shares.iter().sum();
    if sum <= 0.5 {
        return None;
    }
    let mut q: Vec<f64> = shares.iter().map(|s| s / sum).collect();
    // the original caps each layer's shear stress at 8 x an average one (0x41e840: "... * 8.0",
    // constant at 0x5308f8; 100 x for structural steel). Here the average is taken as V over the
    // concrete area (**estimated**: the original divides a section quantity read at +0x816 by
    // another, not identified), so a layer's share may not exceed 8 x its share of the area; the
    // excess is spread over the others in proportion to theirs.
    let area: f64 = m.layers.iter().map(|l| l.b * l.t).sum::<f64>().max(1e-9);
    // SR_SHARE_CAP: the cap factor (default 8, the original's constant), for calibration studies
    static CAPF: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let capf = *CAPF.get_or_init(|| std::env::var("SR_SHARE_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0));
    for _ in 0..4 {
        let mut excess = 0.0;
        let mut free = 0.0;
        for (i, l) in m.layers.iter().enumerate() {
            let cap = capf * l.b * l.t / area;
            if q[i] > cap {
                excess += q[i] - cap;
                q[i] = cap;
            } else {
                free += q[i];
            }
        }
        if excess <= 1e-12 || free <= 1e-12 {
            break;
        }
        for (i, l) in m.layers.iter().enumerate() {
            if q[i] < capf * l.b * l.t / area {
                q[i] += excess * q[i] / free;
            }
        }
    }
    Some(q)
}

/// Dense LU solve with partial pivoting (a: n x n row-major).
fn lu_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    for k in 0..n {
        let mut p = k;
        for r in k + 1..n {
            if a[r * n + k].abs() > a[p * n + k].abs() {
                p = r;
            }
        }
        if a[p * n + k].abs() < 1e-300 {
            return false;
        }
        if p != k {
            for c in 0..n {
                a.swap(k * n + c, p * n + c);
            }
            b.swap(k, p);
        }
        for r in k + 1..n {
            let f = a[r * n + k] / a[k * n + k];
            if f != 0.0 {
                for c in k..n {
                    a[r * n + c] -= f * a[k * n + c];
                }
                b[r] -= f * b[k];
            }
        }
    }
    for k in (0..n).rev() {
        let mut s = b[k];
        for c in k + 1..n {
            s -= a[k * n + c] * b[c];
        }
        b[k] = s / a[k * n + k];
    }
    true
}

/// The damped normal equations (A + mu diag(A)) d = g of the LM corrector, A = J^T J + w w^T,
/// kept in the structure the section system gives them instead of as a dense matrix.
///
/// Each layer's two rows (shear share, transverse balance) touch only that layer's own two
/// unknowns (gamma_i, ez_i) and the three shared ones (ec, phi, load); only the axial-force row,
/// the moment row and the step's constraint row are full. So A = S + U U^T, where S (the layer
/// rows) is a 2 x 2 block per layer bordered by the three shared unknowns and U holds the three
/// full rows. With s = U^T d as three extra unknowns the system
///     [S  U] [d]   [g]
///     [U' -I] [s] = [0]
/// has the same block-and-border form, now with six border unknowns: every layer block is
/// eliminated on its own and a 6 x 6 system is left. The answer is the dense solve's (to
/// rounding), at a cost that grows with the number of layers instead of its cube.
struct NormalEq {
    nl: usize,
    nu: usize,
    /// diagonal of the full A (the damping is scaled by it, as in the dense solve)
    diag: Vec<f64>,
    /// per layer, the layer rows' part of A on (gamma_i, ez_i): (gg, gz, zz)
    k: Vec<[f64; 3]>,
    /// per layer, the layer rows' part of A between (gamma_i, ez_i) and (ec, phi, load)
    c: Vec<[[f64; 3]; 2]>,
    /// the layer rows' part of A on (ec, phi, load)
    e: [[f64; 3]; 3],
    /// the three full rows: axial force, moment, constraint
    u: [Vec<f64>; 3],
    /// right-hand side J^T r + w (w.d - target)
    g: Vec<f64>,
}

impl NormalEq {
    /// From the slope matrix j (nr x nu, nr = 2 nl + 2), the residuals r and the weighted
    /// constraint row and its right-hand side. None when a layer row reaches outside its own
    /// unknowns and the shared ones (the dense solve is used then).
    fn build(j: &[f64], r: &[f64], wrow: &[f64], wrhs: f64, nl: usize) -> Option<NormalEq> {
        let nu = 2 * nl + 3;
        let nr = 2 * nl + 2;
        if j.len() != nr * nu || r.len() != nr || wrow.len() != nu {
            return None;
        }
        let border = [0, 1, nu - 1];
        let mut ne = NormalEq {
            nl,
            nu,
            diag: vec![0.0; nu],
            k: vec![[0.0; 3]; nl],
            c: vec![[[0.0; 3]; 2]; nl],
            e: [[0.0; 3]; 3],
            u: [j[..nu].to_vec(), j[nu..2 * nu].to_vec(), wrow.to_vec()],
            g: vec![0.0; nu],
        };
        let rhs_full = [r[0], r[1], wrhs];
        for (k, row) in ne.u.iter().enumerate() {
            for (col, v) in row.iter().enumerate() {
                ne.g[col] += v * rhs_full[k];
                ne.diag[col] += v * v;
            }
        }
        for rr in 2..nr {
            let i = (rr - 2) % nl;
            let (cg, cz) = (2 + i, 2 + nl + i);
            let row = &j[rr * nu..(rr + 1) * nu];
            // a layer row must be zero outside its five columns
            for (col, v) in row.iter().enumerate() {
                if *v != 0.0 && col != cg && col != cz && col != 0 && col != 1 && col != nu - 1 {
                    return None;
                }
            }
            let rhs = r[rr];
            let al = [row[cg], row[cz]];
            let ab = [row[border[0]], row[border[1]], row[border[2]]];
            let kk = &mut ne.k[i];
            kk[0] += al[0] * al[0];
            kk[1] += al[0] * al[1];
            kk[2] += al[1] * al[1];
            for l in 0..2 {
                for b in 0..3 {
                    ne.c[i][l][b] += al[l] * ab[b];
                }
                ne.g[[cg, cz][l]] += al[l] * rhs;
                ne.diag[[cg, cz][l]] += al[l] * al[l];
            }
            for a in 0..3 {
                for b in 0..3 {
                    ne.e[a][b] += ab[a] * ab[b];
                }
                ne.g[border[a]] += ab[a] * rhs;
                ne.diag[border[a]] += ab[a] * ab[a];
            }
        }
        Some(ne)
    }

    /// The step d of (A + mu (diag(A) + 1e-12)) d = g; None when a pivot vanishes. One round of
    /// iterative refinement (the leftover g - (A + damping) d, worked out with the structure,
    /// solved for again) brings it to the dense LU solve's precision: without it the steps
    /// of nearly singular systems (tiny mu) came out less precise and moved a few runs' peaks.
    fn solve(&self, mu: f64) -> Option<Vec<f64>> {
        let fac = self.factor(mu)?;
        let mut d = self.apply(&fac, &self.g)?;
        let ad = self.product(mu, &d);
        let left: Vec<f64> = self.g.iter().zip(&ad).map(|(g, a)| g - a).collect();
        if let Some(dd) = self.apply(&fac, &left) {
            for (x, y) in d.iter_mut().zip(&dd) {
                *x += y;
            }
        }
        if d.iter().all(|v| v.is_finite()) { Some(d) } else { None }
    }

    fn damp(&self, mu: f64, col: usize) -> f64 {
        mu * (self.diag[col] + 1e-12)
    }

    /// Eliminates every layer block from the bordered system [S + damping, U; U^T, -I]: what is
    /// kept is each block's inverse and coupling, and the 6 x 6 system left on (ec, phi, load,
    /// s1, s2, s3).
    fn factor(&self, mu: f64) -> Option<NeFactor> {
        let (nl, nu) = (self.nl, self.nu);
        let border = [0, 1, nu - 1];
        let mut z = [[0.0f64; 6]; 6];
        for a in 0..3 {
            for b in 0..3 {
                z[a][b] = self.e[a][b];
            }
            z[a][a] += self.damp(mu, border[a]);
            for k in 0..3 {
                z[a][3 + k] = self.u[k][border[a]];
                z[3 + k][a] = self.u[k][border[a]];
            }
            z[3 + a][3 + a] = -1.0;
        }
        // each layer block: K x + F y = g_l, with F its coupling to the six border unknowns
        let mut inv = vec![[[0.0f64; 2]; 2]; nl];
        let mut f = vec![[[0.0f64; 6]; 2]; nl];
        for i in 0..nl {
            let (cg, cz) = (2 + i, 2 + nl + i);
            let kk = &self.k[i];
            let (k00, k01, k11) = (kk[0] + self.damp(mu, cg), kk[1], kk[2] + self.damp(mu, cz));
            let det = k00 * k11 - k01 * k01;
            if !det.is_finite() || det.abs() < 1e-300 {
                return None;
            }
            inv[i] = [[k11 / det, -k01 / det], [-k01 / det, k00 / det]];
            for (l, col) in [cg, cz].into_iter().enumerate() {
                f[i][l][..3].copy_from_slice(&self.c[i][l]);
                for k in 0..3 {
                    f[i][l][3 + k] = self.u[k][col];
                }
            }
            // Schur complement: Z -= F^T K^-1 F
            let mut kf = [[0.0f64; 6]; 2];
            for l in 0..2 {
                for b in 0..6 {
                    kf[l][b] = inv[i][l][0] * f[i][0][b] + inv[i][l][1] * f[i][1][b];
                }
            }
            for a in 0..6 {
                let (fa0, fa1) = (f[i][0][a], f[i][1][a]);
                if fa0 == 0.0 && fa1 == 0.0 {
                    continue;
                }
                for b in 0..6 {
                    z[a][b] -= fa0 * kf[0][b] + fa1 * kf[1][b];
                }
            }
        }
        Some(NeFactor { inv, f, z })
    }

    /// Solves the factored system for the right-hand side rhs (length nu).
    fn apply(&self, fac: &NeFactor, rhs: &[f64]) -> Option<Vec<f64>> {
        let (nl, nu) = (self.nl, self.nu);
        let border = [0, 1, nu - 1];
        let mut rz = [0.0f64; 6];
        for a in 0..3 {
            rz[a] = rhs[border[a]];
        }
        let mut kg = vec![[0.0f64; 2]; nl];
        for i in 0..nl {
            let gl = [rhs[2 + i], rhs[2 + nl + i]];
            let inv = &fac.inv[i];
            kg[i] = [inv[0][0] * gl[0] + inv[0][1] * gl[1], inv[1][0] * gl[0] + inv[1][1] * gl[1]];
            for a in 0..6 {
                rz[a] -= fac.f[i][0][a] * kg[i][0] + fac.f[i][1][a] * kg[i][1];
            }
        }
        let mut zf: Vec<f64> = fac.z.iter().flat_map(|row| row.iter().copied()).collect();
        let mut yb = rz.to_vec();
        if !lu_solve(&mut zf, 6, &mut yb) {
            return None;
        }
        let mut d = vec![0.0; nu];
        for a in 0..3 {
            d[border[a]] = yb[a];
        }
        for i in 0..nl {
            // x = K^-1 (g_l - F y)
            let fy = [(0..6).map(|b| fac.f[i][0][b] * yb[b]).sum::<f64>(), (0..6).map(|b| fac.f[i][1][b] * yb[b]).sum::<f64>()];
            let inv = &fac.inv[i];
            d[2 + i] = kg[i][0] - (inv[0][0] * fy[0] + inv[0][1] * fy[1]);
            d[2 + nl + i] = kg[i][1] - (inv[1][0] * fy[0] + inv[1][1] * fy[1]);
        }
        Some(d)
    }

    /// (A + mu (diag(A) + 1e-12)) d, from the structure.
    fn product(&self, mu: f64, d: &[f64]) -> Vec<f64> {
        let (nl, nu) = (self.nl, self.nu);
        let border = [0, 1, nu - 1];
        let db = [d[border[0]], d[border[1]], d[border[2]]];
        let mut out: Vec<f64> = (0..nu).map(|col| self.damp(mu, col) * d[col]).collect();
        // the full rows: U (U^T d)
        for row in &self.u {
            let s: f64 = row.iter().zip(d).map(|(a, b)| a * b).sum();
            for (o, a) in out.iter_mut().zip(row) {
                *o += a * s;
            }
        }
        // the layer rows: border block, layer blocks and their couplings
        for a in 0..3 {
            out[border[a]] += (0..3).map(|b| self.e[a][b] * db[b]).sum::<f64>();
        }
        for i in 0..nl {
            let (cg, cz) = (2 + i, 2 + nl + i);
            let x = [d[cg], d[cz]];
            let kk = &self.k[i];
            out[cg] += kk[0] * x[0] + kk[1] * x[1] + (0..3).map(|b| self.c[i][0][b] * db[b]).sum::<f64>();
            out[cz] += kk[1] * x[0] + kk[2] * x[1] + (0..3).map(|b| self.c[i][1][b] * db[b]).sum::<f64>();
            for a in 0..3 {
                out[border[a]] += self.c[i][0][a] * x[0] + self.c[i][1][a] * x[1];
            }
        }
        out
    }
}

/// `NormalEq::factor`'s result: each layer block's inverse and border coupling, and the 6 x 6
/// system left on the border.
struct NeFactor {
    inv: Vec<[[f64; 2]; 2]>,
    f: Vec<[[f64; 6]; 2]>,
    z: [[f64; 6]; 6],
}

/// The section system. Unknowns x = (ec, phi, gamma_1..n, ez_1..n, V): the transverse strain of
/// every layer is an unknown of the whole system (with "no net transverse stress" as its
/// equation), so a layer whose response folds back at cracking is followed by the path solver
/// instead of making its stresses jump.
struct Sys<'a> {
    m: &'a SModel,
    sz: f64,
    lo: Lo,
    scale: Vec<f64>,
    tol: Vec<f64>,
}

/// Loads as the original applies them: constant + load factor x increment, for the axial force,
/// the moment and the shear. Here the load unknown p is in newtons of "increment size" g:
/// N = n0 + dn p, M = m0 + dm p, V = v0 + dv p (N, N*mm), and the original's load factor is
/// p / (1000 g).
#[derive(Clone, Copy, Debug)]
struct Lo {
    n0: f64,
    m0: f64,
    v0: f64,
    dn: f64,
    dm: f64,
    dv: f64,
    g: f64,
}

impl Lo {
    /// constants (kN, kNm, kN) and increments (kN, kNm, kN), analysis frame
    fn new(c: (f64, f64, f64), d: (f64, f64, f64), h_mm: f64) -> Lo {
        let g = if d.2.abs() > 1e-12 {
            d.2.abs()
        } else if d.1.abs() > 1e-12 {
            d.1.abs() / (0.4 * h_mm / 1000.0)
        } else if d.0.abs() > 1e-12 {
            0.2 * d.0.abs()
        } else {
            1.0
        };
        Lo { n0: c.0 * 1000.0, m0: c.1 * 1e6, v0: c.2 * 1000.0, dn: d.0 / g, dm: d.1 * 1000.0 / g, dv: d.2 / g, g }
    }
    fn at(&self, p: f64) -> (f64, f64, f64) {
        (self.n0 + self.dn * p, self.m0 + self.dm * p, self.v0 + self.dv * p)
    }
    fn factor(&self, p: f64) -> f64 {
        p / (1000.0 * self.g)
    }
}

impl<'a> Sys<'a> {
    fn nl(&self) -> usize {
        self.m.layers.len()
    }
    fn nu(&self) -> usize {
        2 * self.nl() + 3
    }
    fn unpack(&self, y: &[f64]) -> (f64, f64, Vec<f64>, Vec<f64>, f64) {
        let nl = self.nl();
        let x: Vec<f64> = y.iter().zip(&self.scale).map(|(v, s)| v / s).collect();
        (x[0], x[1], x[2..2 + nl].to_vec(), x[2 + nl..2 + 2 * nl].to_vec(), x[2 + 2 * nl])
    }
    /// scaled residuals: N, M, layer shear shares, layer transverse balance
    fn resid(&self, y: &[f64], q: &[f64]) -> (Vec<f64>, SecEval) {
        let t0 = std::time::Instant::now();
        let out = self.resid_inner(y, q);
        prof_add(3, t0);
        out
    }
    fn resid_inner(&self, y: &[f64], q: &[f64]) -> (Vec<f64>, SecEval) {
        let m = self.m;
        let nl = self.nl();
        let (ec, phi, g, ez, p) = self.unpack(y);
        let ev = section_direct(m, self.sz, ec, phi, &g, &ez);
        let (nn, mm, v) = self.lo.at(p);
        let mut r = vec![0.0; 2 * nl + 2];
        r[0] = (ev.n - nn) / self.tol[0];
        r[1] = (ev.mm - mm) / self.tol[1];
        for i in 0..nl {
            let ar = m.layers[i].b * m.layers[i].t;
            r[2 + i] = (ev.outs[i].tau * ar - q[i] * v) / self.tol[2 + i];
            r[2 + nl + i] = ev.trans[i] * ar / self.tol[2 + nl + i];
        }
        (r, ev)
    }
    /// slope matrix by direct perturbation of every unknown (scaled units)
    fn jac(&self, y: &[f64], q: &[f64], r: &[f64]) -> Vec<f64> {
        let t0 = std::time::Instant::now();
        let out = self.jac_inner(y, q, r);
        prof_add(0, t0);
        out
    }
    /// Every column by its own perturbation (one section evaluation per unknown): the reference
    /// the structured version is checked against (SR_JACFD=1 uses it).
    fn jac_full(&self, y: &[f64], q: &[f64], r: &[f64]) -> Vec<f64> {
        let nu = self.nu();
        let nr = r.len();
        let mut jj = vec![0.0; nr * nu];
        for c in 0..nu {
            let h = 1e-6 * (1.0 + y[c].abs());
            let mut yp = y.to_vec();
            yp[c] += h;
            let (rp, _) = self.resid(&yp, q);
            for rr in 0..nr {
                jj[rr * nu + c] = (rp[rr] - r[rr]) / h;
            }
        }
        jj
    }
    /// The same slope matrix from 6 section evaluations instead of one per unknown. Each layer's
    /// shear strain and transverse strain act only on that layer (its own two equations, and its
    /// share of the axial force and moment), so all the layers' shear strains can be perturbed
    /// at once and each layer's response read separately; likewise all transverse strains. Only
    /// the centroid strain, the curvature and the load need their own evaluation.
    fn jac_inner(&self, y: &[f64], q: &[f64], r: &[f64]) -> Vec<f64> {
        if std::env::var("SR_JACFD").is_ok() {
            return self.jac_full(y, q, r);
        }
        let m = self.m;
        let nl = self.nl();
        let nu = self.nu();
        let nr = r.len();
        let mut jj = vec![0.0; nr * nu];
        let hof = |k: usize| 1e-6 * (1.0 + y[k].abs());
        // centroid strain, curvature and load: plain columns
        for c in [0, 1, nu - 1] {
            let h = hof(c);
            let mut yp = y.to_vec();
            yp[c] += h;
            let (rp, _) = self.resid(&yp, q);
            for rr in 0..nr {
                jj[rr * nu + c] = (rp[rr] - r[rr]) / h;
            }
        }
        // the layers' own unknowns, a block at a time
        let (_, ev0) = self.resid(y, q);
        for block in 0..2 {
            let off = 2 + block * nl;
            let mut yp = y.to_vec();
            for i in 0..nl {
                yp[off + i] += hof(off + i);
            }
            let (_, ev) = self.resid(&yp, q);
            for i in 0..nl {
                let c = off + i;
                let h = hof(c);
                let l = &m.layers[i];
                let ar = l.b * l.t;
                let dsx = (ev.outs[i].sx - ev0.outs[i].sx) * ar;
                jj[c] = dsx / self.tol[0] / h; // axial force row
                jj[nu + c] = -dsx * (l.z - m.zc) / self.tol[1] / h; // moment row
                jj[(2 + i) * nu + c] = (ev.outs[i].tau - ev0.outs[i].tau) * ar / self.tol[2 + i] / h;
                jj[(2 + nl + i) * nu + c] = (ev.trans[i] - ev0.trans[i]) * ar / self.tol[2 + nl + i] / h;
            }
        }
        jj
    }
}

/// Time spent in the main parts of the solver (nanoseconds), for SR_PROF.
pub static PROF: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
pub const PROF_NAMES: [&str; 4] = ["slope matrix (inside corrector)", "corrector, all of it", "shear-flow shape (dual section)", "residual evaluations (inside both)"];
fn prof_add(k: usize, t0: std::time::Instant) {
    PROF[k].fetch_add(t0.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
}

/// Section forces with every layer's transverse strain given (no inner root search).
fn section_direct(m: &SModel, sz: f64, ec: f64, phi: f64, g: &[f64], ez: &[f64]) -> SecEval {
    let nl = m.layers.len();
    let mut s = SecEval { n: 0.0, mm: 0.0, outs: Vec::with_capacity(nl), bar_e: vec![], bar_s: vec![], lin: vec![], bar_kt: vec![], ok: true, trans: Vec::with_capacity(nl) };
    let rxs = m.long_reserve(ec, phi);
    for i in 0..nl {
        let l = &m.layers[i];
        let ex = m.strain(ec, phi, l.z);
        let (f, o) = layer_point(m, i, sz, ex, g[i], ez[i], rxs.as_ref().map(|v| v[i]));
        let fz = f * l.b * l.t;
        let _ = fz;
        s.trans.push(f);
        let fx = o.sx * l.b * l.t;
        s.n += fx;
        s.mm -= fx * (l.z - m.zc);
        s.outs.push(o);
    }
    for b in &m.bars {
        let e = m.strain(ec, phi, b.z) + b.pre;
        let st = b.steel.stress(e);
        s.n += st * b.area;
        s.mm -= st * b.area * (b.z - m.zc);
        s.bar_e.push(e);
        s.bar_s.push(st);
    }
    s
}

fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// One corrector: Newton on [R(y; q) = 0, w.(y - yk) = target].
fn correct(sys: &Sys, yk: &[f64], w: &[f64], target: f64, y0: &[f64], q: &[f64]) -> Option<(Vec<f64>, SecEval, usize)> {
    let t0 = std::time::Instant::now();
    let out = if std::env::var("SR_NEWTON").is_ok() { correct_newton(sys, yk, w, target, y0, q) } else { correct_lm(sys, yk, w, target, y0, q) };
    prof_add(1, t0);
    out
}

/// Damped least squares (Levenberg-Marquardt): minimises the scaled residuals with the
/// constraint as a heavily weighted extra equation; the damping grows when a step does not
/// help (small careful steps over sharp features) and shrinks when it does (Newton speed).
fn correct_lm(sys: &Sys, yk: &[f64], w: &[f64], target: f64, y0: &[f64], q: &[f64]) -> Option<(Vec<f64>, SecEval, usize)> {
    let nu = yk.len();
    let wn = norm2(w).max(1e-300);
    let wc = 1e4 / wn; // weight of the constraint equation
    let cons = |y: &[f64]| -> f64 { w.iter().zip(y).zip(yk).map(|((a, b), c)| a * (b - c)).sum::<f64>() - target };
    let mut y = y0.to_vec();
    // start exactly on the constraint plane
    let c0 = cons(&y);
    for k in 0..nu {
        y[k] -= c0 * w[k] / (wn * wn);
    }
    let (mut r, mut ev) = sys.resid(&y, q);
    let merit = |r: &[f64], c: f64| norm2(r).powi(2) + (wc * c).powi(2);
    let mut f = merit(&r, cons(&y));
    let mut mu = 1e-3;
    let mut polished = 0;
    let dense_only = std::env::var("SR_DENSE_LU").is_ok();
    for it in 0..40 {
        let c = cons(&y);
        if within(sys, &r, &ev, 4.0) && (c / wn).abs() < 1e-6 {
            return Some((y, ev, it));
        }
        let j = sys.jac(&y, q, &r);
        let nr = r.len();
        let wrow: Vec<f64> = w.iter().map(|x| wc * x).collect();
        // normal equations of [J; wc w] d = [r; wc c]: kept in the section system's own
        // structure where the slope matrix has it (see `NormalEq`), else as a dense matrix
        let structured = if dense_only { None } else { NormalEq::build(&j, &r, &wrow, wc * c, sys.nl()) };
        let mut nmat = vec![];
        let mut g = vec![];
        if structured.is_none() {
            // dense: summed row by row over each row's non-zero entries only
            nmat = vec![0.0; nu * nu];
            g = vec![0.0; nu];
            let mut nz: Vec<(usize, f64)> = Vec::with_capacity(nu);
            let mut add_row = |row: &[f64], rhs: f64, nz: &mut Vec<(usize, f64)>| {
                nz.clear();
                nz.extend(row.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(k, v)| (k, *v)));
                for &(a, va) in nz.iter() {
                    g[a] += va * rhs;
                    let base = a * nu;
                    for &(b, vb) in nz.iter() {
                        nmat[base + b] += va * vb;
                    }
                }
            };
            for rr in 0..nr {
                add_row(&j[rr * nu..(rr + 1) * nu], r[rr], &mut nz);
            }
            add_row(&wrow, wc * c, &mut nz);
        }
        let mut improved = false;
        for _ in 0..12 {
            let step = match &structured {
                Some(ne) => ne.solve(mu),
                None => {
                    let mut a = nmat.clone();
                    for k in 0..nu {
                        a[k * nu + k] += mu * (nmat[k * nu + k] + 1e-12);
                    }
                    let mut d = g.clone();
                    if lu_solve(&mut a, nu, &mut d) { Some(d) } else { None }
                }
            };
            let Some(d) = step else {
                mu *= 10.0;
                continue;
            };
            let yn: Vec<f64> = y.iter().zip(&d).map(|(a, b)| a - b).collect();
            let (rn, en) = sys.resid(&yn, q);
            let fnew = merit(&rn, cons(&yn));
            if fnew < f {
                y = yn;
                r = rn;
                ev = en;
                f = fnew;
                mu = (mu / 3.0).max(1e-9);
                improved = true;
                break;
            }
            mu *= 4.0;
        }
        if !improved && polished < 3 && std::env::var("SR_NO_POLISH").is_err() {
            // Stalled: each layer's transverse strain is a one-unknown equation of its own that
            // the layer solver finds exactly (bracketed root next to the present value), where
            // the least squares crawls when the layer's transverse response is nearly flat (top
            // of a compression zone near its peak, a web layer just past yield). Polish them
            // and carry on from there if that helps.
            polished += 1;
            if let Some((yp, rp, ep)) = polish_transverse(sys, &y, q) {
                let fp = merit(&rp, cons(&yp));
                if std::env::var("SR_PATH").is_ok() {
                    let worst = rp.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
                    eprintln!("   lm: polish merit {:.3e} -> {:.3e}, worst row {} {:.2}", f, fp, worst.0, worst.1);
                }
                if fp < f {
                    y = yp;
                    r = rp;
                    ev = ep;
                    f = fp;
                    mu = 1e-3;
                    continue;
                }
            }
        }
        if !improved {
            // balanced to tolerance and (almost) on the step's constraint: good enough
            // (8 tolerances = 0.08 % of the section's axial and moment scales, 0.04 MPa per
            // layer: the slope matrix is taken by perturbation and the crack check has rounded
            // corners, so the last few tolerances are sometimes out of reach)
            if within(sys, &r, &ev, 8.0) && (cons(&y) / wn).abs() < 1e-4 {
                return Some((y, ev, it));
            }
            if std::env::var("SR_PATH").is_ok() {
                let worst = r.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
                eprintln!("   lm: stuck at it {} merit {:.3e} worst row {} {:.3e}", it, f, worst.0, worst.1);
            }
            return None;
        }
    }
    if std::env::var("SR_PATH").is_ok() && !within(sys, &r, &ev, 4.0) {
        let worst = r.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
        let over = r.iter().filter(|x| x.abs() >= 4.0).count();
        eprintln!("   lm: 40 iterations, merit {:.3e} worst row {} {:.3e} ({} rows over 4, mu {:.1e})", f, worst.0, worst.1, over, mu);
    }
    // (Accepting the 8-tolerance band here too was tried, for runs crawling on the bars' yield
    // plateau: it moved them only a little further and shifted other runs onto other branches,
    // --check-mechanics 2.43 -> 2.59 %, so it stays at 4.)
    if !within(sys, &r, &ev, 4.0) && std::env::var("SR_NO_POLISH").is_err() {
        // out of iterations while crawling: the exact transverse strains may finish it
        let pol = polish_transverse(sys, &y, q);
        if std::env::var("SR_PATH").is_ok() {
            match &pol {
                Some((_, rp, _)) => {
                    let worst = rp.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
                    eprintln!("   lm: exhausted; polish gives worst row {} {:.2}", worst.0, worst.1);
                }
                None => eprintln!("   lm: exhausted; polish: a layer has no transverse root"),
            }
        }
        if let Some((yp, rp, ep)) = pol {
            if within(sys, &rp, &ep, 4.0) && (cons(&yp) / wn).abs() < 1e-6 {
                return Some((yp, ep, 40));
            }
        }
    }
    (within(sys, &r, &ev, 4.0)).then_some((y, ev, 40))
}

thread_local! {
    /// Set while curvature control past the bars' yield is running (see `within`).
    static REL_TOL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the scaled residuals `r` are all within `k` tolerances. A layer's transverse balance
/// is held to 0.5 % of its own principal compressive stress where that is looser than the fixed
/// tolerance (0.02 MPa per 4 tolerances): a compressed layer near its peak has an almost flat
/// transverse response, and the fixed tolerance, tiny beside 20-40 MPa of compression, stopped
/// runs on the yield plateau that were balanced to 0.03-0.04 MPa (0.1-0.3 %).
/// SR_ABS_TOL=1 keeps the fixed tolerance only.
fn within(sys: &Sys, r: &[f64], ev: &SecEval, k: f64) -> bool {
    static ABS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // only on curvature control past the bars' yield (REL_TOL set there): used everywhere it
    // let other runs settle on other branches earlier on (--check-mechanics 2.43 -> 2.53 %)
    let abs_only = *ABS.get_or_init(|| std::env::var("SR_ABS_TOL").is_ok()) || !REL_TOL.with(|c| c.get());
    let nl = sys.nl();
    r.iter().enumerate().all(|(j, x)| {
        let lim = if !abs_only && j >= 2 + nl && j < 2 + 2 * nl {
            let i = j - 2 - nl;
            let l = &sys.m.layers[i];
            let s2 = ev.outs.get(i).map(|o| o.s2.min(0.0).abs()).unwrap_or(0.0);
            let rel = 0.005 * s2 * l.b * l.t / sys.tol[j];
            k * (rel / 4.0).max(1.0)
        } else {
            k
        };
        x.abs() < lim
    })
}

/// Every layer's transverse strain solved exactly for its present longitudinal and shear strain
/// (`layer_solve`, the nearest root next to the present value), the other unknowns kept.
/// Returns the new unknowns with their residuals, or None if a layer has no root.
fn polish_transverse(sys: &Sys, y: &[f64], q: &[f64]) -> Option<(Vec<f64>, Vec<f64>, SecEval)> {
    let m = sys.m;
    let nl = sys.nl();
    let (ec, phi, g, ez, _) = sys.unpack(y);
    let rxs = m.long_reserve(ec, phi);
    let mut yp = y.to_vec();
    for i in 0..nl {
        let ex = m.strain(ec, phi, m.layers[i].z);
        let o = layer_solve(m, i, sys.sz, ex, g[i], ez[i], rxs.as_ref().map(|r| r[i]));
        if !o.ok {
            return None;
        }
        yp[2 + nl + i] = o.ez * sys.scale[2 + nl + i];
    }
    let (rp, ep) = sys.resid(&yp, q);
    Some((yp, rp, ep))
}

fn correct_newton(sys: &Sys, yk: &[f64], w: &[f64], target: f64, y0: &[f64], q: &[f64]) -> Option<(Vec<f64>, SecEval, usize)> {
    let nu = yk.len();
    let mut y = y0.to_vec();
    let cons = |y: &[f64]| -> f64 { w.iter().zip(y).zip(yk).map(|((a, b), c)| a * (b - c)).sum::<f64>() - target };
    let wn = norm2(w).max(1e-300);
    for it in 0..20 {
        let (r, ev) = sys.resid(&y, q);
        let c = cons(&y);
        // converged: every residual within 2 tolerances (0.01 MPa per layer, 0.02 % of the
        // section's axial and moment scales)
        if r.iter().all(|x| x.abs() < 4.0) && (c / wn).abs() < 1e-9 {
            return Some((y, ev, it));
        }
        let mut a = sys.jac(&y, q, &r);
        a.extend_from_slice(w);
        let mut b = r.clone();
        b.push(c);
        if !lu_solve(&mut a, nu, &mut b) {
            return None;
        }
        let merit = |r: &[f64]| norm2(r).powi(2);
        let m0 = merit(&r);
        let mut lam = 1.0;
        let mut moved = false;
        for _ in 0..12 {
            let yn: Vec<f64> = y.iter().zip(&b).map(|(a, d)| a - lam * d).collect();
            let (rn, _) = sys.resid(&yn, q);
            // the constraint is linear: any step along the Newton direction keeps it satisfied
            // up to (1 - lam) of its error, so the merit only measures equilibrium
            if merit(&rn) < m0 {
                y = yn;
                moved = true;
                break;
            }
            lam *= 0.5;
        }
        if !moved {
            if std::env::var("SR_PATH").is_ok() {
                let worst = r.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
                let yn: Vec<f64> = y.iter().zip(&b).map(|(a, d)| a - d).collect();
                let (rn, _) = sys.resid(&yn, q);
                let wn2 = rn.iter().enumerate().fold((0, 0.0f64), |w, (k, x)| if x.abs() > w.1 { (k, x.abs()) } else { w });
                eprintln!(
                    "   corr: line search failed at it {}: merit {:.3e} worst row {} {:.3e}; full step merit {:.3e} worst row {} {:.3e}",
                    it, m0, worst.0, worst.1, merit(&rn), wn2.0, wn2.1
                );
            }
            return None;
        }
    }
    let (r, ev) = sys.resid(&y, q);
    (r.iter().all(|x| x.abs() < 4.0)).then_some((y, ev, 20))
}

fn state_of(sys: &Sys, y: &[f64], ev: &SecEval, _n_kn: f64) -> SectionState {
    let m = sys.m;
    let (ec, phi, g, _ez, p) = sys.unpack(y);
    let (nn, mm, v) = sys.lo.at(p);
    let mut st = SectionState { ec, phi: phi * 1e6, n: nn / 1000.0, m: mm / 1e6, v: v / 1000.0, closed: true, ..Default::default() };
    let (mut gs, mut ws) = (0.0, 0.0);
    for (i, o) in ev.outs.iter().enumerate() {
        let l = &m.layers[i];
        let ex = m.strain(ec, phi, l.z);
        if o.tau.abs() > 1e-6 {
            gs += g[i].abs() * o.tau.abs();
            ws += o.tau.abs();
        }
        st.tau.push(o.tau.abs());
        st.layers.push(LayerState {
            z: l.z,
            ex,
            ez: o.ez,
            e1: o.e1,
            e2: o.e2,
            gamma: g[i],
            theta: o.theta,
            f1: o.f1,
            f2: (-o.s2).max(0.0),
            v: o.tau.abs(),
            sx: o.sx,
            fsz: o.fsz,
            w: o.w,
            cracked: o.e1 > m.conc.ecr,
        });
    }
    st.gamma_avg = if ws > 0.0 { gs / ws * 1000.0 } else { 0.0 };
    for (k, b) in m.bars.iter().enumerate() {
        st.bars.push((b.z, ev.bar_e[k], ev.bar_s[k]));
    }
    st
}

/// Converge the shear shares at a fixed constraint; returns the state and its shares.
fn solve_with_flow(sys: &Sys, yk: &[f64], w: &[f64], target: f64, y0: &[f64], q: &[f64]) -> Option<(Vec<f64>, SecEval, Vec<f64>)> {
    solve_with_flow_b(sys, yk, w, target, y0, q, false)
}

/// As `solve_with_flow`; with `rebranch` (load-controlled steps only), after every update of the
/// shares each layer is moved again to the nearest shear strain that carries its new share
/// (`branch_guess`), since a changed share can push a layer past its cracking peak, which the
/// corrector on its own can't jump.
fn solve_with_flow_b(sys: &Sys, yk: &[f64], w: &[f64], target: f64, y0: &[f64], q: &[f64], rebranch: bool) -> Option<(Vec<f64>, SecEval, Vec<f64>)> {
    let m = sys.m;
    let mut qq = q.to_vec();
    let mut y_try = y0.to_vec();
    // relaxation of the shares; on the branch path it is halved whenever the change grows
    // (cracking spreading layer to layer feeds back on the shares and overshoots at 0.6)
    let mut relax: f64 = 0.6;
    let mut last_delta = f64::INFINITY;
    // best pass so far (smallest change of the shares): the state is balanced exactly for the
    // shares it used
    let mut best: Option<(f64, Vec<f64>, SecEval, Vec<f64>)> = None;
    for _ in 0..(if rebranch { 40 } else { 20 }) {
        let attempt = if rebranch {
            let p = y_try[y_try.len() - 1] / sys.scale[y_try.len() - 1];
            correct_condensed(sys, yk, w, target, &y_try, &qq, p).or_else(|| correct(sys, yk, w, target, &y_try, &qq))
        } else {
            correct(sys, yk, w, target, &y_try, &qq)
        };
        let (y, ev, _) = match attempt {
            Some(x) => x,
            None => {
                if std::env::var("SR_CAND").is_ok() && rebranch {
                    eprintln!("   branch: corrector failed");
                }
                return None;
            }
        };
        let (ec, phi, g, ez, p) = sys.unpack(&y);
        let Some(qn) = flow_shape(m, sys.sz, ec, phi, &g, &ez, sys.lo.at(p).0) else { return Some((y, ev, qq)) };
        let delta: f64 = qn.iter().zip(&qq).map(|(a, b)| (a - b).abs()).sum();
        if std::env::var("SR_FLOW").is_ok() {
            eprintln!("   flow delta {:.5}", delta);
        }
        if delta < 3e-3 {
            return Some((y, ev, qn));
        }
        if rebranch && best.as_ref().map(|b| delta < b.0).unwrap_or(true) {
            best = Some((delta, y.clone(), ev, qq.clone()));
        }
        if rebranch {
            // halved when the change grows, raised again while it shrinks
            relax = if delta > last_delta { (relax * 0.5).max(0.05) } else { (relax * 1.3).min(0.6) };
        }
        last_delta = delta;
        qq = qq.iter().zip(&qn).map(|(a, b)| a + relax * (b - a)).collect();
        y_try = if rebranch {
            let p = y[y.len() - 1] / sys.scale[y.len() - 1];
            branch_guess(sys, &y, &qq, p).unwrap_or(y)
        } else {
            y
        };
    }
    // Layers on a cracking knife-edge can keep the shares cycling (a layer switching branch
    // moves several per cent of the shear about) with no exact fixed point. As the older solver
    // did ("up to 4 passes"), the shares then lag: the best pass is kept when its shares changed
    // by under 5 % of the shear (the older solver accepted 4 blended passes without any check);
    // the next step carries on converging them from there.
    if let Some((d, y, ev, qb)) = best.filter(|b| b.0 < 0.05) {
        if std::env::var("SR_CAND").is_ok() {
            eprintln!("   branch: shares cycling, kept the best pass (change {:.4})", d);
        }
        return Some((y, ev, qb));
    }
    if std::env::var("SR_CAND").is_ok() && rebranch {
        eprintln!("   branch: shares did not settle");
    }
    None
}

/// True when a run stopped without real failure evidence: too few states, or it could not
/// balance a higher load while the web was still barely deformed at the peak (largest layer
/// shear strain under 1 mm/m, i.e. stuck at the onset of cracking rather than failing).
pub fn stalled(r: &ShearResult) -> bool {
    if r.points.len() < 3 {
        return true;
    }
    let Some(pk) = r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap()) else { return true };
    let gmax = pk.layers.iter().fold(0.0f64, |a, l| a.max(l.gamma.abs()));
    r.reason.starts_with("No balanced state") && gmax < 1e-3 && pk.bars.iter().all(|b| b.1.abs() < 1.5e-3)
}

/// Whether a run ended with real evidence of failure at its peak (picked by the largest shear,
/// or by the largest moment when `by_moment`): a bar or stirrup fractured, concrete crushed, very
/// large shear strain, the shear fell past its peak under curvature control, or no higher load
/// balanced while the web was clearly working (largest shear strain 1 mm/m or more).
/// A run that ended with unambiguous failure: fracture, crushing, very large shear strain, or
/// the load falling well past its peak while the section kept deforming.
pub fn strong_proof(r: &ShearResult) -> bool {
    r.confirmed
        && r.points.len() >= 3
        && (r.reason.starts_with("Reinforcement fractured")
            || r.reason.starts_with("Concrete strain limit")
            || r.reason.starts_with("Very large shear strain")
            || r.reason.starts_with("Peak passed")
            || r.reason.starts_with("Shear capacity of the concrete"))
}

pub fn failure_proof(r: &ShearResult, by_moment: bool) -> bool {
    failure_proof_m(None, r, by_moment)
}

/// As `failure_proof`; with the model, "no balanced state at a higher load" only counts when
/// the reinforcement was working at the peak (a stirrup or a longitudinal bar at yield, or a
/// web shear strain of 4 mm/m or more). Stops just after diagonal cracking, with the stirrups
/// still elastic, were found to depend on the number of layers (sample beam: a ratio stalls at
/// 230-280 kN with one layer count and reaches 350-400 kN with another), so they are not
/// evidence of failure.
pub fn failure_proof_m(m: Option<&SModel>, r: &ShearResult, by_moment: bool) -> bool {
    if !r.confirmed || r.points.len() < 3 || r.reason.starts_with("The constant loads alone") {
        return false;
    }
    let pk = if by_moment {
        r.points.iter().max_by(|a, b| a.m.abs().partial_cmp(&b.m.abs()).unwrap())
    } else {
        r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap())
    };
    let Some(pk) = pk else { return false };
    let gmax = pk.layers.iter().fold(0.0f64, |a, l| a.max(l.gamma.abs()));
    let strong = r.reason.starts_with("Reinforcement fractured")
        || r.reason.starts_with("Concrete strain limit")
        || r.reason.starts_with("Very large shear strain")
        || r.reason.starts_with("Peak passed")
        || r.reason.starts_with("Shear capacity of the concrete");
    if strong {
        return true;
    }
    if !r.reason.starts_with("No balanced state") {
        return false;
    }
    let Some(m) = m else { return gmax >= 1e-3 };
    let stirrup_yield = m
        .stirrup
        .as_ref()
        .map(|(s, _)| {
            let ey = 0.98 * s.fy() / s.modulus();
            pk.layers.iter().zip(&m.layers).any(|(l, ml)| ml.rho_z > 0.0 && l.ez >= ey)
        })
        .unwrap_or(false);
    let bar_yield = pk.bars.iter().zip(&m.bars).any(|(b, mb)| b.1.abs() >= 0.98 * mb.steel.fy() / mb.steel.modulus());
    stirrup_yield || bar_yield || gmax >= 4e-3
}

/// A starting guess built layer by layer for the load unknown `p_new` (scaled): each layer's
/// shear strain is moved from where it is to the nearest one at which the layer carries its share
/// of the new shear (its transverse strain solved with it, the plane strains held). A layer that
/// can't carry its share on its present branch (past its cracking peak) is marched on through the
/// dip onto the branch where its stirrups pick the load up, which is where it really goes; so the
/// guess is on one consistent branch in every layer, and only the axial force and moment remain
/// to be balanced. The general "scaled" guesses mix branches between layers when many web layers
/// reach their cracking peak together, and then no corrector converges.
/// Returns None when some layer can't carry its share at any shear strain up to 25 mm/m.
fn branch_guess(sys: &Sys, yk: &[f64], q: &[f64], p_new: f64) -> Option<Vec<f64>> {
    let (ec, phi, _, _, _) = sys.unpack(yk);
    branch_layers(sys, yk, ec, phi, q, p_new)
}

/// The layers of `branch_guess` at a given plane strain (ec, phi): every layer's shear and
/// transverse strain are found from the anchors in `base` (nearest crossing of its share of the
/// shear at load `p_new`); the plane strains and the load are set in the returned unknowns.
fn branch_layers(sys: &Sys, base: &[f64], ec: f64, phi: f64, q: &[f64], p_new: f64) -> Option<Vec<f64>> {
    let m = sys.m;
    let nl = sys.nl();
    let iv = sys.nu() - 1;
    let (_, _, g, ez, _) = sys.unpack(base);
    let (_, _, v) = sys.lo.at(p_new);
    let rxs = m.long_reserve(ec, phi);
    let mut y0 = base.to_vec();
    y0[0] = ec * sys.scale[0];
    y0[1] = phi * sys.scale[1];
    y0[iv] = p_new * sys.scale[iv];
    let gmax = 0.025;
    for i in 0..nl {
        let l = &m.layers[i];
        let ar = l.b * l.t;
        let target = q[i] * v / ar; // MPa, signed
        if target.abs() < 1e-6 {
            continue;
        }
        let sgn = target.signum();
        let want = target.abs();
        let ex = m.strain(ec, phi, l.z);
        let rx = rxs.as_ref().map(|r| r[i]);
        let solve = |gg: f64, anchor: f64| layer_solve(m, i, sys.sz, ex, sgn * gg, anchor, rx);
        let g0 = (g[i] * sgn).max(0.0);
        let o0 = solve(g0, ez[i]);
        // march to the nearest crossing: upward while short of the target, downward while over it
        let up = o0.tau.abs() < want;
        let (mut ga, mut oa) = (g0, o0);
        let mut found: Option<(f64, LayerOut, f64, LayerOut)> = None;
        for _ in 0..400 {
            let gb = if up { ga * 1.04 + 2e-6 } else { (ga / 1.04 - 2e-6).max(0.0) };
            let ob = solve(gb, oa.ez);
            if !ob.ok {
                break;
            }
            if (ob.tau.abs() >= want) == up {
                found = Some((ga, oa, gb, ob));
                break;
            }
            if gb >= gmax || gb <= 0.0 {
                break;
            }
            ga = gb;
            oa = ob;
        }
        let (mut a, mut oa, mut b, mut ob) = match found {
            Some(x) => x,
            None if up => return None, // this layer can't carry its share on any branch
            None => continue,
        };
        // refine between the two strains that bracket the target
        for _ in 0..30 {
            let fa = oa.tau.abs() - want;
            let fb = ob.tau.abs() - want;
            let c = if (fb - fa).abs() > 1e-12 { a - fa * (b - a) / (fb - fa) } else { 0.5 * (a + b) };
            let c = if c.is_finite() && (c - a) * (c - b) < 0.0 { c } else { 0.5 * (a + b) };
            let oc = solve(c, oa.ez);
            if (oc.tau.abs() - want).abs() < 1e-5 * want.max(0.1) {
                a = c;
                oa = oc;
                break;
            }
            if (oc.tau.abs() >= want) == (ob.tau.abs() >= want) {
                b = c;
                ob = oc;
            } else {
                a = c;
                oa = oc;
            }
        }
        y0[2 + i] = sgn * a * sys.scale[2 + i];
        y0[2 + nl + i] = oa.ez * sys.scale[2 + nl + i];
    }
    Some(y0)
}

/// Condensed corrector for a load-controlled step at load `p` (scaled) with shares `q`: Newton on
/// the plane strain (ec, phi) alone, the axial force and moment being the only equations; at each
/// trial plane every layer is solved on its own (`branch_layers`, anchored at `base`), so a layer
/// stays on one branch however the plane moves. Ends with the full corrector from that state
/// (normally accepted at once). Layers only interact through the plane strain, so this two-unknown
/// problem is far better behaved than the whole system when many layers sit near their cracking
/// peak.
fn correct_condensed(sys: &Sys, yk: &[f64], w: &[f64], target: f64, base: &[f64], q: &[f64], p: f64) -> Option<(Vec<f64>, SecEval, usize)> {
    let (nn, mm, _) = sys.lo.at(p);
    let eval = |x: [f64; 2]| -> Option<([f64; 2], Vec<f64>)> {
        let (ec, phi) = (x[0] / sys.scale[0], x[1] / sys.scale[1]);
        let y = branch_layers(sys, base, ec, phi, q, p)?;
        let (ec2, phi2, g, ez, _) = sys.unpack(&y);
        let ev = section_direct(sys.m, sys.sz, ec2, phi2, &g, &ez);
        Some(([(ev.n - nn) / sys.tol[0], (ev.mm - mm) / sys.tol[1]], y))
    };
    // from the present plane first; then, as for flexural cracking (here: a flange in tension
    // cracking while the web takes its shear), the smallest curvature jump from which it balances
    let starts = [1.0, 1.3, 2.0, 3.0, 5.0, 8.0];
    for (k, f) in starts.iter().enumerate() {
        let x0 = [base[0], base[1] * f];
        if let Some(out) = condensed_from(sys, yk, w, target, base, q, x0, nn, mm, &eval) {
            if k > 0 && std::env::var("SR_CAND").is_ok() {
                eprintln!("   condensed: balanced after a curvature jump x{}", f);
            }
            return Some(out);
        }
        if base[1].abs() < 1e-9 {
            break; // no curvature to scale
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn condensed_from(
    sys: &Sys,
    yk: &[f64],
    w: &[f64],
    target: f64,
    _base: &[f64],
    q: &[f64],
    x0: [f64; 2],
    _nn: f64,
    _mm: f64,
    eval: &dyn Fn([f64; 2]) -> Option<([f64; 2], Vec<f64>)>,
) -> Option<(Vec<f64>, SecEval, usize)> {
    let mut x = x0;
    let (mut r, mut y) = eval(x)?;
    let nrm = |r: &[f64; 2]| r[0].hypot(r[1]);
    for it in 0..30 {
        if r[0].abs() < 2.0 && r[1].abs() < 2.0 {
            let out = correct(sys, yk, w, target, &y, q);
            if std::env::var("SR_CAND").is_ok() {
                let (rf, _) = sys.resid(&y, q);
                let worst = rf.iter().enumerate().fold((0, 0.0f64), |a, (k, x)| if x.abs() > a.1 { (k, x.abs()) } else { a });
                eprintln!(
                    "   condensed: balanced at it {}, full corrector {} (its residual there: worst row {} {:.2})",
                    it,
                    if out.is_some() { "accepts" } else { "refuses" },
                    worst.0,
                    worst.1
                );
            }
            return out;
        }
        let mut jm = [[0.0; 2]; 2];
        for c in 0..2 {
            let h = 1e-6 * (1.0 + x[c].abs());
            let mut xp = x;
            xp[c] += h;
            let (rp, _) = eval(xp)?;
            jm[0][c] = (rp[0] - r[0]) / h;
            jm[1][c] = (rp[1] - r[1]) / h;
        }
        let det = jm[0][0] * jm[1][1] - jm[0][1] * jm[1][0];
        if det.abs() < 1e-30 {
            return None;
        }
        let dx = [(r[0] * jm[1][1] - r[1] * jm[0][1]) / det, (jm[0][0] * r[1] - jm[1][0] * r[0]) / det];
        let mut lam = 1.0;
        let mut moved = false;
        for _ in 0..12 {
            let xn = [x[0] - lam * dx[0], x[1] - lam * dx[1]];
            if let Some((rn, yn)) = eval(xn) {
                if nrm(&rn) < nrm(&r) {
                    x = xn;
                    r = rn;
                    y = yn;
                    moved = true;
                    break;
                }
            }
            lam *= 0.5;
        }
        if !moved {
            if std::env::var("SR_CAND").is_ok() {
                eprintln!("   condensed: stuck at it {} with N {:.2} M {:.2} tolerances", it, r[0], r[1]);
            }
            // a layer switching branch makes the forces jump a little as the plane moves: within
            // the band the full corrector accepts, let it finish from here
            if r[0].abs() < 8.0 && r[1].abs() < 8.0 {
                return correct(sys, yk, w, target, &y, q);
            }
            return None;
        }
    }
    None
}

/// The layer (if any) that carries all the shear it can at this state: no larger shear stress
/// is found at any larger shear strain up to 25 mm/m (its present plane strain held). A layer with
/// stirrups reloads past its cracking dip, so this is mostly concrete outside the stirrups, for
/// example the top of the compression zone once the bars yield and the shear has moved up into
/// it. Returns (layer, its shear stress, the most it reaches).
fn layer_at_capacity(sys: &Sys, y: &[f64], ev: &SecEval) -> Option<(usize, f64, f64)> {
    let m = sys.m;
    let (ec, phi, g, _, _) = sys.unpack(y);
    let rxs = m.long_reserve(ec, phi);
    let mut worst: Option<(usize, f64, f64)> = None;
    for (i, o) in ev.outs.iter().enumerate() {
        let tau = o.tau.abs();
        if tau < 0.05 || g[i].abs() < 1e-9 {
            continue;
        }
        let ex = m.strain(ec, phi, m.layers[i].z);
        let sgn = g[i].signum();
        let mut gg = g[i].abs();
        let mut anchor = o.ez;
        let mut best = tau;
        while gg < 0.025 {
            gg = gg * 1.08 + 1e-6;
            let s = layer_solve(m, i, sys.sz, ex, sgn * gg, anchor, rxs.as_ref().map(|r| r[i]));
            if !s.ok {
                break;
            }
            anchor = s.ez;
            best = best.max(s.tau.abs());
        }
        if std::env::var("SR_CAP").is_ok() && best < 1.3 * tau {
            eprintln!("   capacity: layer {} z {:.0} rho_z {:.4} tau {:.3} at most {:.3} ({:.1} % reserve)", i, m.layers[i].z, m.layers[i].rho_z, tau, best, 100.0 * (best / tau - 1.0));
        }
        if best <= 1.02 * tau && worst.map(|w| tau > w.1).unwrap_or(true) {
            worst = Some((i, tau, best));
        }
    }
    worst
}

/// A "further deformed" starting guess from the state `yk` moving in direction `tk`
/// (family picks which deformations grow, by factor f).
fn jump_guess(sys: &Sys, yk: &[f64], tk: &[f64], tmax: f64, family: usize, f: f64) -> Vec<f64> {
    let m = sys.m;
    let nl = sys.nl();
    let mut y0 = yk.to_vec();
    match family {
        // a diagonal crack opening: the fastest-changing layers' shear strain grows and their
        // stirrups stretch with it (about 0.4 x the shear strain, a crack near 45 degrees), which
        // plain scaling can't give an uncracked layer whose stirrup strain is still close to zero.
        // 4: the crack alone; 5: the crack with the bending growing too (a flexure-shear crack);
        // 6: the whole web crossed by stirrups cracking together
        4 | 5 | 6 => {
            if family == 5 {
                y0[0] *= f.min(8.0);
                y0[1] *= f.min(8.0);
            }
            if family == 6 {
                y0[0] *= f.min(3.0);
                y0[1] *= f.min(3.0);
            }
            // a crack opening by delta normal to a crack at angle theta adds delta sin^2(theta) to
            // ex, delta cos^2(theta) to ez and 2 delta sin(theta) cos(theta) to gamma: the stirrup
            // strain grows by cot(theta) / 2 times the shear strain (0.5 at 45 degrees, about 2 for
            // the flat cracks of a heavily compressed web), so each layer's own angle is used
            let (ec0, phi0, g0, ez0, _) = sys.unpack(yk);
            for i in 0..nl {
                let k = 2 + i;
                let pick = if family == 6 { m.layers[i].rho_z > 0.0 } else { tk[k].abs() > 0.2 * tmax };
                if pick {
                    let ex = m.strain(ec0, phi0, m.layers[i].z);
                    let psi = 0.5 * g0[i].abs().atan2(ex - ez0[i]);
                    let theta = (std::f64::consts::FRAC_PI_2 - psi).clamp(0.12, std::f64::consts::FRAC_PI_2 - 0.05);
                    y0[k] *= f;
                    let dg = ((y0[k] / sys.scale[k]) - g0[i]).abs();
                    let kz = 2 + nl + i;
                    let ez_x = ez0[i] + 0.5 * dg / theta.tan();
                    y0[kz] = ez_x.max(ez0[i]) * sys.scale[kz];
                }
            }
        }
        // the fastest-changing layers deformed further
        0 => {
            for k in 2..2 + 2 * nl {
                if tk[k].abs() > 0.2 * tmax {
                    y0[k] *= f;
                }
            }
        }
        // bending only: flexural cracking or bar yield needs a curvature jump
        1 => {
            y0[0] *= f;
            y0[1] *= f;
        }
        // bending and the fastest-changing layers together
        2 => {
            y0[0] *= f;
            y0[1] *= f;
            for k in 2..2 + 2 * nl {
                if tk[k].abs() > 0.2 * tmax {
                    y0[k] *= f;
                }
            }
        }
        // the whole section
        _ => {
            for k in 0..2 + 2 * nl {
                y0[k] *= f;
            }
        }
    }
    y0
}

/// Load levels with a search for the shared branch. The shear is raised step by step; at each
/// level the balanced state is sought from the previous one and, when that fails (a sudden
/// load drop or a yield plateau), from "further deformed" guesses: the fastest-changing layers
/// or the whole section scaled up. Of the states found, the one whose deformation is most
/// evenly shared (smallest largest layer shear strain) is kept, so the load stays spread over
/// the web instead of one layer running away. The capacity is the highest level reached; it
/// is confirmed when no guess balances a slightly higher load.
fn load_levels(sys: &Sys, res: ShearResult, yk: Vec<f64>, tk: Vec<f64>, q: Vec<f64>, n_kn: f64) -> ShearResult {
    load_levels_cap(sys, res, yk, tk, q, n_kn, None).0
}

/// `cap`: stop when the load unknown reaches this value (scaled), used to apply the constant
/// loads before the increments; returns the result, the last state, its shear shares and
/// whether the cap was reached.
fn load_levels_cap(
    sys: &Sys,
    mut res: ShearResult,
    mut yk: Vec<f64>,
    mut tk: Vec<f64>,
    mut q: Vec<f64>,
    n_kn: f64,
    cap: Option<f64>,
) -> (ShearResult, Vec<f64>, Vec<f64>, bool) {
    let mut reached = false;
    let m = sys.m;
    let nl = sys.nl();
    let nu = sys.nu();
    let iv = nu - 1;
    let mut wv = vec![0.0; nu];
    wv[iv] = 1.0;
    let mut dv: f64 = 0.05; // in scaled V units (about 5 % of the shear scale)
    let (dv_min, dv_max) = (0.003, 0.12);
    let gmax_of = |y: &[f64]| -> f64 {
        let (_, _, g, _, _) = sys.unpack(y);
        g.iter().fold(0.0f64, |a, b| a.max(b.abs()))
    };
    // Under bending, once the web has cracked, a load step should deform the section: a balanced
    // state at a higher shear whose curvature and total web shear strain both grew by less than a
    // quarter of the load's own relative rise (or fell) is another root of the equations, not the
    // next state on the loading path. Taking it walks the run onto a branch that ends in a false
    // stop (a 600 x 600 column at M/V 0.219 m: V 634 -> 648 kN at a constant 3.61 rad/km, then
    // no continuation, against 690 kN at M/V 0.2195 m).
    // (only where the moment grows with the load: with the moment held, the curvature need not grow)
    let bending_run = sys.lo.dm.abs() > 50.0;
    let no_stagnant = std::env::var("SR_NO_STAGNANT").is_ok();
    let stagnant = |a: &[f64], b: &[f64]| -> bool {
        if no_stagnant || !bending_run || gmax_of(a) < 1e-3 {
            return false;
        }
        let (v0, v1) = (a[iv], b[iv]);
        if v0 <= 0.0 || v1 <= v0 {
            return false;
        }
        let rise = (v1 - v0) / v1;
        let (ec0, phi0, g0, _, _) = sys.unpack(a);
        let (_, phi1, g1, _, _) = sys.unpack(b);
        if phi1.abs() < 1e-12 {
            return false;
        }
        // only next to the tension bars' yield, where it was seen; just after diagonal cracking
        // the web settles for a few steps (curvature and shear strain easing back as the load
        // creeps up) and that is the loading path
        let near_yield = m.bars.iter().any(|b| {
            let e = m.strain(ec0, phi0, b.z) + b.pre;
            let tension_side = -(b.z - m.zc) * phi0.signum() > 0.0;
            tension_side && e > 0.9 * b.steel.fy() / b.steel.modulus()
        });
        if !near_yield {
            return false;
        }
        let dphi = (phi1 - phi0) * phi1.signum() / phi1.abs();
        let (s0, s1): (f64, f64) = (g0.iter().map(|x| x.abs()).sum(), g1.iter().map(|x| x.abs()).sum());
        let dg = (s1 - s0) / s1.max(1e-12);
        dphi < 0.25 * rise && dg < 0.25 * rise
    };
    let mut steps = 0;
    let mut drops = 0;
    let mut peak_state: Option<SectionState> = None;
    'levels: loop {
        steps += 1;
        if steps > 600 {
            res.reason = "Step limit reached.".into();
            break;
        }
        if let Some(c) = cap {
            let left = c - yk[iv];
            if left <= 1e-9 * c.abs().max(1.0) {
                reached = true;
                break;
            }
            dv = dv.min(left);
        }
        // predictor along the last direction, scaled to the load step
        let pred: Vec<f64> = if tk[iv] > 0.05 {
            yk.iter().zip(&tk).map(|(y, t)| y + dv / tk[iv] * t).collect()
        } else {
            let mut p = yk.clone();
            p[iv] += dv;
            p
        };
        let mut found = solve_with_flow(sys, &yk, &wv, dv, &pred, &q);
        // a large step that lands far away (one layer suddenly running away) is not trusted:
        // take a smaller step first, so the jump is only made where a small step can't balance
        if let Some((y, _, _)) = &found {
            let (g0, g1) = (gmax_of(&yk), gmax_of(y));
            if g1 > 2.0 * g0 + 0.002 && dv > 4.0 * dv_min {
                dv *= 0.5;
                continue;
            }
        }
        // a step that carries more load without deforming more is not on the loading path
        if found.as_ref().map(|c| stagnant(&yk, &c.0)).unwrap_or(false) {
            if dv > 4.0 * dv_min {
                dv *= 0.5;
                continue;
            }
            found = None;
        }
        if found.is_none() && dv > 4.0 * dv_min {
            // try a smaller step before looking for a jump
            dv *= 0.5;
            continue;
        }
        // Only for what it was built for: the web cracking while the shear rises, before any
        // longitudinal bar has yielded. On the bars' yield plateau the curvature-controlled
        // continuation below applies (the rescue there carried a constant-shear run on to strain
        // hardening, 354 kNm against the original's 328).
        let pre_yield = res
            .points
            .last()
            .map(|p| p.bars.iter().zip(&m.bars).all(|(b, mb)| b.1.abs() < 0.98 * mb.steel.fy() / mb.steel.modulus()))
            .unwrap_or(true);
        if found.is_none() && sys.lo.dv.abs() > 0.0 && pre_yield && std::env::var("SR_NO_BRANCH_GUESS").is_err() {
            // first: every layer moved to the nearest shear strain that carries its share of the
            // new load (through its cracking dip where it must), see `branch_guess`
            let p_new = (yk[iv] + dv) / sys.scale[iv];
            if let Some(y0) = branch_guess(sys, &yk, &q, p_new) {
                let got = solve_with_flow_b(sys, &yk, &wv, dv, &y0, &q, true);
                if std::env::var("SR_CAND").is_ok() {
                    eprintln!("   cand branch guess -> {}", got.as_ref().map(|c| format!("gmax {:.5}", gmax_of(&c.0))).unwrap_or("none".into()));
                }
                if let Some(c) = got.filter(|c| gmax_of(&c.0) <= 0.025 && !stagnant(&yk, &c.0)) {
                    res.rescued += 1;
                    found = Some(c);
                }
            }
        }
        if found.is_none() {
            // further-deformed guesses at the same load step
            let tmax = tk.iter().skip(2).take(2 * nl).fold(0.0f64, |a, b| a.max(b.abs())).max(1e-12);
            let mut cands: Vec<(Vec<f64>, SecEval, Vec<f64>)> = vec![];
            for family in [0, 4, 1, 5, 6, 2, 3] {
                for f in [1.3f64, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 40.0] {
                    if family == 1 && f > 12.0 {
                        break;
                    }
                    let mut y0 = jump_guess(sys, &yk, &tk, tmax, family, f);
                    y0[iv] += dv;
                    // shear shares that fit the guessed state (a cracked section shares its shear
                    // very differently from the uncracked one it jumps from)
                    let q0 = if family == 0 {
                        q.clone()
                    } else {
                        let (ec, phi, g, ez, p) = sys.unpack(&y0);
                        flow_shape(m, sys.sz, ec, phi, &g, &ez, sys.lo.at(p).0).unwrap_or_else(|| q.clone())
                    };
                    let got = solve_with_flow(sys, &yk, &wv, dv, &y0, &q0);
                    if std::env::var("SR_CAND").is_ok() {
                        eprintln!("   cand family {} x{} -> {}", family, f, got.as_ref().map(|c| format!("gmax {:.5}", gmax_of(&c.0))).unwrap_or("none".into()));
                    }
                    if let Some(c) = got {
                        if gmax_of(&c.0) > 0.025 || stagnant(&yk, &c.0) {
                            continue; // one layer run away: not the shared branch
                        }
                        cands.push(c);
                        if family == 1 {
                            break; // the smallest curvature jump that balances is the next state
                        }
                    }
                }
                if !cands.is_empty() {
                    break;
                }
            }
            if let Some(best) = cands.into_iter().min_by(|a, b| gmax_of(&a.0).partial_cmp(&gmax_of(&b.0)).unwrap()) {
                res.rescued += 1;
                found = Some(best);
            }
        }
        // A sudden load drop (a diagonal crack forming): no balanced state at a higher load, but
        // the section may carry less load in a cracked state and then reload as the stirrups
        // take over. The drop is followed by prescribing the strain of the layer that changes
        // fastest (the opening crack) instead of the load, step by step, until the load rises
        // above where it dropped; then load control resumes. The capacity stays the highest load
        // reached before or after. If the load never comes back, the peak was the true capacity.
        let mut dropped = false;
        if found.is_none() && dv * 0.5 < dv_min && drops < 3 && cap.is_none() {
            let v_before = yk[iv];
            // on the bars' yield plateau the moment can't rise until they harden: the curvature
            // is the quantity to drive then (as a curvature-controlled bending test does)
            // (only bars on the tension side of the bending count, and only when the moment
            // grows with the load: in pure shear the curvature isn't what drives the section)
            let (_, phi_now, _, _, _) = sys.unpack(&yk);
            let bending = sys.lo.dm.abs() > 50.0 || sys.lo.m0.abs() > 0.0;
            let yielded_now = bending
                && res.points.last().map(|p| {
                    p.bars.iter().zip(&m.bars).any(|(b, mb)| {
                        let tension_side = -(mb.z - m.zc) * phi_now.signum() > 0.0;
                        tension_side && b.1 > 0.98 * mb.steel.fy() / mb.steel.modulus()
                    })
                }).unwrap_or(false);
            // A stall under bending with the bars just short of yield (or with the stirrups
            // yielded and the strut carrying more) is the same situation: load control can't find
            // the next state on a nearly flat path, but curvature control can. When following
            // the drop gets nowhere, curvature control is tried too, so neighbouring ratios do
            // not end at different loads depending on which side of the yield strain they
            // stalled (a 600 x 600 column: 622-662 kN stops against 694-713 kN peaks).
            let modes: &[bool] = if yielded_now {
                &[true]
            } else if sys.lo.dm.abs() > 50.0 && std::env::var("SR_NO_CURV_RETRY").is_err() {
                &[false, true]
            } else {
                &[false]
            };
            for &yielded in modes {
                let (mut yd, mut td, mut qd) = (yk.clone(), tk.clone(), q.clone());
                let mut pts: Vec<SectionState> = vec![];
                let mut dd = 0.02;
                let mut back = false;
                let mut vmax_f = v_before;
                let mut curv_end: Option<&str> = None;
                for _ in 0..(if yielded { 400 } else { 120 }) {
                    let mut j = 2;
                    for k in 2..2 + 2 * nl {
                        if td[k].abs() > td[j].abs() {
                            j = k;
                        }
                    }
                    if yielded {
                        j = 1;
                    }
                    let mut w = vec![0.0; nu];
                    w[j] = 1.0;
                    // under curvature control the bending only grows: the curvature is driven the
                    // way it already points, even when the last load-controlled steps were
                    // turning back along the path (shear strain falling as the load rose)
                    let tgt = if yielded && phi_now != 0.0 && std::env::var("SR_CURV_TANGENT_SIGN").is_err() {
                        dd * phi_now.signum()
                    } else if td[j] < 0.0 {
                        -dd
                    } else {
                        dd
                    };
                    let pred: Vec<f64> = if td[j].abs() > 0.05 {
                        yd.iter().zip(&td).map(|(y, t)| y + tgt / td[j] * t).collect()
                    } else {
                        let mut p = yd.clone();
                        p[j] += tgt;
                        p
                    };
                    // distance along the path first (it can turn back through a snap-back), the
                    // crack's own strain second
                    let arc_pred: Vec<f64> = yd.iter().zip(&td).map(|(y, t)| y + dd * t).collect();
                    let got = if yielded {
                        REL_TOL.with(|c| c.set(true));
                        let g = solve_with_flow(sys, &yd, &w, tgt, &pred, &qd).or_else(|| solve_with_flow(sys, &yd, &td, dd, &arc_pred, &qd));
                        REL_TOL.with(|c| c.set(false));
                        g
                    } else {
                        solve_with_flow(sys, &yd, &td, dd, &arc_pred, &qd).or_else(|| solve_with_flow(sys, &yd, &w, tgt, &pred, &qd))
                    };
                    match got {
                        Some((y, ev, qn)) => {
                            let dy: Vec<f64> = y.iter().zip(&yd).map(|(a, b)| a - b).collect();
                            let n = norm2(&dy).max(1e-300);
                            td = dy.iter().map(|d| d / n).collect();
                            let st = state_of(sys, &y, &ev, n_kn);
                            if std::env::var("SR_PATH").is_ok() {
                                let bmax = ev.bar_e.iter().fold(0.0f64, |a, b| a.max(*b));
                                eprintln!("   drop: V {:.2} kN M {:.1} phi {:.2} rad/km bar {:.2} mm/m gmax {:.5} dd {:.4} (unknown {})", st.v, st.m, st.phi, bmax * 1e3, gmax_of(&y), dd, j);
                            }
                            let broken = m.bars.iter().zip(&ev.bar_e).any(|(b, e)| e.abs() >= b.steel.eu())
                                || gmax_of(&y) > 0.025
                                || st.layers.iter().any(|l| l.ex < -0.012);
                            yd = y;
                            qd = qn;
                            pts.push(st);
                            if broken {
                                curv_end = Some(if gmax_of(&yd) > 0.025 {
                                    "Very large shear strain reached (25 mm/m)."
                                } else if pts.last().map(|p| p.layers.iter().any(|l| l.ex < -0.012)).unwrap_or(false) {
                                    "Concrete strain limit reached."
                                } else {
                                    "Reinforcement fractured."
                                });
                                break;
                            }
                            vmax_f = vmax_f.max(yd[iv]);
                            if yielded {
                                // stay on curvature control past the plateau until the shear has
                                // clearly passed its peak
                                if yd[iv] < 0.85 * vmax_f {
                                    curv_end = Some("Peak passed under curvature control (the shear fell while the bending kept growing).");
                                    break;
                                }
                                dd = (dd * 1.3).min(0.5);
                                continue;
                            }
                            if yd[iv] > v_before {
                                back = true;
                                break;
                            }
                            dd = (dd * 1.3).min(if yielded { 0.5 } else { 0.06 });
                        }
                        None => {
                            if std::env::var("SR_PATH").is_ok() {
                                eprintln!("   drop: step dd {:.4} failed", dd);
                            }
                            dd *= 0.5;
                            if dd < 1e-3 {
                                break;
                            }
                        }
                    }
                }
                if yielded && !pts.is_empty() {
                    // the run ends on curvature control: its states join the path, and the
                    // capacity is the highest shear reached before or during it
                    if std::env::var("SR_PATH").is_ok() {
                        eprintln!("   curvature control past yield: {} states, top V {:.1} kN", pts.len(), vmax_f / sys.scale[iv] / 1000.0);
                    }
                    // a peak already passed: the shear fell steadily (3 or more states in a row,
                    // 0.5 % or more below the top) while the curvature kept growing
                    let falling = pts.windows(2).rev().take_while(|w| w[1].v < w[0].v).count();
                    let passed = falling >= 3
                        && yd[iv] < 0.995 * vmax_f
                        && std::env::var("SR_NO_SOFT_PEAK").is_err();
                    res.points.extend(pts);
                    peak_state = res.points.last().cloned();
                    res.rescued += 1;
                    if curv_end.is_none() && passed {
                        curv_end = Some("Peak passed under curvature control (the shear fell while the bending kept growing).");
                    }
                    match curv_end {
                        Some(t) => {
                            res.reason = t.into();
                            res.confirmed = true;
                        }
                        None => {
                            // no balance a little further on: a real failure if some layer already
                            // carries all the shear it can (no larger stress at any larger strain)
                            let (_, ev_end) = sys.resid(&yd, &qd);
                            match layer_at_capacity(sys, &yd, &ev_end) {
                                Some((i, tau, top)) => {
                                    if std::env::var("SR_PATH").is_ok() {
                                        eprintln!("   layer {} at z {:.0} carries {:.2} MPa, at most {:.2}: real failure", i, m.layers[i].z, tau, top);
                                    }
                                    res.reason = format!(
                                        "Shear capacity of the concrete at z = {:.0} mm reached{} past the bars' yield.",
                                        m.layers[i].z,
                                        if m.layers[i].rho_z > 0.0 { "" } else { " (outside the stirrups)" }
                                    );
                                    res.confirmed = true;
                                }
                                None => {
                                    res.reason = "Curvature control past yield could not be continued (solver stop).".into();
                                    res.confirmed = false;
                                }
                            }
                        }
                    }
                    break 'levels;
                }
                // even when the load hasn't come back yet, carry on from the most cracked state the
                // drop reached: load control and its jumps start afresh from there
                let adopt = !back && !pts.is_empty() && gmax_of(&yd) > 1.05 * gmax_of(&yk);
                if back || adopt {
                    if std::env::var("SR_PATH").is_ok() {
                        eprintln!("   load drop followed, reloading past {:.1} kN", pts.last().map(|p| p.v).unwrap_or(0.0));
                    }
                    let last = pts.pop();
                    res.points.extend(pts);
                    if let Some(st) = last {
                        res.points.push(st.clone());
                        peak_state = Some(st);
                    }
                    tk = td;
                    yk = yd;
                    q = qd;
                    drops += 1;
                    dropped = true;
                    res.rescued += 1;
                    dv = 0.02;
                    continue 'levels;
                }
            }
        }
        match found {
            None => {
                dv *= 0.5;
                if dv < dv_min {
                    res.reason = "No balanced state at a slightly higher load from any starting guess.".into();
                    res.confirmed = true;
                    break;
                }
            }
            Some((y, ev, qn)) => {
                let st = state_of(sys, &y, &ev, n_kn);
                if std::env::var("SR_PATH").is_ok() {
                    eprintln!("level {} dv {:.4} V {:.2} kN M {:.2} phi {:.3} gmax {:.6} gsum {:.5} bar {:.3}", steps, dv, st.v, st.m, st.phi, gmax_of(&y), st.layers.iter().map(|l| l.gamma.abs()).sum::<f64>(), ev.bar_e.iter().fold(0.0f64, |a, b| a.max(*b)) * 1e3);
                }
                let dy: Vec<f64> = y.iter().zip(&yk).map(|(a, b)| a - b).collect();
                let n = norm2(&dy).max(1e-300);
                tk = dy.iter().map(|d| d / n).collect();
                yk = y;
                q = qn;
                let fractured = m.bars.iter().zip(&ev.bar_e).any(|(b, e)| e.abs() >= b.steel.eu())
                    || st.layers.iter().zip(&m.layers).any(|(l, ml)| {
                        ml.rho_z > 0.0 && m.stirrup.as_ref().map(|s| l.ez >= s.0.eu()).unwrap_or(false)
                    });
                let crushed = st.layers.iter().any(|l| l.ex < -0.012);
                let huge = gmax_of(&yk) > 0.025;
                peak_state = Some(st.clone());
                res.points.push(st);
                if fractured || crushed || huge {
                    res.reason = if fractured {
                        "Reinforcement fractured.".into()
                    } else if crushed {
                        "Concrete strain limit reached.".into()
                    } else {
                        "Very large shear strain reached (25 mm/m).".into()
                    };
                    res.confirmed = true;
                    break;
                }
                dv = if dropped { 0.02 } else { (dv * 1.3).min(dv_max) };
            }
        }
    }
    // What governs: the work done on the section from just before the peak to the end of the run,
    // by the shear (layer shear stress x change of its shear strain) against the bending
    // (moment x change of curvature, plus axial force x change of centroid strain). One layer's
    // shear strain against the extreme fibre's strain, as compared before, called bending
    // failures web failures: once a web layer is cracked wide (ex 30 mm/m and more, near the
    // bending capacity) a tiny shear stress gives it a large shear strain that carries no load.
    // Crushing counts only an inclined strut (principal compression 10-80 deg from the axis),
    // not the flexural compression zone (about 0 deg, f2 = the axial stress).
    if let Some(pk) = peak_state {
        let n = res.points.len();
        let ipk = res
            .points
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.v.partial_cmp(&b.1.v).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(n.saturating_sub(1));
        let base = ipk.saturating_sub(1).min(n.saturating_sub(2));
        let prev = if n >= 2 { Some(&res.points[base]) } else { None };
        let mut web = false;
        let mut crush = false;
        if let Some(p) = prev {
            let w_shear: f64 = pk
                .layers
                .iter()
                .zip(&p.layers)
                .zip(&m.layers)
                .map(|((a, b), l)| 0.5 * (a.v.abs() + b.v.abs()) * (a.gamma - b.gamma).abs() * l.b * l.t)
                .sum();
            let w_bend = 0.5 * (pk.m.abs() + p.m.abs()) * (pk.phi - p.phi).abs()
                + 0.5 * (pk.n.abs() + p.n.abs()) * 1000.0 * (pk.ec - p.ec).abs();
            web = w_shear > w_bend;
            let inclined = |l: &LayerState| {
                let t = l.theta.to_degrees().abs();
                t > 10.0 && t < 80.0
            };
            if std::env::var("SR_MECH").is_ok() {
                eprintln!(
                    "   mech: shear work {:.1} N, bending work {:.1} N (states {} -> {} of {}), struts at their limit: {}",
                    w_shear,
                    w_bend,
                    base,
                    n - 1,
                    n,
                    pk.layers.iter().filter(|l| inclined(l) && l.f2 >= 0.85 * m.conc.peak(l.e1)).count()
                );
            }
            if web {
                crush = pk.layers.iter().any(|l| inclined(l) && l.f2 >= 0.85 * m.conc.peak(l.e1));
            }
        }
        let mech = if web {
            if crush { Mechanism::WebCrushing } else { Mechanism::WebShear }
        } else {
            let mut yielded = false;
            let mut hard = false;
            for (b, mb) in pk.bars.iter().zip(&m.bars) {
                if let SteelProps::Rebar { fy, es, esh, .. } = mb.steel {
                    yielded |= b.1 > 0.98 * fy / es;
                    hard |= b.1 > esh;
                }
            }
            if hard {
                Mechanism::LongHardening
            } else if yielded {
                Mechanism::LongYield
            } else {
                Mechanism::Flexure
            }
        };
        res.mech = Some(mech);
    }
    (res, yk, q, reached)
}

/// Trace the equilibrium path at moment/shear ratio `a` (m) up to and past the peak of V.
pub fn shear_path(m: &SModel, n_kn: f64, a: f64, mphi: &MPhiResult) -> ShearResult {
    shear_path_const(m, n_kn, a, 0.0, mphi)
}

/// As `shear_path`, with a constant moment `m0_kn` (kNm, analysis frame) applied first and held.
pub fn shear_path_const(m: &SModel, n_kn: f64, a: f64, m0_kn: f64, mphi: &MPhiResult) -> ShearResult {
    sectional_path(m, (n_kn, m0_kn, 0.0), (0.0, a, 1.0), mphi)
}

/// The original's "Constant + Increment" loading (analysis frame; kN, kNm, kN): the constant
/// loads are applied first (raised together from zero) and held, then the load factor on the
/// increments is raised until the section fails. Loads at any stage are constant + factor x
/// increment. With no increments it is a "one load" analysis: the section is solved at the
/// constant loads only.
pub fn sectional_path(m: &SModel, cst: (f64, f64, f64), inc: (f64, f64, f64), _mphi: &MPhiResult) -> ShearResult {
    let ratio = if inc.2.abs() > 1e-12 { inc.1 / inc.2 } else { 1000.0 };
    let mut res = ShearResult { mv: ratio, points: vec![], reason: String::new(), confirmed: false, rescued: 0, mech: None };
    let nl = m.layers.len();
    let sz = m.stirrup.as_ref().map(|s| s.1).unwrap_or(f64::INFINITY);
    let bw = m.layers.iter().filter(|l| l.b > 0.0).map(|l| l.b).fold(f64::MAX, f64::min);
    let vs = (0.25 * m.conc.fc.sqrt() * bw * 0.8 * m.h).max(1000.0); // N
    let es = 0.002;
    let mut scale = vec![1.0 / es, m.h / (2.0 * es)];
    for _ in 0..2 {
        for l in &m.layers {
            scale.push(((l.b * l.t) / m.area).sqrt().max(1e-6) / es);
        }
    }
    scale.push(1.0 / vs);
    let mut tol = vec![1e-4 * m.conc.fc * m.area, 1e-4 * m.conc.fc * m.area * m.h];
    tol.extend(std::iter::repeat(1e-4 * vs).take(2 * nl));
    let no_inc = inc.0.abs() < 1e-12 && inc.1.abs() < 1e-12 && inc.2.abs() < 1e-12;
    let has_const_mv = cst.1.abs() > 1e-9 || cst.2.abs() > 1e-9;
    // start: unloaded, only the constant axial force applied
    let ec0 = m.solve_ec(0.0, cst.0 * 1000.0).unwrap_or(0.0);
    let mut x0 = vec![ec0, 0.0];
    x0.extend(std::iter::repeat(0.0).take(2 * nl));
    x0.push(0.0);
    let mut yk: Vec<f64> = x0.iter().zip(&scale).map(|(a, b)| a * b).collect();
    let mut q = match flow_shape(m, sz, ec0, 0.0, &vec![0.0; nl], &vec![0.0; nl], cst.0 * 1000.0) {
        Some(q) => q,
        None => {
            res.reason = "No shear path could be started.".into();
            return res;
        }
    };
    let tangent = |sys: &Sys, yk: &[f64], q: &[f64]| -> Option<Vec<f64>> {
        let nu = sys.nu();
        let (r0, _) = sys.resid(yk, q);
        let mut aa = sys.jac(yk, q, &r0);
        let mut row = vec![0.0; nu];
        row[nu - 1] = 1.0;
        aa.extend_from_slice(&row);
        let mut b = vec![0.0; nu];
        b[nu - 1] = 1.0;
        if !lu_solve(&mut aa, nu, &mut b) {
            return None;
        }
        let n = norm2(&b);
        Some(b.iter().map(|x| x / n).collect())
    };
    // phase 1: the constant moment and shear, raised together from zero (the original solves
    // this "dead load" state first and warns when it can't: "Unable to converge on dead load
    // solution", "Negative Load Factor! Lower initial loads")
    if has_const_mv {
        let lo1 = Lo::new((cst.0, 0.0, 0.0), (0.0, cst.1, cst.2), m.h);
        let sys1 = Sys { m, sz, lo: lo1, scale: scale.clone(), tol: tol.clone() };
        let ev0 = section_direct(m, sz, ec0, 0.0, &vec![0.0; nl], &vec![0.0; nl]);
        res.points.push(state_of(&sys1, &yk, &ev0, cst.0));
        let Some(tk) = tangent(&sys1, &yk, &q) else {
            res.reason = "No shear path could be started.".into();
            return res;
        };
        // the constants are reached at load factor 1
        let cap = 1000.0 * lo1.g * scale[scale.len() - 1];
        let (r1, y1, q1, reached) = load_levels_cap(&sys1, res, yk, tk, q, cst.0, Some(cap));
        res = r1;
        if !reached {
            res.reason = format!(
                "The constant loads alone can't be carried (the section fails at {:.0} % of them): lower the constant loads. {}",
                100.0 * lo1.factor(y1[y1.len() - 1] / scale[scale.len() - 1]),
                res.reason
            );
            res.confirmed = true;
            return res;
        }
        yk = y1;
        q = q1;
        if no_inc {
            res.reason = "One load: the section is balanced at the constant loads.".into();
            res.confirmed = true;
            return res;
        }
        let k = yk.len() - 1;
        yk[k] = 0.0; // from here the load unknown counts the increments
    } else if no_inc {
        let lo = Lo::new(cst, inc, m.h);
        let sys = Sys { m, sz, lo, scale: scale.clone(), tol: tol.clone() };
        let ev = section_direct(m, sz, ec0, 0.0, &vec![0.0; nl], &vec![0.0; nl]);
        res.points.push(state_of(&sys, &yk, &ev, cst.0));
        res.reason = "One load: the section is balanced at the constant axial load.".into();
        res.confirmed = true;
        return res;
    }
    // phase 2: constant + load factor x increment
    let lo = Lo::new(cst, inc, m.h);
    let sys = Sys { m, sz, lo, scale, tol };
    let (_, ev) = sys.resid(&yk, &q);
    if !has_const_mv {
        res.points.push(state_of(&sys, &yk, &ev, cst.0));
    }
    let Some(tk) = tangent(&sys, &yk, &q) else {
        res.reason = "No shear path could be started.".into();
        return res;
    };
    let mut r = load_levels(&sys, res, yk.clone(), tk, q, cst.0);
    // the original reports the load factor on the increments at failure
    if let Some(pk) = r.points.iter().skip(if has_const_mv { 1 } else { 0 }).max_by(|a, b| a.v.partial_cmp(&b.v).unwrap().then(a.m.abs().partial_cmp(&b.m.abs()).unwrap())) {
        // recover the factor from the governing load component
        let f = if inc.2.abs() > 1e-12 {
            (pk.v - cst.2) / inc.2
        } else if inc.1.abs() > 1e-12 {
            (pk.m - cst.1) / inc.1
        } else {
            (pk.n - cst.0) / inc.0
        };
        r.reason = format!("{} Load factor at the peak: {:.2}.", r.reason, f);
    }
    r
}
