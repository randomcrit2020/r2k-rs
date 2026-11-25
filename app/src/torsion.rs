//! Torsion checks to ACI 318-19 (22.7) and AASHTO LRFD (9th ed., 5.7.2.1, 5.7.3.6) for solid
//! sections with closed ties (this program's addition; the original has no torsion). SI units:
//! N, mm, MPa; results in kN and kNm.
//!
//! Section properties: Acp and pcp from the outline (gross area and outside perimeter); Aoh and
//! ph from the centreline of the closed tie (a rectangular tie: 2 xs x its height, xs the
//! half width of its centreline as the drawing places it; a hoop: pi r^2 and 2 pi r; two
//! interlocking hoops: their union, estimated as two circles less the lens they share). At is one
//! leg of the tie (its area over its legs). Open ties and single legs give no torsion
//! reinforcement (Tn = 0).
//!
//! ACI 318-19 (normal-weight concrete, lambda = 1):
//! * threshold Tth = 0.083 sqrt(f'c) Acp^2 / pcp sqrt(1 + Nu / (0.33 Ag sqrt(f'c))) (Nu
//!   compression +, Table 22.7.4.1(a)); torsion may be neglected when Tu < phi Tth;
//! * Tn = 2 Ao At fyt cot(theta) / s, Ao = 0.85 Aoh, theta = 45 deg (37.5 for a prestressed
//!   section), fyt at most 420 MPa (22.7.6.1, 20.2.2.4);
//! * solid sections, shear and torsion together (22.7.7.1(a)):
//!   sqrt((Vu/(bw d))^2 + (Tu ph/(1.7 Aoh^2))^2) <= phi (Vc/(bw d) + 0.66 sqrt(f'c)), with
//!   Vc = (0.17 sqrt(f'c) + Nu/(6 Ag)) bw d (Table 22.5.5.1, with minimum shear reinforcement);
//! * the longitudinal steel torsion needs, Al = At/s ph (fyt/fy) cot^2(theta) (22.7.6.1),
//!   reported beside the steel the section has.
//!
//! AASHTO LRFD (normal-weight concrete):
//! * cracking torque Tcr = 0.126 K sqrt(f'c) Acp^2/pc in ksi and inches (5.7.2.1-4), here
//!   0.331 K sqrt(f'c) Acp^2/pc in MPa, K = sqrt(1 + fpc/(0.331 sqrt(f'c))) <= 2, fpc the axial
//!   compression stress Nu/Ag; torsion may be neglected when Tu <= 0.25 phi Tcr (5.7.2.1-3);
//! * Tn = 2 Ao At fy cot(theta) / s (5.7.3.6.2), Ao = 0.85 Aoh (C5.7.3.6.2), theta = 45 deg,
//!   fy at most 690 MPa;
//! * solid sections: the shear is designed for Vu,eff = sqrt(Vu^2 + (0.9 ph Tu / (2 Ao))^2)
//!   (5.7.2.1-6), here checked against phi_v Vn with Vn the program's own sectional shear
//!   capacity (MCFT, as AASHTO's general method) in the shear's direction.
//!
//! Both: the torsion check Tu / (phi_t Tn).

use crate::model::*;

#[derive(Clone, Debug, Default)]
pub struct TorsionProps {
    /// gross area and outside perimeter (mm2, mm)
    pub acp: f64,
    pub pcp: f64,
    /// area enclosed by the tie's centreline and its perimeter (mm2, mm)
    pub aoh: f64,
    pub ph: f64,
    /// one leg's area (mm2), spacing (mm), yield (MPa)
    pub at: f64,
    pub s: f64,
    pub fyt: f64,
    /// closed ties that resist torsion
    pub closed: bool,
    /// web width and effective depth for the ACI stress check (mm)
    pub bw: f64,
    pub d: f64,
    /// longitudinal steel area and its yield (mm2, MPa) and whether the section is prestressed
    pub al: f64,
    pub fy: f64,
    pub prestressed: bool,
    pub fc: f64,
}

