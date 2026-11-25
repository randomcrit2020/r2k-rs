//! ACI 318 design resistances from the nominal (MCFT) response: a rational approach.
//!
//! ACI 318 does not factor the materials as CSA A23.3 does. It multiplies a nominal strength by a
//! strength reduction factor phi. This module keeps the analysis nominal (unfactored concrete and
//! steel, the full MCFT response, so moment, shear and axial load interact as the mechanics say)
//! and turns each nominal interaction point into a design point:
//!
//! * flexure and axial load (ACI 318-19 21.2.2): phi from the net tensile strain et of the extreme
//!   tension steel at nominal strength, the extreme compression fibre at ecu = 0.003, plane
//!   sections at the point's own axial load. phi = 0.65 (ties) or 0.75 (spiral) up to et = ety,
//!   0.90 from et = ety + 0.003, linear between; ety = fy/Es (0.002 for prestressing steel);
//! * shear (21.2.1): phi_v = 0.75;
//! * axial compression is capped at phi Pn,max = phi alpha P0 (22.4.2.1), alpha 0.80 with ties,
//!   0.85 with spirals, P0 = 0.85 f'c (Ag - Ast - Apt) + fy Ast - (fse - 0.003 Ep) Apt (22.4.2.2).
//!
//! An M-V point becomes (phi(et) M, phi_v V): each action is reduced by its own factor, so a
//! demand (Mu, Vu) lies inside the design envelope exactly when (Mu / phi, Vu / phi_v) lies inside
//! the nominal one - the per-action check of ACI 318 (Mu <= phi Mn, Vu <= phi_v Vn) done on the
//! interacting M-V strength instead of separate Mn and Vn. At pure bending it gives phi Mn, at
//! pure shear phi_v Vn. M-N points are scaled radially by phi(et) (both coordinates, as ACI
//! interaction diagrams are drawn) and cut at phi Pn,max.

use crate::analysis::{self, SModel};
use crate::materials::SteelProps;
use crate::model::*;

/// The net tensile strain state at nominal strength for one axial load.
#[derive(Clone, Copy, Debug)]
pub struct StrainState {
    /// net tensile strain of the extreme tension steel (plain strain, tension +)
    pub et: f64,
    /// its yield strain
    pub ety: f64,
    /// the resulting phi
    pub phi: f64,
}

impl StrainState {
    pub fn class(&self, a: &AciFactors) -> &'static str {
        if self.et <= self.ety {
            "compression-controlled"
        } else if self.et >= self.ety + a.tc_extra {
            "tension-controlled"
        } else {
            "transition"
        }
    }
}

/// The two plane-sections models (positive moment: bottom in tension; negative: top) used to
/// find et, and the squash load.
pub struct AciModels {
    /// the factors as chosen (the cache key) and as they apply to this section
    pub chosen: AciFactors,
    pub a: AciFactors,
    pos: Option<SModel>,
    neg: Option<SModel>,
    /// P0 (N, compression positive)
    pub p0: f64,
}

impl AciModels {
    pub fn new(sec: &Section, a: &AciFactors) -> Self {
        let mut s = sec.clone();
        // nominal materials, whatever the material factors say
        s.phi = [1.0; 3];
        // ACI's et is a plane-sections strain: bars rupture only at their own eu here, not at a
        // crack (the sectional model's rupture rule would stop the strain search early)
        let plain = |m: Option<SModel>| {
            m.map(|mut m| {
                for b in m.bars.iter_mut() {
                    b.steel = b.steel.clone().with_rupture(f64::INFINITY);
                }
                m
            })
        };
        s.loads.negative = false;
        let pos = plain(analysis::build(&s, 80).ok());
        s.loads.negative = true;
        let neg = plain(analysis::build(&s, 80).ok());
        let p0 = pos.as_ref().map(squash_load).unwrap_or(0.0);
        AciModels { chosen: a.clone(), a: a.for_section(sec), pos, neg, p0 }
    }

    /// The strain state at nominal strength at axial load n (kN, tension +), for positive or
    /// negative moment.
    pub fn state(&self, n_kn: f64, negative: bool) -> StrainState {
        let m = if negative { self.neg.as_ref() } else { self.pos.as_ref() };
        let (et, ety) = m.and_then(|m| net_tensile_strain(m, n_kn * 1000.0, self.a.ecu)).unwrap_or((-self.a.ecu, 0.002));
        StrainState { et, ety, phi: self.a.phi_flexure(et, ety) }
    }

    /// Design maximum axial compression phi Pn,max (kN, negative = compression).
    pub fn n_cap(&self) -> f64 {
        -self.a.phi_compression() * self.a.pmax_alpha() * self.p0 / 1000.0
    }