pub fn props(sec: &Section) -> TorsionProps {
    let h = sec.height();
    let o = &sec.outline;
    let acp = sec.gross_area();
    // outside perimeter: bottom and top widths plus both sides along the width profile
    let mut pcp = o.first().map(|p| p.1).unwrap_or(0.0) + o.last().map(|p| p.1).unwrap_or(0.0);
    for w in o.windows(2) {
        pcp += 2.0 * (w[1].0 - w[0].0).hypot((w[1].1 - w[0].1) / 2.0);
    }
    let fc = sec.concretes.first().map(|c| c.fc).unwrap_or(30.0);
    let mut p = TorsionProps { acp, pcp, fc, ..Default::default() };
    if let Some(st) = &sec.stirrups {
        let zt = st.z_top.max(st.z_bot);
        let zb = st.z_top.min(st.z_bot);
        let db = st.db.max(1.0);
        p.s = st.spacing.max(1.0);
        p.fyt = sec.find_rebar(&st.steel).or_else(|| sec.rebars.first()).map(|r| r.fy).unwrap_or(400.0);
        p.at = st.area / stirrup_legs(st.pattern).max(1.0);
        match st.pattern {
            3 => {
                let mid = 0.5 * (zt + zb);
                let wmin = [zb + 1.0, mid, zt - 1.0].iter().map(|z| sec.width_at(*z)).fold(f64::INFINITY, f64::min);
                let mut c = (zb - db / 2.0).max(0.0);
                if wmin < 2.0 * c + 2.0 * db {
                    c = (wmin - 2.0 * db) / 3.0;
                }
                let xs = (wmin / 2.0 - c - db / 2.0).max(0.0);
                p.aoh = 2.0 * xs * (zt - zb);
                p.ph = 2.0 * (2.0 * xs + (zt - zb));
                p.closed = true;
            }
            4 => {
                let r = 0.5 * (zt - zb);
                p.aoh = std::f64::consts::PI * r * r;
                p.ph = 2.0 * std::f64::consts::PI * r;
                p.closed = true;
            }
            6 => {
                let cover = (zb - db / 2.0).max(0.0);
                let r = (0.5 * (zt - zb)).min(sec.width_at(0.5 * (zt + zb)) / 2.0 - cover - db / 2.0).max(1.0);
                let dc = ((zt - r) - (zb + r)).abs();
                // two circles of radius r with centres dc apart: union area and outer perimeter
                let (area, per) = if dc >= 2.0 * r {
                    (2.0 * std::f64::consts::PI * r * r, 4.0 * std::f64::consts::PI * r)
                } else {
                    let a = (dc / (2.0 * r)).acos();
                    let lens = 2.0 * r * r * a - 0.5 * dc * (4.0 * r * r - dc * dc).sqrt();
                    (2.0 * std::f64::consts::PI * r * r - lens, 2.0 * (2.0 * std::f64::consts::PI - 2.0 * a) * r)
                };
                p.aoh = area;
                p.ph = per;
                p.closed = true;
            }
            _ => {}
        }
    }
    // effective depth: compression face to the centroid of the bars in the lower half; web width
    // the narrowest width between the bars and the top (a circle: 0.8 D and D, ACI 22.5.2.2)
    let low: Vec<&LongLayer> = sec.long.iter().filter(|l| l.z < h / 2.0).collect();
    let a_low: f64 = low.iter().map(|l| l.area).sum();
    p.d = if matches!(sec.shape, Shape::Circle { .. }) {
        0.8 * h
    } else if a_low > 0.0 {
        h - low.iter().map(|l| l.area * l.z).sum::<f64>() / a_low
    } else {
        0.9 * h
    };
    p.bw = if let Shape::Circle { d } = sec.shape {
        d
    } else {
        (1..20).map(|i| sec.width_at(h * i as f64 / 20.0)).fold(f64::INFINITY, f64::min)
    };
    p.al = sec.long.iter().filter(|l| sec.find_rebar(&l.steel).is_some()).map(|l| l.area).sum();
    p.fy = sec.rebars.first().map(|r| r.fy).unwrap_or(400.0);
    p.prestressed = sec.long.iter().any(|l| sec.find_prestress(&l.steel).is_some());
    p
}

#[derive(Clone, Debug, Default)]
pub struct TorsionCheck {
    /// nominal torsional strength (kNm) and its design value
    pub tn: f64,
    pub phi_tn: f64,
    /// threshold (ACI) or a quarter of the cracking torque (AASHTO) times phi, below which
    /// torsion may be neglected (kNm)
    pub neglect_limit: f64,
    pub neglect: bool,
    /// Tu / (phi Tn)
    pub dc_t: f64,
    /// shear and torsion together: ACI's stress check, AASHTO's Vu,eff / (phi_v Vn)
    pub dc_vt: f64,
    /// longitudinal steel torsion needs (mm2), and the section's
    pub al_req: f64,
    pub al_have: f64,
    pub note: String,
}

/// The torsion check for a load case: Pu (kN, compression +), Vu (kN, resultant), Tu (kNm),
/// Vn the program's nominal shear capacity in the shear's direction (kN, for AASHTO).
pub fn check(p: &TorsionProps, a: &AciFactors, pu: f64, vu: f64, tu: f64, vn: f64) -> TorsionCheck {
    let tu = tu.abs() * 1e6; // N mm
    let vu = vu.abs() * 1e3; // N
    let nu = pu * 1e3; // N, compression +
    let sfc = p.fc.max(1.0).sqrt();
    let mut c = TorsionCheck { al_have: p.al, ..Default::default() };
    let phi_t = a.phi_torsion;
    let aoh = p.aoh.max(0.0);
    let ao = 0.85 * aoh;
    if a.code == 1 {
        // AASHTO LRFD
        let k = (1.0 + (nu / p.acp.max(1.0)).max(0.0) / (0.331 * sfc)).sqrt().min(2.0);
        let tcr = 0.331 * k * sfc * p.acp * p.acp / p.pcp.max(1.0);
        c.neglect_limit = 0.25 * phi_t * tcr / 1e6;
        let fy = p.fyt.min(690.0);
        c.tn = if p.closed { 2.0 * ao * p.at * fy / p.s / 1e6 } else { 0.0 };
        let veff = if p.closed && ao > 0.0 { (vu * vu + (0.9 * p.ph * tu / (2.0 * ao)).powi(2)).sqrt() } else { vu };
        c.dc_vt = if vn > 0.0 { veff / (a.phi_shear * vn * 1e3) } else { f64::INFINITY };
        c.al_req = if p.closed && p.s > 0.0 { p.at / p.s * p.ph * (p.fyt / p.fy.max(1.0)) } else { 0.0 };
        c.note = format!("Tcr = {:.1} kNm (K = {:.2}); Vu,eff = {:.1} kN", tcr / 1e6, k, veff / 1e3);
    } else {
        // ACI 318-19
        let tth = 0.083 * sfc * p.acp * p.acp / p.pcp.max(1.0) * (1.0 + nu / (0.33 * p.acp.max(1.0) * sfc)).max(0.0).sqrt();
        c.neglect_limit = phi_t * tth / 1e6;
        let fy = p.fyt.min(420.0);
        let cot = if p.prestressed { 1.0 / 37.5f64.to_radians().tan() } else { 1.0 };
        c.tn = if p.closed { 2.0 * ao * p.at * fy * cot / p.s / 1e6 } else { 0.0 };
        let bd = (p.bw * p.d).max(1.0);
        let vc = ((0.17 * sfc + nu / (6.0 * p.acp.max(1.0))).max(0.0) * bd).min(0.42 * sfc * bd);
        let lhs = if aoh > 0.0 { ((vu / bd).powi(2) + (tu * p.ph / (1.7 * aoh * aoh)).powi(2)).sqrt() } else { vu / bd };
        let rhs = a.phi_shear * (vc / bd + 0.66 * sfc);
        c.dc_vt = lhs / rhs;
        c.al_req = if p.closed && p.s > 0.0 { p.at / p.s * p.ph * (fy / p.fy.max(1.0)) * cot * cot } else { 0.0 };
        c.note = format!("Tth = {:.1} kNm; Vc = {:.1} kN (bw {:.0}, d {:.0} mm)", tth / 1e6, vc / 1e3, p.bw, p.d);
    }
    c.phi_tn = phi_t * c.tn;
    c.neglect = tu / 1e6 < c.neglect_limit;
    c.dc_t = if tu <= 0.0 {
        0.0
    } else if c.phi_tn > 0.0 {
        tu / 1e6 / c.phi_tn
    } else {
        f64::INFINITY
    };
    if !p.closed && tu > 0.0 {
        c.note.push_str("; no closed ties: no torsion reinforcement");
    }
    if c.neglect && tu > 0.0 {
        c.note.push_str("; below the threshold: torsion may be neglected");
    }
    c
}