    /// Design M-V envelope from the nominal one (M kNm, V kN) at constant axial load n.
    pub fn mv(&self, pts: &[(f64, f64)], n_kn: f64) -> Vec<(f64, f64)> {
        let (sp, sn) = (self.state(n_kn, false), self.state(n_kn, true));
        pts.iter().map(|&(m, v)| (if m < 0.0 { sn.phi } else { sp.phi } * m, self.a.phi_shear * v)).collect()
    }

    /// Design M-N diagram from the nominal one ((M kNm, N kN tension +) points). phi varies
    /// continuously with N, so the nominal polygon is first cut into short pieces (10 to a side):
    /// the transition from compression- to tension-control then shows as a smooth curve rather
    /// than a jump between two far-apart nominal points.
    pub fn mn(&self, pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let cap = self.n_cap();
        densify(pts, 10)
            .iter()
            .map(|&(m, n)| {
                let phi = self.state(n, m < 0.0).phi;
                (phi * m, (phi * n).max(cap))
            })
            .collect()
    }

    /// Design N-V diagram ((N kN, V kN) points) at a constant moment of the given sign.
    pub fn nv(&self, pts: &[(f64, f64)], negative: bool) -> Vec<(f64, f64)> {
        let cap = self.n_cap();
        densify(pts, 10)
            .iter()
            .map(|&(n, v)| {
                let phi = self.state(n, negative).phi;
                ((phi * n).max(cap), self.a.phi_shear * v)
            })
            .collect()
    }
}

/// The polyline with each segment cut into k equal pieces (straight lines between the nominal
/// points, as the plot draws them).
fn densify(pts: &[(f64, f64)], k: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(pts.len() * k + 1);
    for w in pts.windows(2) {
        for i in 0..k {
            let t = i as f64 / k as f64;
            out.push((w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1)));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

/// The extreme tension steel's strain when the extreme compression fibre (the model's top) is
/// at -ecu and the section carries axial load n (N): plane sections with the section's own
/// uniaxial materials, the curvature found by bisection. Returns (et, ety); a section fully in
/// compression at -ecu gives its bar strain there, one that can't reach n in tension a large et.
pub fn net_tensile_strain(m: &SModel, n: f64, ecu: f64) -> Option<(f64, f64)> {
    let bar = m.bars.iter().filter(|b| b.area > 0.0).min_by(|a, b| a.z.partial_cmp(&b.z).unwrap())?;
    let ety = match bar.steel {
        SteelProps::Prestress { .. } => 0.002,
        _ => bar.steel.fy() / bar.steel.modulus(),
    };
    let top = m.h - m.zc;
    let ec = |phi: f64| -ecu + phi * top;
    let g = |phi: f64| m.forces_uniaxial(ec(phi), phi).0 - n;
    // largest curvature tried: the extreme bar at 50 mm/m, or 90 % of the lowest rupture strain
    // (beyond it bars break, the force drops and the search would no longer be monotonic); a
    // section needing more is tension-controlled anyway
    let e_max = m.bars.iter().map(|b| 0.9 * b.steel.eu()).fold(0.05f64, f64::min).max(0.01);
    let (mut lo, mut hi) = (1e-9f64, (e_max + ecu) / (m.h - bar.z).max(1.0));
    if g(lo) >= 0.0 {
        // the whole section at about -ecu still gives less compression than n asks
        return Some((m.strain(ec(lo), lo, bar.z), ety));
    }
    if g(hi) <= 0.0 {
        return Some((m.strain(ec(hi), hi, bar.z), ety));
    }
    for _ in 0..80 {
        let mid = (lo * hi).sqrt();
        if g(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let phi = (lo * hi).sqrt();
    Some((m.strain(ec(phi), phi, bar.z), ety))
}

/// ACI 318-19 22.4.2.2: P0 = 0.85 f'c (Ag - Ast - Apt) + fy Ast - (fse - 0.003 Ep) Apt (N).
pub fn squash_load(m: &SModel) -> f64 {
    let steel_area: f64 = m.bars.iter().map(|b| b.area).sum();
    let mut p0 = 0.85 * m.conc.fc * (m.area - steel_area).max(0.0);
    for b in &m.bars {
        match b.steel {
            SteelProps::Prestress { .. } => {
                let ep = b.steel.modulus();
                let fse = ep * b.pre.max(0.0);
                p0 -= (fse - 0.003 * ep) * b.area;
            }
            _ => p0 += b.steel.fy() * b.area,
        }
    }
    p0
}

/// One line describing phi for a run at axial load n (kN), for both moment directions.
pub fn describe(models: &AciModels, n_kn: f64) -> String {
    let a = &models.a;
    let part = |s: StrainState| format!("φ = {:.3} (εt = {:.2} mm/m, {})", s.phi, s.et * 1000.0, s.class(a));
    format!(
        "{} at N = {:.0} kN: positive moment {}; negative moment {}; shear φv = {:.2}; φPn,max = {:.0} kN",
        a.name(),
        n_kn,
        part(models.state(n_kn, false)),
        part(models.state(n_kn, true)),
        a.phi_shear,
        -models.n_cap()
    )
}
