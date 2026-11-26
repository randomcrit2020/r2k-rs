//! Sectional analysis: moment-curvature, MCFT shear response, M-V interaction and a simple member check.
//! Units inside: mm, MPa, N, N*mm; results reported in kN, kNm, mm/m, rad/km.

use crate::materials::*;
use crate::model::*;
use std::f64::consts::PI;

#[derive(Clone, Debug)]
pub struct CLayer {
    pub z: f64,
    pub t: f64,
    pub b: f64,
    pub m_bond: f64,
    pub sx: f64,
    pub rho_z: f64,
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub z: f64,
    pub area: f64,
    pub steel: SteelProps,
    pub pre: f64,
    pub conc_area_removed: bool,
    /// designer's strain limit (plain strain): the rebar's own, or its strain-hardening start;
    /// for prestressing steel its rupture strain
    pub e_lim: f64,
    /// bar diameter (mm) and number of bars in the layer
    pub db: f64,
    pub num: f64,
}

#[derive(Clone, Debug)]
pub struct SModel {
    pub layers: Vec<CLayer>,
    pub bars: Vec<Bar>,
    pub conc: ConcreteProps,
    pub h: f64,
    pub zc: f64,
    pub area: f64,
    pub stirrup: Option<(SteelProps, f64)>,
    pub flip: bool,
    /// the section it was built from (to build it again with another number of layers)
    pub src: Section,
}

#[derive(Clone, Debug, Default)]
pub struct LayerState {
    pub z: f64,
    pub ex: f64,
    pub ez: f64,
    pub e1: f64,
    pub e2: f64,
    pub gamma: f64,
    pub theta: f64, // angle of principal compression from the member axis (rad)
    pub f1: f64,
    pub f2: f64, // compressive, positive magnitude
    pub v: f64,
    pub sx: f64,
    pub fsz: f64,
    pub w: f64,
    pub cracked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SectionState {
    pub layers: Vec<LayerState>,
    pub bars: Vec<(f64, f64, f64)>, // z, strain, stress
    pub n: f64,                      // kN
    pub m: f64,                      // kNm
    pub v: f64,                      // kN
    pub phi: f64,                    // rad/km
    pub ec: f64,
    pub gamma_avg: f64, // mm/m
    /// shear stress per layer the state was solved with (MPa)
    pub tau: Vec<f64>,
    /// the state needed shear sharing (a layer at its limit passed shear to others)
    pub shared: bool,
    /// the dual-section shear-flow closure converged (flow change under 2 %)
    pub closed: bool,
}

/// On a moment-curvature curve: the first points (curvature rad/km, moment kNm, interpolated)
/// where the concrete's most compressed fibre reaches the designer's limit ecu, and where a bar
/// reaches its limit (by default the start of strain hardening).
pub fn mphi_limits(m: &SModel, r: &MPhiResult) -> (Option<(f64, f64)>, Option<(f64, f64)>) {
    let conc = |p: &MPhiPoint| -> f64 {
        m.layers.iter().map(|l| -m.strain(p.ec, p.phi_raw, l.z)).fold(0.0, f64::max) / m.conc.ecu
    };
    let steel = |p: &MPhiPoint| -> f64 {
        m.bars.iter().map(|b| (m.strain(p.ec, p.phi_raw, b.z) + b.pre).abs() / b.e_lim.max(1e-6)).fold(0.0, f64::max)
    };
    let first = |f: &dyn Fn(&MPhiPoint) -> f64| -> Option<(f64, f64)> {
        for w in r.points.windows(2) {
            let (a, b) = (f(&w[0]), f(&w[1]));
            if a < 1.0 && b >= 1.0 {
                let t = ((1.0 - a) / (b - a)).clamp(0.0, 1.0);
                return Some((w[0].phi + t * (w[1].phi - w[0].phi), w[0].m + t * (w[1].m - w[0].m)));
            }
        }
        None
    };
    (first(&conc), first(&steel))
}

/// The layer model used for shear runs (SR_LAYERS overrides the layer count, for testing).
pub fn build_shear(sec: &Section) -> Result<SModel, String> {
    let n = std::env::var("SR_LAYERS").ok().and_then(|v| v.parse().ok()).unwrap_or(40usize);
    build(sec, n.clamp(8, 200))
}

pub fn build(sec: &Section, nlayers: usize) -> Result<SModel, String> {
    let h = sec.height();
    if h <= 0.0 || sec.outline.len() < 2 {
        return Err("The concrete section has no height. Define the section shape first.".into());
    }
    // material factors (the original's Options > General): a factored run scales every concrete
    // stress by phi_c and every steel stress by phi_s / phi_p, the steel held at fy
    let fac = is_factored(&sec.phi);
    let conc = ConcreteProps::new(sec.concretes.first().ok_or("No concrete defined.")?).factored(sec.phi[0]);
    let steel_f = |s: SteelProps| s.factored(sec.phi[1], sec.phi[2], fac);
    let flip = sec.loads.negative;
    let zmap = |z: f64| if flip { h - z } else { z };
    let mut layers = vec![];
    let t = h / nlayers as f64;
    let mut area = 0.0;
    let mut az = 0.0;
    for i in 0..nlayers {
        let z = (i as f64 + 0.5) * t;
        let b = sec.width_at(zmap(z));
        area += b * t;
        az += b * t * z;
        layers.push(CLayer { z, t, b, m_bond: 0.0, sx: 0.0, rho_z: 0.0 });
    }
    if area <= 0.0 {
        return Err("The concrete section has no area.".into());
    }
    let zc = az / area;
    let mut bars = vec![];
    for l in &sec.long {
        let steel = if let Some(r) = sec.find_rebar(&l.steel) {
            SteelProps::from_rebar(r)
        } else if let Some(p) = sec.find_prestress(&l.steel) {
            SteelProps::from_prestress(p)
        } else if let Some(r) = sec.rebars.first() {
            SteelProps::from_rebar(r)
        } else {
            return Err(format!("Layer '{}' uses an unknown steel '{}'.", l.name, l.steel));
        };
        let steel = steel_f(steel);
        let e_lim = match (sec.find_rebar(&l.steel).or_else(|| if sec.find_prestress(&l.steel).is_none() { sec.rebars.first() } else { None }), &steel) {
            (Some(r), _) => r.e_lim.unwrap_or(r.esh) / 1000.0,
            (None, s) => s.eu(),
        };
        bars.push(Bar { z: zmap(l.z), area: l.area, steel, pre: l.dep / 1000.0, conc_area_removed: true, e_lim, db: l.bar_dia(), num: l.num.max(1) as f64 });
    }
    // rupture at a crack (r2k 0x4aaf70 / 0x4ae520 / 0x4ac830): the strain at a crack is the
    // average concrete strain magnified, e_loc = x e_c + dep + (esh - fy/E), x = max(1,
    // k 0.014 / rho), k = 0.625 for a bar title starting with S or T (strands), 1 otherwise,
    // rho = the bar area on the bar's side of the centroid / (0.8 Ag) (0x4bc470; 2 when rho is
    // 0). A bar ruptures (its force drops out, 0x4201e0) when e_loc > eu and its average strain
    // has reached fy/E; a strand's esh - fy/E is about 0 (0x4ab4e0 sets esh = fy/(E + 1)). So
    // the bar's rupture strain, on its total strain e_c + dep, is
    // max((eu - dep - (esh - ey)) / x, ey - dep) + dep. Checked against the original's T-beam
    // with 4 strands only: peak at 388.9 kNm with the strands' concrete strain 8.87 mm/m, the
    // rule's 8.88 (shots/orig-pt-tee-monly.png). SR_NO_CRACK_RUPTURE=1 turns it off.
    if std::env::var("SR_NO_CRACK_RUPTURE").is_err() {
        let zc0 = {
            let mut a = 0.0;
            let mut az = 0.0;
            for i in 0..nlayers {
                let z = (i as f64 + 0.5) * t;
                let b = sec.width_at(zmap(z));
                a += b * t;
                az += b * t * z;
            }
            if a > 0.0 { az / a } else { h / 2.0 }
        };
        let a_up: f64 = bars.iter().filter(|b| b.z > zc0).map(|b| b.area).sum();
        let a_dn: f64 = bars.iter().filter(|b| b.z < zc0).map(|b| b.area).sum();
        let ag = 0.8 * area;
        for (b, l) in bars.iter_mut().zip(&sec.long) {
            let side = if b.z > zc0 { a_up } else if b.z < zc0 { a_dn } else { 0.5 * (a_up + a_dn) };
            let rho = side / ag;
            let k = if l.bartitle.trim().starts_with(['S', 'T', 's', 't']) { 0.625 } else { 1.0 };
            let x = if rho > 0.0 { (k * 0.014 / rho).max(1.0) } else { 2.0 };
            let ey = b.steel.fy() / b.steel.modulus();
            let hard = match b.steel {
                SteelProps::Rebar { esh, .. } => esh - ey,
                SteelProps::Prestress { .. } => 0.0,
            };
            let eu = b.steel.eu();
            let ec_r = ((eu - b.pre - hard) / x).max(ey - b.pre);
            let e_r = ec_r + b.pre;
            if e_r < eu {
                b.steel = b.steel.clone().with_rupture(e_r.max(ey));
            }
        }
    }
    // bond parameter and crack spacing for each layer (see MECHANICS.md: estimated)
    let bar_info: Vec<(f64, f64, f64)> = sec
        .long
        .iter()
        .map(|l| (zmap(l.z), l.bar_dia(), l.num.max(1) as f64))
        .collect();
    let mut bz: Vec<f64> = bar_info.iter().map(|b| b.0).collect();
    bz.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let bond = BondProfile::new(sec, h, &zmap);
    for ly in layers.iter_mut() {
        ly.m_bond = if std::env::var("SR_OLD_BOND").is_ok() {
            let mut best = (f64::INFINITY, 0.0, 1.0);
            for &(z, db, n) in &bar_info {
                let d = (z - ly.z).abs();
                if d < best.0 {
                    best = (d, db, n);
                }
            }
            if best.0.is_finite() {
                let (d, db, n) = best;
                let m = 15.0 * ly.b.max(1.0) / (n * PI);
                if d <= 7.5 * db.max(10.0) {
                    m.max(20.0)
                } else {
                    (m * 4.0).max(100.0)
                }
            } else {
                1000.0
            }
        } else {
            bond.m_at(ly.z)
        };
        // crack spacing: gap between the bar layers around this height
        let below = bz.iter().rev().find(|z| **z <= ly.z).copied();
        let above = bz.iter().find(|z| **z > ly.z).copied();
        let gap = match (below, above) {
            (Some(a), Some(b)) => b - a,
            (Some(a), None) => 2.0 * (ly.z - a),
            (None, Some(b)) => 2.0 * (b - ly.z),
            _ => 0.9 * h,
        };
        ly.sx = gap.clamp(100.0, (0.9 * h).max(100.0));
    }
    let stirrup = if let Some(st) = &sec.stirrups {
        let steel = sec
            .find_rebar(&st.steel)
            .or_else(|| sec.rebars.first())
            .map(SteelProps::from_rebar)
            .map(steel_f)
            .ok_or("Stirrups need a steel type.")?;
        let (zb, zt) = (zmap(st.z_bot).min(zmap(st.z_top)), zmap(st.z_bot).max(zmap(st.z_top)));
        for ly in layers.iter_mut() {
            if ly.z >= zb && ly.z <= zt && ly.b > 0.0 && st.spacing > 0.0 {
                ly.rho_z = st.area / (ly.b * st.spacing);
            }
        }
        Some((steel, st.spacing.max(1.0)))
    } else {
        None
    };
    Ok(SModel { layers, bars, conc, h, zc, area, stirrup, flip, src: sec.clone() })
}

/// The original's tension-stiffening bond profile (r2k, Bentz 1999 curve, type 7):
/// ft / (1 + sqrt(k e1)) with k = min(kx, kz) at the layer's depth (0x4a94d0).
///
/// * kx, "tension stiffening factor" (0x4fec60): each longitudinal layer contributes
///   r = pi db n / Ac, Ac the concrete within 7.5 db of the bars (cut at the section's faces),
///   spread as a triangle of peak 2.5 r over +-s, s the crack spacing at the bars (below);
///   kx = 3600 / (sum + 1e-4) with r in 1/m. 3600 x (1/r in m) is 3.6 M in mm: the usual
///   bond parameter M = Ac / sum(pi db), here taken 2.5 times tighter at the bars and fading to
///   nothing a crack spacing away from them.
/// * kz, "tension stiffening factor Z" (0x4ff1a0): where stirrups are, 3600 / (sum pi db n /
///   (b s) + 1e-4), so M = b s / (pi db n) of the stirrups.
/// * Both are sampled at 100 depths and read by straight lines between them, as the original
///   keeps them. Away from all bars and stirrups k is about 3.6e7: no tension stiffening.
///
/// The crack spacing at a depth (0x43faf0) is, over the nearest bar layer above and below,
/// min(sqrt(4 d^2 + sb^2)), sb the lateral spacing of the bars (0.1 m when a layer gives none,
/// 0x4fc640); 5 h when there are no bars, and never below the aggregate size (kept within
/// 2..25 mm).
pub struct BondProfile {
    ys: Vec<f64>,
    k: Vec<f64>,
    kz: Vec<f64>,
}

impl BondProfile {
    pub fn new(sec: &Section, h: f64, zmap: &dyn Fn(f64) -> f64) -> BondProfile {
        let sb = 100.0;
        let agg = sec.concretes.first().map(|c| c.agg).unwrap_or(19.0).clamp(2.0, 25.0);
        let bars: Vec<(f64, f64, f64)> = sec
            .long
            .iter()
            .map(|l| {
                let db = l.bar_dia().max(1.0);
                (zmap(l.z), db, 4.0 * l.area / db)
            })
            .collect();
        let width = |z: f64| sec.width_at(zmap(z.clamp(0.0, h))).max(0.0);
        let band = |lo: f64, hi: f64| {
            let n = 60;
            let dz = (hi - lo) / n as f64;
            (0..n).map(|i| width(lo + (i as f64 + 0.5) * dz) * dz).sum::<f64>()
        };
        let crack_x = |y: f64| -> f64 {
            let y = y.clamp(0.0, h);
            let mut above: Option<f64> = None;
            let mut below: Option<f64> = None;
            for &(z, _, _) in &bars {
                if z >= y && z < h && above.map(|a| z - y < a).unwrap_or(true) {
                    above = Some(z - y);
                }
                if z <= y && z > 0.0 && below.map(|b| y - z < b).unwrap_or(true) {
                    below = Some(y - z);
                }
            }
            let f = |d: f64| (4.0 * d * d + sb * sb).sqrt();
            let s = match (above, below) {
                (None, None) => 5.0 * h,
                (a, b) => a.map(f).unwrap_or(f64::INFINITY).min(b.map(f).unwrap_or(f64::INFINITY)),
            };
            s.min(5.0 * h).max(agg)
        };
        // each bar layer's triangle: centre, half width, peak (1/mm)
        let tri: Vec<(f64, f64, f64)> = bars
            .iter()
            .map(|&(z, db, per)| {
                let lo = (z - 7.5 * db).max(0.0);
                let hi = (z + 7.5 * db).min(h);
                let ac = band(lo, hi).max(1e-6);
                let r = per / ac;
                (z, crack_x(z), 2.5 * r)
            })
            .collect();
        let st = sec.stirrups.as_ref().filter(|s| s.spacing > 0.0 && s.area > 0.0).map(|s| {
            let db = if s.db > 0.0 { s.db } else { (4.0 * s.area / (2.0 * PI)).sqrt() };
            let (a, b) = (zmap(s.z_bot), zmap(s.z_top));
            (a.min(b), a.max(b), 4.0 * s.area / db, s.spacing)
        });
        let mut ys = vec![];
        let mut k = vec![];
        let mut kzs = vec![];
        for i in 0..100 {
            let y = h * i as f64 / 99.0;
            let sx: f64 = tri
                .iter()
                .map(|&(z, half, peak)| {
                    let d = (y - (z - half)).min(z + half - y).max(0.0);
                    d * peak / half.max(1e-3)
                })
                .sum();
            let kx = 3600.0 / (sx * 1000.0 + 1e-4);
            let kz = match st {
                Some((zb, zt, per, s)) if y >= zb && y <= zt => {
                    // b s in m2 plus the original's 1e-4 m2
                    let sz = per / 1000.0 / (width(y) * s * 1e-6 + 1e-4);
                    3600.0 / (sz + 1e-4)
                }
                _ => 3600.0 / 1e-4,
            };
            ys.push(y);
            k.push(kx);
            kzs.push(kz);
        }
        BondProfile { ys, k, kz: kzs }
    }

    fn interp(&self, v: &[f64], z: f64) -> f64 {
        let n = self.ys.len();
        let z = z.clamp(self.ys[0], self.ys[n - 1]);
        let mut i = 0;
        while i + 2 < n && self.ys[i + 1] < z {
            i += 1;
        }
        let (y0, y1) = (self.ys[i], self.ys[(i + 1).min(n - 1)]);
        let t = if y1 > y0 { (z - y0) / (y1 - y0) } else { 0.0 };
        v[i] + t * (v[(i + 1).min(n - 1)] - v[i])
    }

    /// The bond parameter M in mm (k / 3.6) at height z.
    pub fn m_at(&self, z: f64) -> f64 {
        if self.ys.is_empty() {
            return 1e7;
        }
        self.interp(&self.k, z).min(self.interp(&self.kz, z)) / 3.6
    }
}

static NO_SPACING_FACTOR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static NO_LONG_RESERVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

impl SModel {
    /// The original's factor on the stirrups' reserve at a crack (0x4a80e0): k = clamp(0.5 (3 -
    /// s/h), 0, 1) with s the stirrup spacing and h the section depth - full help up to s = h,
    /// none at s = 3 h (SR_NO_SPACING_FACTOR=1 turns it off).
    pub fn stirrup_reserve_factor(&self) -> f64 {
        match &self.stirrup {
            Some((_, s)) if self.h > 0.0 && !*NO_SPACING_FACTOR.get_or_init(|| std::env::var("SR_NO_SPACING_FACTOR").is_ok()) => {
                (0.5 * (3.0 - s / self.h)).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// The longitudinal bars' reserve at a crack, per concrete layer (MPa), for the plane-strain
    /// state (ec, phi): the original's profile at layer +0x104 (built by 0x441d80 and 0x440400,
    /// used by the crack check 0x46aaa0 as f1 <= rx + vci / tan theta).
    ///
    /// * Crack fronts: where the axial strain changes sign, scanning in from each face that is in
    ///   tension; a face in tension all the way through puts its front 1000 m away (0x531af8,
    ///   0x52e660), which makes the result a uniform average.
    /// * For each bar layer in a cracked zone: its spare force at the crack A (fmax - fs), times
    ///   its distance from the zone's front, times min(distance to the nearest front / db, 1),
    ///   plus 0.1 N m (0x52f048) - a moment about the front. fmax (0x4ae520): the stress at the
    ///   crack's local strain x e + (esh - ey) (0x4ac830; x = max(1, 0.014/rho) (0x53b180),
    ///   rho = A / (0.07 m x lateral spacing), 0x4fc340 / 0x5337a0), never below fy; no reserve
    ///   once that strain reaches rupture.
    /// * That moment is carried by a tension stress over the cracked zone (0x440400): uniform up
    ///   to 0.6 ft (0x52f058); then rising linearly from 0.6 ft at the face to at most 3 ft at
    ///   the front (0x52e030); then the face value rising. Between the fronts the profile is
    ///   3 ft. ft is the concrete's cracking strength (material +0x18, read as the tension
    ///   curve's numerator in 0x4ad9c0).
    ///
    /// Off unless SR_LONG_RESERVE=1: measured against the original's own runs it made the match
    /// worse (pure tension 824 kN against the original's 886 kN; M/V 1.5 m -5 % to -11 %), so a
    /// part of it is still misread - most likely the strain at the crack (x stays 1 for the
    /// sample beam's bars, leaving no reserve at yield, while the original's 886 kN needs one).
    pub fn long_reserve(&self, ec: f64, phi: f64) -> Option<Vec<f64>> {
        // on in a factored run: there the steel is held at fy, so once the bars yield they have
        // no reserve at a crack and the concrete between cracks can carry no tension - checked
        // against the original's own factored run of the sample beam (CSA factors: M-phi peak
        // 258.2 kNm in the original, 258.1 here with it, 270.6 without)
        if *NO_LONG_RESERVE.get_or_init(|| std::env::var("SR_LONG_RESERVE").is_err())
            && (!is_factored(&self.src.phi) || std::env::var("SR_NO_FACTORED_RESERVE").is_ok())
        {
            return None;
        }
        let ft = self.conc.ft;
        let h = self.h;
        let ex = |z: f64| self.strain(ec, phi, z);
        let far = 1.0e6;
        // fronts: tension zone above front_top, below front_bot
        let (t_top, t_bot) = (ex(h) > 0.0, ex(0.0) > 0.0);
        let z0 = if phi.abs() > 1e-15 { self.zc + ec / phi } else { f64::NAN };
        let inside = z0.is_finite() && z0 > 0.0 && z0 < h;
        let front_top = if !t_top { h } else if inside { z0 } else { -far };
        let front_bot = if !t_bot { 0.0 } else if inside { z0 } else { far };
        let whole = t_top && t_bot;
        // the bar ratio for the strain at a crack, as the original sets it for beam sections
        // (0x4bc470): the bar area on the bar's side of the centre (half of a bar on it to each
        // side) over 0.8 Ag (0x52f068; Ag at +0x816, read by 0x4bb270)
        let (mut a_up, mut a_dn) = (0.0f64, 0.0f64);
        for b in &self.bars {
            if b.z > self.zc {
                a_up += b.area;
            } else if b.z < self.zc {
                a_dn += b.area;
            } else {
                a_up += 0.5 * b.area;
                a_dn += 0.5 * b.area;
            }
        }
        let rho_side = |z: f64| {
            let ag = 0.8 * self.area.max(1e-9);
            if z > self.zc {
                a_up / ag
            } else if z < self.zc {
                a_dn / ag
            } else {
                0.5 * (a_up + a_dn) / ag
            }
        };
        // the bars' moments of spare force about each front
        let (mut m_top, mut m_bot) = (0.0f64, 0.0f64);
        for b in &self.bars {
            let in_top = b.z > front_top;
            let in_bot = b.z < front_bot;
            if !in_top && !in_bot {
                continue;
            }
            let e = ex(b.z) + b.pre;
            let fs = b.steel.stress(e);
            let reserve = match b.steel {
                SteelProps::Rebar { fy, es, esh, eu, .. } => {
                    let rho = rho_side(b.z);
                    let x = if rho > 0.0 { (0.014 / rho).max(1.0) } else { 2.0 };
                    let eloc = x * e + (esh - fy / es);
                    if eloc >= eu {
                        0.0
                    } else {
                        (b.steel.stress(eloc).max(b.steel.fy()) - fs).max(0.0)
                    }
                }
                SteelProps::Prestress { eu, .. } => {
                    let rho = rho_side(b.z);
                    let x = if rho > 0.0 { (0.625 * 0.014 / rho).max(1.0) } else { 2.0 };
                    let eloc = x * e;
                    if eloc >= eu {
                        0.0
                    } else {
                        (b.steel.stress(eloc).max(b.steel.fy()) - fs).max(0.0)
                    }
                }
            };
            let near = (b.z - front_bot).abs().min((b.z - front_top).abs());
            let k = (near / (b.db + 0.1)).min(1.0);
            let df = b.area * reserve * k;
            if in_top {
                m_top += (b.z - front_top).abs() * df + 100.0;
            }
            if in_bot {
                m_bot += (front_bot - b.z).abs() * df + 100.0;
            }
        }
        // stress over a cracked zone from its front to its face: (front value, face value)
        let zone = |front: f64, face: f64, mom: f64| -> (f64, f64) {
            let d = (face - front).abs();
            if d < 0.1 {
                return (3.0 * ft, 3.0 * ft);
            }
            let (mut i0, mut i1, mut i2) = (0.0, 0.0, 0.0);
            for l in &self.layers {
                let inz = if face > front { l.z > front } else { l.z < front };
                if !inz {
                    continue;
                }
                let t = (l.z - front).abs();
                let w = l.b * l.t * t;
                i0 += w;
                i1 += w * (1.0 - t / d);
                i2 += w * (t / d);
            }
            let l40 = 0.6 * ft * i0;
            let l38 = 2.4 * ft * i1;
            if mom <= l40 {
                let s = if i0 > 0.0 { mom / i0 } else { 0.0 };
                (s, s)
            } else if mom <= l40 + l38 {
                (0.6 * ft + if i1 > 0.0 { (mom - l40) / i1 } else { 0.0 }, 0.6 * ft)
            } else {
                (3.0 * ft, 0.6 * ft + if i2 > 0.0 { (mom - l40 - l38) / i2 } else { 0.0 })
            }
        };
        let (ft_front, ft_face) = zone(front_top, h, m_top);
        let (fb_front, fb_face) = zone(front_bot, 0.0, m_bot);
        Some(
            self.layers
                .iter()
                .map(|l| {
                    if whole {
                        fb_face + (ft_face - fb_face) * (l.z / h.max(1e-9))
                    } else if l.z > front_top {
                        let d = (h - front_top).max(1e-9);
                        ft_front + (ft_face - ft_front) * ((l.z - front_top) / d)
                    } else if l.z < front_bot {
                        let d = front_bot.max(1e-9);
                        fb_front + (fb_face - fb_front) * ((front_bot - l.z) / d)
                    } else {
                        3.0 * ft
                    }
                })
                .collect(),
        )
    }

    pub fn strain(&self, ec: f64, phi: f64, z: f64) -> f64 {
        ec - phi * (z - self.zc)
    }

    /// Uniaxial concrete with the crack check on the longitudinal bars' side: once cracked, the
    /// tension carried across the cracks is at most the bars' reserve rx (no shear, so the shear
    /// on the crack adds nothing).
    pub fn uniaxial_capped(&self, e: f64, m_bond: f64, rx: Option<f64>) -> f64 {
        let s = self.conc.uniaxial(e, m_bond);
        match rx {
            Some(rx) if e > self.conc.ecr && s > 0.0 => s.min(rx.max(0.0)),
            _ => s,
        }
    }

    /// Plain-sections forces with uniaxial concrete (N, N*mm).
    pub fn forces_uniaxial(&self, ec: f64, phi: f64) -> (f64, f64) {
        let mut n = 0.0;
        let mut m = 0.0;
        let rxs = self.long_reserve(ec, phi);
        for (i, l) in self.layers.iter().enumerate() {
            let e = self.strain(ec, phi, l.z);
            let f = self.uniaxial_capped(e, l.m_bond, rxs.as_ref().map(|v| v[i])) * l.b * l.t;
            n += f;
            m -= f * (l.z - self.zc);
        }
        for b in &self.bars {
            let e = self.strain(ec, phi, b.z);
            let mut f = b.steel.stress(e + b.pre) * b.area;
            if b.conc_area_removed {
                f -= self.conc.uniaxial(e, 100.0).min(0.0) * b.area;
            }
            n += f;
            m -= f * (b.z - self.zc);
        }
        (n, m)
    }

    pub fn solve_ec(&self, phi: f64, n_target: f64) -> Option<f64> {
        let span = phi.abs() * self.h;
        let mut lo = -0.005 - span;
        let mut hi = 0.02 + span;
        let f = |ec: f64| self.forces_uniaxial(ec, phi).0 - n_target;
        let mut flo = f(lo);
        let mut fhi = f(hi);
        let mut k = 0;
        if n_target < 0.0 {
            // under compression the force is not monotonic in the strain (the concrete softens past
            // its peak), so step down from the tension side for the first strain that carries the
            // load: the bracket then holds the root on the rising branch, never the softening one
            for j in 1..=400 {
                let e = 0.02 + span - (0.025 + 2.0 * span) * j as f64 / 400.0;
                let fe = f(e);
                if fe <= 0.0 {
                    lo = e;
                    flo = fe;
                    hi = hi.min(e + (0.025 + 2.0 * span) / 400.0);
                    fhi = f(hi);
                    break;
                }
            }
        }
        while flo > 0.0 && k < 20 {
            lo -= 0.005;
            flo = f(lo);
            k += 1;
        }
        while fhi < 0.0 && k < 40 {
            hi += 0.01;
            fhi = f(hi);
            k += 1;
        }
        if flo > 0.0 || fhi < 0.0 {
            return None;
        }
        for _ in 0..70 {
            let mid = 0.5 * (lo + hi);
            if f(mid) < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }

    pub fn uniaxial_state(&self, ec: f64, phi: f64) -> SectionState {
        let mut st = SectionState { ec, phi: phi * 1e6, ..Default::default() };
        let rxs = self.long_reserve(ec, phi);
        for (i, l) in self.layers.iter().enumerate() {
            let e = self.strain(ec, phi, l.z);
            let s = self.uniaxial_capped(e, l.m_bond, rxs.as_ref().map(|v| v[i]));
            st.layers.push(LayerState {
                z: l.z,
                ex: e,
                ez: 0.0,
                e1: e.max(0.0),
                e2: e.min(0.0),
                gamma: 0.0,
                theta: if e > 0.0 { 0.0 } else { PI / 2.0 },
                f1: s.max(0.0),
                f2: (-s).max(0.0),
                v: 0.0,
                sx: s,
                fsz: 0.0,
                w: if e > self.conc.ecr { e * l.sx } else { 0.0 },
                cracked: e > self.conc.ecr,
            });
        }
        for b in &self.bars {
            let e = self.strain(ec, phi, b.z);
            st.bars.push((b.z, e + b.pre, b.steel.stress(e + b.pre)));
        }
        let (n, m) = self.forces_uniaxial(ec, phi);
        st.n = n / 1000.0;
        st.m = m / 1e6;
        st
    }

    /// z in the person's coordinates (undo the mirror used for negative moment).
    pub fn display_z(&self, z: f64) -> f64 {
        if self.flip {
            self.h - z
        } else {
            z
        }
    }
}

#[derive(Clone, Debug)]
pub struct MPhiPoint {
    pub phi: f64, // rad/km
    pub m: f64,   // kNm
    pub ec: f64,
    pub phi_raw: f64,
    pub top: f64,
    pub bot_steel: f64,
}

#[derive(Clone, Debug, Default)]
pub struct MPhiResult {
    pub points: Vec<MPhiPoint>,
    pub peak: usize,
    pub stop_reason: String,
    pub crack_m: Option<f64>,
    pub yield_m: Option<f64>,
}

pub fn moment_curvature(m: &SModel, n_kn: f64) -> MPhiResult {
    let n = n_kn * 1000.0;
    let phi_max = 0.15 / m.h;
    let k_steps = 320;
    let mut pts: Vec<MPhiPoint> = vec![];
    let mut res = MPhiResult::default();
    let mut mpeak = f64::MIN;
    let top_z = m.h;
    for k in 0..=k_steps {
        let r = k as f64 / k_steps as f64;
        let phi = phi_max * r * r;
        let ec = match m.solve_ec(phi, n) {
            Some(e) => e,
            None => {
                res.stop_reason = "Axial load cannot be carried at this curvature.".into();
                break;
            }
        };
        let (_, mm) = m.forces_uniaxial(ec, phi);
        let top = m.strain(ec, phi, top_z);
        let mut bot = f64::MIN;
        let mut fractured = false;
        for b in &m.bars {
            let e = m.strain(ec, phi, b.z) + b.pre;
            bot = bot.max(e);
            if e.abs() > b.steel.eu() {
                fractured = true;
            }
        }
        let p = MPhiPoint { phi: phi * 1e6, m: mm / 1e6, ec, phi_raw: phi, top, bot_steel: bot };
        if res.crack_m.is_none() {
            let bottom = m.strain(ec, phi, 0.0);
            if bottom > m.conc.ecr && k > 0 {
                res.crack_m = Some(p.m);
            }
        }
        if res.yield_m.is_none() {
            for b in &m.bars {
                let e = m.strain(ec, phi, b.z) + b.pre;
                if e > 0.0 && b.steel.stress(e) >= 0.98 * b.steel.fy() {
                    res.yield_m = Some(p.m);
                }
            }
        }
        if p.m > mpeak {
            mpeak = p.m;
            res.peak = pts.len();
        }
        pts.push(p.clone());
        if fractured {
            res.stop_reason = "Reinforcement fractured.".into();
            break;
        }
        if top < -0.012 {
            res.stop_reason = "Concrete strain limit reached at the top.".into();
            break;
        }
        // (only once the moment is positive: with an axial load and unequal top and bottom steel
        // the moment at zero curvature is not zero, and may start negative)
        if k > 10 && mpeak > 0.0 && p.m < 0.6 * mpeak {
            res.stop_reason = "Moment dropped well below the peak.".into();
            break;
        }
    }
    if res.stop_reason.is_empty() {
        res.stop_reason = "Curvature limit reached.".into();
    }
    res.points = pts;
    res
}

/// Flexure-only state carrying moment `m_knm` (ascending branch).
pub fn flex_state(m: &SModel, n_kn: f64, m_knm: f64, mphi: &MPhiResult) -> Option<(f64, f64)> {
    let pts = &mphi.points;
    if pts.is_empty() || m_knm > pts[mphi.peak].m {
        return None;
    }
    if m_knm <= pts[0].m {
        return Some((pts[0].ec, pts[0].phi_raw));
    }
    let mut i = 0;
    while i < mphi.peak && pts[i + 1].m < m_knm {
        i += 1;
    }
    let (mut lo, mut hi) = (pts[i].phi_raw, pts[(i + 1).min(mphi.peak)].phi_raw);
    let n = n_kn * 1000.0;
    let mut ec = pts[i].ec;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        let e = m.solve_ec(mid, n)?;
        let mm = m.forces_uniaxial(e, mid).1 / 1e6;
        if mm < m_knm {
            lo = mid;
        } else {
            hi = mid;
        }
        ec = e;
    }
    let phi = 0.5 * (lo + hi);
    let ec = m.solve_ec(phi, n).unwrap_or(ec);
    Some((ec, phi))
}

/// Shear stress distribution (MPa per layer) from the change in longitudinal force between two
/// nearby sections (dual section), scaled so it adds up to V.
pub fn shear_flow(m: &SModel, n_kn: f64, m_knm: f64, v_kn: f64, mphi: &MPhiResult) -> Option<Vec<f64>> {
    let nl = m.layers.len();
    if v_kn.abs() < 1e-9 {
        return Some(vec![0.0; nl]);
    }
    let mpk = mphi.points[mphi.peak].m.max(1e-6);
    let dm = 0.02 * mpk;
    let (s1, s2) = match (flex_state(m, n_kn, m_knm, mphi), flex_state(m, n_kn, m_knm + dm, mphi)) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => (flex_state(m, n_kn, (m_knm - dm).max(0.0), mphi)?, a),
        _ => return None,
    };
    let lf = |s: (f64, f64), z: f64, mb: f64| m.conc.uniaxial(m.strain(s.0, s.1, z), mb);
    let dl: Vec<f64> = m
        .layers
        .iter()
        .map(|l| (lf(s2, l.z, l.m_bond) - lf(s1, l.z, l.m_bond)) * l.b * l.t)
        .collect();
    let db: Vec<f64> = m
        .bars
        .iter()
        .map(|b| {
            let e1 = m.strain(s1.0, s1.1, b.z) + b.pre;
            let e2 = m.strain(s2.0, s2.1, b.z) + b.pre;
            (b.steel.stress(e2) - b.steel.stress(e1)) * b.area
        })
        .collect();
    Some(flow_from_changes(m, &dl, &db, v_kn))
}

/// Shear stress per layer from the change of longitudinal force in each layer (`dl`) and bar
/// (`db`) between two sections a short distance apart; scaled so the stresses add up to V.
pub fn flow_from_changes(m: &SModel, dl: &[f64], db: &[f64], v_kn: f64) -> Vec<f64> {
    let nl = m.layers.len();
    let mut order: Vec<usize> = (0..nl).collect();
    order.sort_by(|a, b| m.layers[*b].z.partial_cmp(&m.layers[*a].z).unwrap());
    let mut tau = vec![0.0; nl];
    let mut cum = 0.0;
    let mut bars_done = vec![false; m.bars.len()];
    for &i in &order {
        let l = &m.layers[i];
        for (k, b) in m.bars.iter().enumerate() {
            if !bars_done[k] && b.z > l.z {
                bars_done[k] = true;
                cum += db[k];
            }
        }
        let df = dl[i];
        let mid = cum + 0.5 * df;
        cum += df;
        tau[i] = if l.b > 0.0 { (-mid).max(0.0) / l.b } else { 0.0 };
    }
    let total: f64 = (0..nl).map(|i| tau[i] * m.layers[i].b * m.layers[i].t).sum();
    if total <= 0.0 {
        let total_b: f64 = m.layers.iter().map(|l| l.b * l.t).sum();
        return vec![v_kn * 1000.0 / total_b; nl];
    }
    let scale = v_kn * 1000.0 / total;
    tau.iter().map(|t| t * scale).collect()
}

/// MCFT for one layer: given the longitudinal strain ex and shear stress v, find the transverse
/// strain ez and shear strain gamma so that the layer carries v with no net transverse stress
/// (concrete plus stirrups). Principal stresses follow the principal strain directions.
pub fn mcft_layer(
    c: &ConcreteProps,
    ly: &CLayer,
    ex: f64,
    v: f64,
    stir: Option<&(SteelProps, f64)>,
    guess: (f64, f64),
    kres: f64,
    rx: Option<f64>,
) -> Option<LayerState> {
    let va = v.abs();
    if va < 1e-6 {
        let mut s = c.uniaxial(ex, ly.m_bond);
        if let Some(rx) = rx {
            if ex > c.ecr && s > 0.0 {
                s = s.min(rx.max(0.0));
            }
        }
        return Some(LayerState {
            z: ly.z,
            ex,
            e1: ex.max(0.0),
            e2: ex.min(0.0),
            theta: if ex > 0.0 { PI / 2.0 } else { 0.0 },
            f1: s.max(0.0),
            f2: (-s).max(0.0),
            sx: s,
            cracked: ex > c.ecr,
            w: if ex > c.ecr { ex * ly.sx } else { 0.0 },
            ..Default::default()
        });
    }
    let rho = ly.rho_z;
    let (steel, sz) = match stir {
        Some((s, sp)) if rho > 0.0 => (Some(s), *sp),
        _ => (None, f64::INFINITY),
    };
    // stresses from strains; returns (residual sz, residual tau, state)
    let eval = |ez: f64, g: f64| -> (f64, f64, LayerState) {
        let cm = 0.5 * (ex + ez);
        let r = ((0.5 * (ex - ez)).powi(2) + (0.5 * g).powi(2)).sqrt();
        let e1 = cm + r;
        let e2 = cm - r;
        let phi1 = 0.5 * g.atan2(ex - ez); // direction of principal tension from x
        let theta = (PI / 2.0 - phi1).clamp(1e-6, PI - 1e-6); // strut angle from x
        let fsz = steel.map(|s| s.stress(ez)).unwrap_or(0.0);
        let s2 = if e2 < 0.0 { c.compression(e2, e1.max(0.0)) } else { c.tension(e2, ly.m_bond) };
        // the original's lower cracking stress under compression (see ConcreteProps::crack_factor)
        let kct = if std::env::var("SR_NO_CRACK_REDUCTION").is_err() && s2 < 0.0 { c.crack_factor(-s2, e1) } else { 1.0 };
        let mut f1 = if kct < 1.0 && std::env::var("SR_CRACK_MODE").map(|v| v != "A").unwrap_or(true) {
            let mut sc = c.clone();
            sc.ft *= kct;
            sc.ecr *= kct;
            sc.tension(e1, ly.m_bond)
        } else {
            c.tension(e1, ly.m_bond)
        };
        let cracked = e1 > c.ecr * kct;
        let mut w = 0.0;
        if cracked {
            let th = theta.min(PI / 2.0);
            let stheta = 1.0 / (th.sin().abs() / ly.sx + th.cos().abs() / sz);
            w = e1 * stheta;
            let floor_old = std::env::var("SR_FLOOR_LAST").is_err();
            if kct < 1.0 && floor_old {
                // the original's floor across a crack: ft' / (1 + 250 w) (0x4ac920)
                f1 = f1.max(c.ft * kct / (1.0 + 250.0 * w));
            }
            let mut lim = c.vci_max(w) * th.tan().abs();
            if let Some(s) = steel {
                lim += kres * rho * (s.fy() - fsz).max(0.0);
            }
            if let Some(rx) = rx {
                // the longitudinal bars' side (SModel::long_reserve)
                lim = lim.min(rx.max(0.0) + c.vci_max(w) / th.tan().abs().max(1e-9));
            }
            f1 = f1.min(lim.max(0.0));
            if !floor_old {
                // applied last, after the cap, as the original (0x46aaa0, 0x4ac920)
                f1 = f1.max(c.ft * kct / (1.0 + 250.0 * w));
            }
        }
        let (cs, sn) = (phi1.cos(), phi1.sin());
        let sx = f1 * cs * cs + s2 * sn * sn;
        let szc = f1 * sn * sn + s2 * cs * cs;
        let txz = (f1 - s2) * sn * cs;
        let st = LayerState {
            z: ly.z,
            ex,
            ez,
            e1,
            e2,
            gamma: g,
            theta,
            f1,
            f2: (-s2).max(0.0),
            v: txz,
            sx,
            fsz,
            w,
            cracked,
        };
        (szc + rho * fsz, txz - va, st)
    };
    let tol = 1e-4 * (1.0 + va);
    let newton = |mut ez: f64, mut g: f64| -> Option<LayerState> {
        let (mut a, mut b, mut st) = eval(ez, g);
        for _ in 0..60 {
            if a.abs() < tol && b.abs() < tol {
                return Some(st);
            }
            let he = 1e-8 + 1e-6 * ez.abs();
            let hg = 1e-8 + 1e-6 * g.abs();
            let (a1, b1, _) = eval(ez + he, g);
            let (a2, b2, _) = eval(ez, g + hg);
            let j = [[(a1 - a) / he, (a2 - a) / hg], [(b1 - b) / he, (b2 - b) / hg]];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            if !det.is_finite() || det.abs() < 1e-30 {
                return None;
            }
            let de = (a * j[1][1] - b * j[0][1]) / det;
            let dg = (j[0][0] * b - j[1][0] * a) / det;
            let old = a.abs() + b.abs();
            let mut lam = 1.0;
            let mut moved = false;
            for _ in 0..20 {
                let ne = ez - lam * de;
                let ng = (g - lam * dg).max(0.0);
                let (na, nb, ns) = eval(ne, ng);
                if na.abs() + nb.abs() < old {
                    ez = ne;
                    g = ng;
                    a = na;
                    b = nb;
                    st = ns;
                    moved = true;
                    break;
                }
                lam *= 0.5;
            }
            if !moved {
                return None;
            }
        }
        None
    };
    let g_el = va / (0.4 * c.ec);
    let starts = [
        guess,
        (-0.3 * ex.abs(), g_el),
        (0.0, 10.0 * g_el),
        (ex.abs().max(1e-4), ex.abs().max(1e-4) * 2.0),
        (0.002, 0.004),
    ];
    for s in starts {
        if !(s.0.is_finite() && s.1.is_finite()) || s.1 <= 0.0 {
            continue;
        }
        if let Some(st) = newton(s.0, s.1) {
            return Some(st);
        }
    }
    // robust fallback: for a shear strain g find the transverse strain that balances the stirrup
    // direction, then search g so that the layer carries v (first rising crossing = pre-peak)
    let ez_grid = [
        -0.003, -0.0015, -0.0007, -0.0003, -0.0001, -3e-5, 0.0, 1e-5, 3e-5, 1e-4, 3e-4, 1e-3, 3e-3, 0.01, 0.03,
        0.08,
    ];
    let solve_ez = |g: f64| -> Option<LayerState> {
        let mut prev: Option<(f64, f64)> = None;
        for &e in &ez_grid {
            let a = eval(e, g).0;
            if let Some((pe, pa)) = prev {
                if (pa <= 0.0) != (a <= 0.0) {
                    let (mut lo, mut hi, mut flo) = (pe, e, pa);
                    for _ in 0..36 {
                        let mid = 0.5 * (lo + hi);
                        let am = eval(mid, g).0;
                        if (am <= 0.0) == (flo <= 0.0) {
                            lo = mid;
                            flo = am;
                        } else {
                            hi = mid;
                        }
                    }
                    return Some(eval(0.5 * (lo + hi), g).2);
                }
            }
            prev = Some((e, a));
        }
        None
    };
    let mut prev: Option<(f64, f64)> = None;
    for k in 0..=30 {
        let g = 1e-7 * (5e5f64).powf(k as f64 / 30.0);
        let Some(st) = solve_ez(g) else { continue };
        let r = st.v - va;
        if let Some((pg, pr)) = prev {
            if pr < 0.0 && r >= 0.0 {
                let (mut lo, mut hi) = (pg, g);
                let mut best = st;
                for _ in 0..26 {
                    let mid = 0.5 * (lo + hi);
                    match solve_ez(mid) {
                        Some(s) => {
                            if s.v < va {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                            best = s;
                        }
                        None => return None,
                    }
                }
                return Some(best);
            }
        }
        prev = Some((g, r));
    }
    None
}


/// Layer with very little shear: uniaxial longitudinal stress and Mohr's circle for the
/// principal directions (used only where the full MCFT solve is numerically ill-posed).
fn low_shear_layer(c: &ConcreteProps, ly: &CLayer, ex: f64, v: f64) -> LayerState {
    let va = v.abs();
    let s = c.uniaxial(ex, ly.m_bond);
    let centre = s / 2.0;
    let rad = (centre * centre + va * va).sqrt();
    let f1 = (centre + rad).max(0.0);
    let f2 = (rad - centre).max(0.0);
    // angle of principal compression from the axis
    let theta = if s <= 0.0 {
        0.5 * (2.0 * va).atan2(-s)
    } else {
        PI / 2.0 - 0.5 * (2.0 * va).atan2(s)
    };
    let g = if ex > c.ecr { c.ec / 10.0 } else { c.ec / 2.4 };
    LayerState {
        z: ly.z,
        ex,
        ez: 0.0,
        e1: ex.max(0.0),
        e2: ex.min(0.0),
        gamma: va / g,
        theta,
        f1,
        f2,
        v: va,
        sx: s,
        fsz: 0.0,
        w: if ex > c.ecr { ex * ly.sx } else { 0.0 },
        cracked: ex > c.ecr,
    }
}

#[derive(Clone, Debug)]
pub struct ShearResult {
    pub mv: f64,
    pub points: Vec<SectionState>,
    pub reason: String,
    /// the final stop was checked: no equilibrium was found just above it (x1.005 to x1.05)
    /// from any of several fresh starts
    pub confirmed: bool,
    /// how many times the solver stopped but a slightly higher load still balanced
    pub rescued: usize,
    /// what governed the peak, when the solver determined it
    pub mech: Option<Mechanism>,
}

/// What ended a run, in plain words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mechanism {
    Flexure,
    LongYield,
    LongHardening,
    WebShear,
    WebCrushing,
    Unclear,
}

impl Mechanism {
    pub fn label(&self) -> &'static str {
        match self {
            Mechanism::Flexure => "Flexure (bending capacity)",
            Mechanism::LongYield => "Longitudinal bars yielded",
            Mechanism::LongHardening => "Longitudinal bars strain-hardening",
            Mechanism::WebShear => "Web shear (crack slip / stirrups)",
            Mechanism::WebCrushing => "Web crushing",
            Mechanism::Unclear => "Unclear (solver stop)",
        }
    }
}

/// Classify the end of a run from its stop reason and its last converged state.
pub fn mechanism(m: &SModel, sr: &ShearResult) -> Mechanism {
    // crushing means an inclined strut (10-80 deg from the axis), not the flexural compression zone
    let strut = |l: &LayerState| {
        let t = l.theta.to_degrees().abs();
        t > 10.0 && t < 80.0
    };
    if let Some(mm) = sr.mech {
        return mm;
    }
    if sr.reason.starts_with("Flexural") {
        return Mechanism::Flexure;
    }
    let Some(last) = sr.points.last() else { return Mechanism::Unclear };
    if sr.reason.starts_with("The web") {
        let crushed = last
            .layers
            .iter()
            .any(|l| l.f2 > 0.9 * m.conc.peak(l.e1) && l.v > 0.0 && strut(l));
        return if crushed { Mechanism::WebCrushing } else { Mechanism::WebShear };
    }
    // equilibrium not found: is the longitudinal steel the reason?
    let mut yielded = false;
    let mut hardening = false;
    for (b, mb) in last.bars.iter().zip(&m.bars) {
        if let SteelProps::Rebar { fy, es, esh, .. } = mb.steel {
            if b.1 > 0.98 * fy / es {
                yielded = true;
            }
            if b.1 > esh {
                hardening = true;
            }
        } else if b.2 > 0.95 * mb.steel.fy() {
            yielded = true;
        }
    }
    if hardening {
        Mechanism::LongHardening
    } else if yielded {
        Mechanism::LongYield
    } else if last.layers.iter().any(|l| l.f2 > 0.9 * m.conc.peak(l.e1) && l.v > 0.0 && strut(l)) {
        Mechanism::WebCrushing
    } else {
        Mechanism::Unclear
    }
}

fn mcft_forces(
    m: &SModel,
    ec: f64,
    phi: f64,
    tau: &[f64],
    guesses: &[(f64, f64)],
) -> Result<(f64, f64, Vec<LayerState>), usize> {
    let mut n = 0.0;
    let mut mm = 0.0;
    let mut states = Vec::with_capacity(m.layers.len());
    let rxs = m.long_reserve(ec, phi);
    for (i, l) in m.layers.iter().enumerate() {
        let ex = m.strain(ec, phi, l.z);
        let st = match mcft_layer(&m.conc, l, ex, tau[i], m.stirrup.as_ref(), guesses[i], m.stirrup_reserve_factor(), rxs.as_ref().map(|v| v[i])) {
            Some(s) => s,
            None if tau[i].abs() < 0.04 * m.conc.fc.sqrt() => low_shear_layer(&m.conc, l, ex, tau[i]),
            // cover strips outside the stirrups of a stirruped section: their cracks are held by
            // the stirrups next to them, so they are not allowed to govern (estimate)
            None if m.stirrup.is_some() && l.rho_z == 0.0 => {
                low_shear_layer(&m.conc, l, ex, tau[i])
            }
            None => return Err(i),
        };
        let f = st.sx * l.b * l.t;
        n += f;
        mm -= f * (l.z - m.zc);
        states.push(st);
    }
    for b in &m.bars {
        let e = m.strain(ec, phi, b.z);
        let f = b.steel.stress(e + b.pre) * b.area;
        n += f;
        mm -= f * (b.z - m.zc);
    }
    Ok((n, mm, states))
}

fn solve_shear_state(
    m: &SModel,
    n_kn: f64,
    m_knm: f64,
    v_kn: f64,
    mphi: &MPhiResult,
    guesses: &[(f64, f64)],
    start: Option<(f64, f64)>,
    tau_unit: Option<&[f64]>,
) -> Result<SectionState, String> {
    let flex = flex_state(m, n_kn, m_knm, mphi).ok_or("Flexural capacity reached.")?;
    // start from the shear flow the previous load step converged to (scaled to this V); only the
    // first step uses the flexure-only dual-section flow, which is distorted near cracking
    let mut tau = match tau_unit {
        Some(t) if t.len() == m.layers.len() && t.iter().any(|x| *x > 0.0) => t.iter().map(|x| x * v_kn).collect(),
        _ => shear_flow(m, n_kn, m_knm, v_kn, mphi).ok_or("Flexural capacity reached.")?,
    };
    let fresh = vec![(0.0, 0.0); guesses.len()];
    // continue from the last converged state first (shear moves it well away from the
    // flexure-only state), then from the flexure-only state, then with fresh layer guesses
    let solve = |tau: &[f64], mk: f64, start: Option<(f64, f64)>| -> Result<SectionState, String> {
        let mut last_err = String::new();
        let mut tries: Vec<((f64, f64), &[(f64, f64)])> = vec![];
        if let Some(s) = start {
            tries.push((s, guesses));
        }
        tries.push((flex, guesses));
        tries.push((flex, &fresh));
        for (s, g) in tries {
            match solve_shear_from(m, n_kn, mk, v_kn, tau, g, s) {
                Ok(st) => return Ok(st),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    };
    // Shear sharing: when one layer of the web cannot carry its share any more, it keeps a little
    // less and the rest of the web takes the difference (in proportion to what each layer already
    // carries). The section fails only when no layer has reserve left.
    let nl = m.layers.len();
    let area: Vec<f64> = m.layers.iter().map(|l| l.b * l.t).collect();
    let solve_shared = |tau0: &[f64], mk: f64, start: Option<(f64, f64)>| -> Result<(SectionState, Vec<f64>), String> {
        let mut tau = tau0.to_vec();
        // only the web crossed by stirrups (or the whole section when it has none) takes extra
        // shear: cover strips use a simple estimate that never fails, so they must not absorb it
        let mut capped: Vec<bool> = m
            .layers
            .iter()
            .map(|l| m.stirrup.is_some() && l.rho_z == 0.0)
            .collect();
        let mut last = String::new();
        for round in 0..16 {
            match solve(&tau, mk, start) {
                Ok(mut st) => {
                    st.shared = round > 0;
                    return Ok((st, tau));
                }
                Err(e) => {
                    let Some(i) = failed_layer(&e) else { return Err(e) };
                    last = e;
                    let removed = 0.12 * tau[i] * area[i];
                    tau[i] *= 0.88;
                    capped[i] = true;
                    let wsum: f64 = (0..nl).filter(|j| !capped[*j]).map(|j| tau[j] * area[j]).sum();
                    if wsum <= removed {
                        break;
                    }
                    let k = 1.0 + removed / wsum;
                    for j in 0..nl {
                        if !capped[j] {
                            tau[j] *= k;
                        }
                    }
                }
            }
        }
        Err(last)
    };
    let (mut st, t0) = solve_shared(&tau, m_knm, start)?;
    tau = t0;
    if v_kn.abs() < 1e-9 {
        st.tau = tau;
        st.closed = true;
        return Ok(st);
    }
    let mut closed = false;
    // Redistribute the shear over the depth: compare this section with one a short distance
    // along the member (moment changed by dM, same V), both solved with the MCFT, and use the
    // change of longitudinal force in each layer as the new shear flow ("dual section").
    let mpk = mphi.points[mphi.peak].m.max(1e-6);
    let dm = 0.02 * mpk;
    // fixed-point closure of the shear flow (the original allows 20 iterations): under-relaxed,
    // and a pass that cannot be carried is retried with a smaller step instead of stopping
    for _ in 0..5 {
        // the neighbouring section is a small step from this one: one solve from here is enough;
        // near the flexural capacity step back instead of forward
        let here = (st.ec, st.phi / 1e6);
        let near = |mk: f64| solve_shear_from(m, n_kn, mk, v_kn, &tau, guesses, here);
        let first = if m_knm + dm < 0.97 * mpk { near(m_knm + dm).map(|s| (s, 1.0)) } else { Err(String::new()) };
        let (other, sign) = match first {
            Ok(x) => x,
            Err(_) => match near((m_knm - dm).max(0.0)) {
                Ok(s) => (s, -1.0),
                Err(_) => break,
            },
        };
        let dl: Vec<f64> = m
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| sign * (other.layers[i].sx - st.layers[i].sx) * l.b * l.t)
            .collect();
        let db: Vec<f64> = m
            .bars
            .iter()
            .enumerate()
            .map(|(k, b)| sign * (other.bars[k].2 - st.bars[k].2) * b.area)
            .collect();
        let new = flow_from_changes(m, &dl, &db, v_kn);
        let change: f64 = m
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| (new[i] - tau[i]).abs() * l.b * l.t)
            .sum::<f64>()
            / (v_kn.abs() * 1000.0);
        if change < 0.02 {
            closed = true;
            break;
        }
        // if the redistributed flow cannot be carried even with sharing, try a smaller
        // relaxation; if none works keep the state found with the previous flow (a valid
        // equilibrium at this load, marked as not closed)
        let mut moved = false;
        for omega in [0.5, 0.25, 0.1] {
            let blended: Vec<f64> = tau.iter().zip(&new).map(|(a, b)| (1.0 - omega) * a + omega * b).collect();
            if let Ok((s2, t2)) = solve_shared(&blended, m_knm, Some(here)) {
                st = s2;
                tau = t2;
                moved = true;
                break;
            }
        }
        if !moved {
            break;
        }
    }
    st.tau = tau;
    st.closed = closed;
    Ok(st)
}

/// The layer index carried in a "web reached its shear capacity" error, if any.
fn failed_layer(e: &str) -> Option<usize> {
    e.split('\u{1}').nth(1).and_then(|s| s.parse().ok())
}

/// The error text without the internal layer marker.
pub fn clean_reason(e: &str) -> String {
    e.split('\u{1}').next().unwrap_or("").to_string()
}

fn solve_shear_from(
    m: &SModel,
    n_kn: f64,
    m_knm: f64,
    v_kn: f64,
    tau: &[f64],
    guesses: &[(f64, f64)],
    start: (f64, f64),
) -> Result<SectionState, String> {
    let (mut ec, mut phi) = start;
    let nt = n_kn * 1000.0;
    let mt = m_knm * 1e6;
    let tol_n = 1e-4 * m.conc.fc * m.area + 50.0;
    let tol_m = 1e-4 * m.conc.fc * m.area * m.h + 5e4;
    let f = |ec: f64, phi: f64| -> Result<(f64, f64, Vec<LayerState>), String> {
        mcft_forces(m, ec, phi, &tau, guesses)
            .map(|(n, mm, s)| (n - nt, mm - mt, s))
            .map_err(|i| {
                if std::env::var("SR_DEBUG").is_ok() {
                    let l = &m.layers[i];
                    let ex = m.strain(ec, phi, l.z);
                    eprintln!("fail layer z {:.0} ex {:.6} v {:.4} rho {:.5} sx {:.0} m {:.0}", l.z, ex, tau[i], l.rho_z, l.sx, l.m_bond);
                }
                // the failing layer's index rides along after a marker (see `failed_layer`)
                format!("The web at {:.0} mm reached its shear capacity.\u{1}{}", m.display_z(m.layers[i].z), i)
            })
    };
    let mut r = f(ec, phi)?;
    let de = 2e-6;
    let dp = 2e-6 / m.h;
    let mut jac: Option<[[f64; 2]; 2]> = None;
    let mut fresh = false;
    for it in 0..40 {
        let norm = (r.0 / tol_n).abs() + (r.1 / tol_m).abs();
        if r.0.abs() < tol_n && r.1.abs() < tol_m {
            break;
        }
        if jac.is_none() || it % 4 == 3 {
            let a = f(ec + de, phi)?;
            let b = f(ec, phi + dp)?;
            jac = Some([[(a.0 - r.0) / de, (b.0 - r.0) / dp], [(a.1 - r.1) / de, (b.1 - r.1) / dp]]);
            fresh = true;
        }
        let j = jac.unwrap();
        if std::env::var("SR_DEBUG").is_ok() {
            eprintln!("  it {} V {:.1} rN {:.0} rM {:.0} ec {:.6} phi {:.3e}", it, v_kn, r.0, r.1, ec, phi);
        }
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.abs() < 1e-30 {
            return Err("Section became unstable.".into());
        }
        let dx = (r.0 * j[1][1] - r.1 * j[0][1]) / det;
        let dy = (j[0][0] * r.1 - j[1][0] * r.0) / det;
        let mut lam = 1.0;
        let mut ok = false;
        for _ in 0..8 {
            let (ne, np) = (ec - lam * dx, phi - lam * dy);
            if let Ok(nr) = f(ne, np) {
                let new = (nr.0 / tol_n).abs() + (nr.1 / tol_m).abs();
                if new < norm {
                    ec = ne;
                    phi = np;
                    r = nr;
                    ok = true;
                    break;
                }
            }
            lam *= 0.5;
        }
        if !ok {
            if fresh {
                break;
            }
            jac = None; // the slope was stale: rebuild it and try again
            continue;
        }
        fresh = false;
    }
    // accept a small leftover imbalance (kinks from cracking make exact closure hard)
    if (r.0 / tol_n).abs() + (r.1 / tol_m).abs() > 10.0 {
        return Err("Section equilibrium could not be found (failure).".into());
    }
    let mut st = SectionState { ec, phi: phi * 1e6, n: n_kn, m: m_knm, v: v_kn, ..Default::default() };
    let mut gsum = 0.0;
    let mut wsum = 0.0;
    for s in &r.2 {
        if s.v > 0.0 {
            gsum += s.gamma * s.v;
            wsum += s.v;
        }
    }
    st.gamma_avg = if wsum > 0.0 { gsum / wsum * 1000.0 } else { 0.0 };
    st.layers = r.2;
    for b in &m.bars {
        let e = m.strain(ec, phi, b.z) + b.pre;
        st.bars.push((b.z, e, b.steel.stress(e)));
    }
    Ok(st)
}

/// One shear run for the section's loads: a constant moment (if any) is applied first and held,
/// then the shear and the moment increment rise together until the section fails.
/// `m0` is the constant moment in the analysis frame (kNm).
pub fn shear_response_const(m: &SModel, n_kn: f64, mv: f64, m0: f64, mphi: &MPhiResult) -> ShearResult {
    sectional_response(m, (n_kn, m0, 0.0), (0.0, mv, 1.0), mphi)
}

/// The section under the original's loading: constants (N, M, V) applied first and held, then
/// constant + load factor x increments (dN, dM, dV) until failure (analysis frame; kN, kNm).
pub fn sectional_response(m: &SModel, cst: (f64, f64, f64), inc: (f64, f64, f64), mphi: &MPhiResult) -> ShearResult {
    sectional_response_m(m, cst, inc, mphi).0
}

/// As `sectional_response`, with the layer model the kept run used when it was re-meshed (so
/// its layer results can be drawn against the right layers).
pub fn sectional_response_m(m: &SModel, cst: (f64, f64, f64), inc: (f64, f64, f64), mphi: &MPhiResult) -> (ShearResult, Option<SModel>) {
    let plain = cst.1.abs() < 1e-9 && cst.2.abs() < 1e-9 && inc.0.abs() < 1e-12 && inc.2.abs() > 1e-12 && inc.1 / inc.2 >= 0.0;
    let by_moment = inc.2.abs() < 1e-12;
    let (mut r, alt) = with_mesh_retry(m, by_moment, &|mm, primary| {
        if plain && primary {
            // the plain M/V-ratio case also has the older solver as a fallback
            shear_response(mm, cst.0, inc.1 / inc.2, mphi)
        } else {
            crate::path::sectional_path(mm, cst, inc, mphi)
        }
    });
    if plain {
        return (r, alt);
    }
    if crate::path::stalled(&r) && r.points.len() > 1 {
        r.confirmed = false;
        r.reason = format!("{} The solver stopped at the onset of cracking, so the true capacity may be higher.", r.reason);
    }
    (r, alt)
}

pub fn shear_response(m: &SModel, n_kn: f64, mv: f64, mphi: &MPhiResult) -> ShearResult {
    new_or_old(m, n_kn, mv, mphi, || shear_response_steps(m, n_kn, mv, mphi, 1.0))
}

/// Automatic re-meshing: a run that stops without evidence of failure (see
/// `path::failure_proof_m`) is run again with 1.5 x and 0.75 x the number of layers. Of the
/// runs that end in a real failure the one carrying the most is kept; if none does, the
/// largest result is kept and stays unconfirmed. SR_NOREMESH=1 turns it off.
pub fn with_mesh_retry(m: &SModel, by_moment: bool, run: &(dyn Fn(&SModel, bool) -> ShearResult + Sync)) -> (ShearResult, Option<SModel>) {
    with_mesh_retry_sel(m, by_moment, false, run)
}

/// As `with_mesh_retry`. With `outermost` (used for the points of interaction diagrams), the
/// run carrying the most of all meshes is kept even without failure proof: every balanced state
/// of a run is a load that mesh carries, so the envelope passes at least that far out, and taking
/// the proven run only would leave dips wherever the proven mesh happened to stop lower. The
/// result counts as confirmed when that run has failure proof or a proven run comes within 3 %.
pub fn with_mesh_retry_sel(m: &SModel, by_moment: bool, outermost: bool, run: &(dyn Fn(&SModel, bool) -> ShearResult + Sync)) -> (ShearResult, Option<SModel>) {
    let first = run(m, true);
    if std::env::var("SR_NOREMESH").is_ok() || crate::path::strong_proof(&first) {
        return (first, None);
    }
    let first_ok = crate::path::failure_proof_m(Some(m), &first, by_moment);
    let n0 = m.layers.len();
    let peak = |r: &ShearResult| -> f64 {
        if by_moment {
            r.points.iter().map(|p| p.m.abs()).fold(0.0, f64::max)
        } else {
            r.points.iter().map(|p| p.v).fold(0.0, f64::max)
        }
    };
    let mut cands: Vec<(usize, ShearResult, bool, Option<SModel>)> = vec![(n0, first, first_ok, None)];
    let meshes: Vec<usize> = [n0 * 3 / 2, (n0 * 3 / 4).max(12), n0 * 2].into_iter().filter(|n| *n != n0).collect();
    let one_mesh = |n: usize| -> Option<(usize, ShearResult, bool, bool, SModel)> {
        let alt = build(&m.src, n).ok()?;
        let r = run(&alt, false);
        let ok = crate::path::failure_proof_m(Some(&alt), &r, by_moment);
        let strong = crate::path::strong_proof(&r);
        Some((n, r, ok, strong, alt))
    };
    if crate::pool::spare() >= 2 && std::env::var("SR_SERIAL_REMESH").is_err() {
        // cores are free: run the other meshes side by side, then keep them in the same order as
        // the one-at-a-time loop, up to the first unambiguous failure (the same result)
        crate::pool::BUSY.fetch_add(meshes.len(), std::sync::atomic::Ordering::Relaxed);
        let runs: Vec<Option<(usize, ShearResult, bool, bool, SModel)>> = std::thread::scope(|s| {
            let hs: Vec<_> = meshes.iter().map(|&n| s.spawn(move || one_mesh(n))).collect();
            hs.into_iter().map(|h| h.join().ok().flatten()).collect()
        });
        crate::pool::BUSY.fetch_sub(meshes.len(), std::sync::atomic::Ordering::Relaxed);
        for (n, r, ok, strong, alt) in runs.into_iter().flatten() {
            cands.push((n, r, ok, Some(alt)));
            if strong {
                break;
            }
        }
    } else {
        for &n in &meshes {
            let Some((n, r, ok, strong, alt)) = one_mesh(n) else { continue };
            cands.push((n, r, ok, Some(alt)));
            if strong {
                break; // an unambiguous failure: no need for more meshes
            }
        }
    }
    let best = if cands.iter().any(|c| c.2) && !outermost {
        cands.iter().filter(|c| c.2).max_by(|a, b| peak(&a.1).partial_cmp(&peak(&b.1)).unwrap()).unwrap()
    } else {
        cands.iter().max_by(|a, b| peak(&a.1).partial_cmp(&peak(&b.1)).unwrap()).unwrap()
    };
    let summary: Vec<String> = cands
        .iter()
        .map(|c| format!("{} layers {:.1}{}", c.0, peak(&c.1), if c.2 { "" } else { " (no failure proof)" }))
        .collect();
    let mut r = best.1.clone();
    r.confirmed = best.2 || (outermost && cands.iter().any(|c| c.2 && peak(&c.1) >= 0.97 * peak(&best.1)));
    r.reason = format!("{} [re-meshed: {}; kept {} layers]", r.reason, summary.join(", "), best.0);
    (r, best.3.clone())
}

/// The strain-driven solver (path.rs) is used first. Where it stalls right at cracking (stopped
/// before the web has deformed: largest shear strain under 1 mm/m) the older load-stepping
/// solver's result is used instead and the reason says so. SR_OLDSOLVER=1 uses the older solver
/// only.
pub fn new_or_old(m: &SModel, n_kn: f64, a: f64, mphi: &MPhiResult, old: impl Fn() -> ShearResult) -> ShearResult {
    if std::env::var("SR_OLDSOLVER").is_ok() {
        return old();
    }
    let r = crate::path::shear_path(m, n_kn, a, mphi);
    if crate::path::stalled(&r) {
        let mut o = old();
        o.reason = format!("{} (the strain-driven solver stalled at cracking here; the load-stepping solver was used)", o.reason);
        return o;
    }
    r
}

/// `step_scale` below 1 starts with smaller load steps (used to re-run a suspicious result).
pub fn shear_response_steps(m: &SModel, n_kn: f64, mv: f64, mphi: &MPhiResult, step_scale: f64) -> ShearResult {
    let mut res = ShearResult { mv, points: vec![], reason: String::new(), confirmed: false, rescued: 0, mech: None };
    if mphi.points.is_empty() {
        res.reason = "No moment-curvature result.".into();
        return res;
    }
    let nl = m.layers.len();
    let mut guesses = vec![(0.0, 0.0); nl];
    let p0 = mphi.points[0].clone();
    res.points.push(m.uniaxial_state(p0.ec, p0.phi_raw));
    let bw = m.layers.iter().filter(|l| l.b > 0.0).map(|l| l.b).fold(f64::MAX, f64::min);
    let vref = (0.25 * m.conc.fc.sqrt() * bw * 0.8 * m.h / 1000.0).max(1.0);
    let mut v = 0.0;
    // at large M/V ratios a shear step is a big moment step: keep each step within 3 % of the
    // flexural capacity so cracking, yielding and failure are located accurately
    let mpk = mphi.points[mphi.peak].m.max(1e-6);
    let dv_max = if mv > 1e-9 { (vref / 4.0).min(0.03 * mpk / mv) } else { vref / 4.0 };
    let mut dv = (vref / 12.0 * step_scale).min(dv_max);
    let mut tries = 0;
    let mut finished = false;
    // shear flow per kN of V from the last converged step
    let unit = |st: &SectionState| -> Option<Vec<f64>> {
        (st.v > 1e-9 && !st.tau.is_empty()).then(|| st.tau.iter().map(|t| t / st.v).collect())
    };
    while tries < 400 {
        tries += 1;
        let vt = v + dv;
        let mt = vt * mv;
        let last = res.points.last().filter(|p| p.v > 0.0);
        let start = last.map(|p| (p.ec, p.phi / 1e6));
        let tu = last.and_then(unit);
        match solve_shear_state(m, n_kn, mt, vt, mphi, &guesses, start, tu.as_deref()) {
            Ok(st) => {
                for (i, l) in st.layers.iter().enumerate() {
                    if l.v > 0.0 {
                        guesses[i] = (l.ez, l.gamma);
                    }
                }
                v = vt;
                res.points.push(st);
                dv *= 1.3;
                dv = dv.min(dv_max);
            }
            Err(e) => {
                res.reason = clean_reason(&e);
                dv *= 0.5;
                if dv < 0.004 * v.max(vref * 0.1) {
                    // Confirm the stop: look for equilibrium just above it (x1.005 to x1.05)
                    // from several starts. The material model has no unloading, so a balanced
                    // state at a higher load means the section carries it and the stop was
                    // the solver's: the run goes on from there. Only small steps are probed, so
                    // the run cannot leap onto a different branch.
                    if v <= 0.0 || res.reason.starts_with("Flexural") {
                        finished = true;
                        break;
                    }
                    let fresh = vec![(0.0, 0.0); nl];
                    let flow = last.and_then(unit);
                    // a probe may only continue the same branch: it must not leap from a
                    // yielding bar straight onto strain-hardening, nor jump in deformation
                    let max_bar = |st: &SectionState| -> f64 { st.bars.iter().map(|b| b.1).fold(0.0, f64::max) };
                    let esh_min = m
                        .bars
                        .iter()
                        .filter_map(|b| if let SteelProps::Rebar { esh, .. } = b.steel { Some(esh) } else { None })
                        .fold(f64::INFINITY, f64::min);
                    let ey_min = m
                        .bars
                        .iter()
                        .filter_map(|b| if let SteelProps::Rebar { fy, es, .. } = b.steel { Some(fy / es) } else { None })
                        .fold(f64::INFINITY, f64::min);
                    let prev_bar = last.map(max_bar).unwrap_or(0.0);
                    let prev_gamma = last.map(|p| p.gamma_avg).unwrap_or(0.0);
                    // On the yield plateau the section's stiffness is almost zero, so the solver
                    // stalls there although the moment-curvature curve keeps rising into strain
                    // hardening: a run whose bars have already yielded may carry on onto
                    // hardening. A bar that is still elastic may not leap straight past yield,
                    // and the deformation may not jump.
                    // (load control is used for shear-dominated ratios only; there a probe may not
                    // leap onto strain hardening - bending-dominated ratios cross the plateau
                    // properly with curvature control)
                    let _ = ey_min;
                    let same_branch = |st: &SectionState| -> bool {
                        let leap_hardening = prev_bar < esh_min && max_bar(st) >= esh_min;
                        let leap_strain = st.gamma_avg > 1.5 * prev_gamma + 1.0;
                        !leap_hardening && !leap_strain
                    };
                    let mut found = None;
                    'probe: for f in [1.005, 1.01, 1.02] {
                        let vj = v * f;
                        let starts: [(&[(f64, f64)], Option<(f64, f64)>, Option<&[f64]>); 3] = [
                            (&guesses, start, flow.as_deref()),
                            (&fresh, start, flow.as_deref()),
                            (&fresh, None, None),
                        ];
                        for (g, s, t) in starts {
                            if let Ok(st) = solve_shear_state(m, n_kn, vj * mv, vj, mphi, g, s, t) {
                                if same_branch(&st) {
                                    found = Some((vj, st));
                                    break 'probe;
                                }
                            }
                        }
                    }
                    match found {
                        Some((vj, st)) => {
                            for (i, l) in st.layers.iter().enumerate() {
                                if l.v > 0.0 {
                                    guesses[i] = (l.ez, l.gamma);
                                }
                            }
                            v = vj;
                            res.points.push(st);
                            res.rescued += 1;
                            dv = (vref / 24.0 * step_scale).min(dv_max);
                        }
                        None => {
                            finished = true;
                            break;
                        }
                    }
                }
            }
        }
    }
    res.confirmed = finished;
    if res.reason.is_empty() {
        res.reason = "Analysis stopped.".into();
    }
    res
}

#[derive(Clone, Debug, Default)]
pub struct Interaction {
    pub points: Vec<(f64, f64)>, // strength envelope (M kNm, V kN)
    /// for each envelope point: what ended its run and how sure we are of it
    pub info: Vec<PointInfo>,
    // where each run first cracked, first yielded a longitudinal bar and first yielded a stirrup,
    // as the original plots them beside the strength envelope
    pub cracking: Vec<(f64, f64)>,
    pub long_yield: Vec<(f64, f64)>,
    pub trans_yield: Vec<(f64, f64)>,
    /// where the concrete first reaches the designer's strain limit ecu, and where a bar first
    /// reaches its designer's strain limit (by default the start of strain hardening)
    pub conc_limit: Vec<(f64, f64)>,
    pub steel_limit: Vec<(f64, f64)>,
    /// doubtful points checked along the two other loading paths
    pub checks: Vec<CrossCheck>,
    /// for each point: lies inside the envelope (another point on the same side carries both
    /// more moment and more shear), so its run stopped early; not part of the envelope line
    pub inside: Vec<bool>,
}

impl Interaction {
    /// The strength envelope: the points not inside it, in order along the curve (from negative
    /// bending through shear to positive bending). The order is by direction, not by run, so a
    /// point moved out by a cross-check can never make the line double back.
    pub fn envelope(&self) -> Vec<(f64, f64)> {
        let live: Vec<(f64, f64)> = self.points.iter().zip(&self.inside).filter(|(_, i)| !**i).map(|(p, _)| *p).collect();
        let polar = mv_polar(&live);
        let mut order: Vec<usize> = (0..live.len()).collect();
        order.sort_by(|a, b| polar[*b].0.partial_cmp(&polar[*a].0).unwrap_or(std::cmp::Ordering::Equal));
        order.into_iter().map(|i| live[i]).collect()
    }
}

/// Each M-V point in a plot scaled by the bending capacity of its own side and the largest shear:
/// (direction, distance from the origin). Direction 0 = positive bending, pi/2 = pure shear,
/// pi = negative bending.
pub fn mv_polar(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let vmax = points.iter().map(|p| p.1).fold(0.0, f64::max).max(1e-9);
    let mpos = points.iter().map(|p| p.0).fold(0.0, f64::max).max(1e-9);
    let mneg = points.iter().map(|p| -p.0).fold(0.0, f64::max).max(1e-9);
    points
        .iter()
        .map(|p| {
            let x = if p.0 >= 0.0 { p.0 / mpos } else { p.0 / mneg };
            let y = p.1 / vmax;
            (y.atan2(x), (x * x + y * y).sqrt())
        })
        .collect()
}

/// How far short of the line through its neighbours a point may lie and still be on the
/// envelope: a point backed by real failure evidence may mark a true bend of the envelope, so it
/// is given 8 %; a point whose run stopped without that evidence most likely stopped early, so 3 %.
pub const INSIDE_TOL_SURE: f64 = 0.08;
pub const INSIDE_TOL_UNSURE: f64 = 0.03;

/// Each M-V point is where one run, loaded along a straight line from the origin (its M/V
/// ratio), failed; so the envelope is read along those directions. In a plot scaled by the
/// bending capacity of each side and the largest shear, every point has a direction and a
/// distance from the origin. A point whose distance is short of the chord between its neighbours
/// (in direction order, measured along its own direction; the chord does not depend on how far
/// apart the neighbours are) by more than its tolerance lies inside the envelope: its run stopped
/// early. The tolerance depends on the evidence (`sure`: the run ended in real failure):
/// `INSIDE_TOL_SURE` for a confirmed point, `INSIDE_TOL_UNSURE` for one that is not, so solver
/// stops no longer leave small saw-teeth in the line while confirmed bends stay. The worst point
/// is dropped first and the test repeated until no more points drop out, so a run of several
/// early stops is found too. The bending capacities (no shear) always stay as the ends.
pub fn inside_flags(points: &[(f64, f64)], sure: &[bool]) -> Vec<bool> {
    let n = points.len();
    let polar = mv_polar(points);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| polar[*a].0.partial_cmp(&polar[*b].0).unwrap_or(std::cmp::Ordering::Equal));
    let mut inside = vec![false; n];
    loop {
        let live: Vec<usize> = order.iter().copied().filter(|i| !inside[*i]).collect();
        let mut worst: Option<(usize, f64)> = None;
        for w in live.windows(3) {
            let (a, b, c) = (w[0], w[1], w[2]);
            if points[b].1.abs() < 1e-6 {
                continue;
            }
            // where the chord between the neighbours crosses this point's direction (the line
            // the envelope would take without it; independent of how far apart they are)
            let xy = |k: usize| (polar[k].1 * polar[k].0.cos(), polar[k].1 * polar[k].0.sin());
            let ((ax, ay), (cx, cy)) = (xy(a), xy(c));
            let (tb, rb) = polar[b];
            let (ux, uy) = (tb.cos(), tb.sin());
            let (dx, dy) = (cx - ax, cy - ay);
            let den = dx * uy - dy * ux;
            let expect = if den.abs() > 1e-12 {
                let s = -(ax * uy - ay * ux) / den;
                (ax + s * dx) * ux + (ay + s * dy) * uy
            } else {
                0.5 * (polar[a].1 + polar[c].1)
            };
            let short = rb / expect.max(1e-12);
            let tol = if sure.get(b).copied().unwrap_or(true) { INSIDE_TOL_SURE } else { INSIDE_TOL_UNSURE };
            // ranked by how far past its own tolerance each point is
            let excess = short - (1.0 - tol);
            if excess < 0.0 && worst.map(|w| excess < w.1).unwrap_or(true) {
                worst = Some((b, excess));
            }
        }
        match worst {
            Some((b, _)) => inside[b] = true,
            None => break,
        }
    }
    inside
}

/// One doubtful M-V point checked two more ways: its moment held while the shear is raised,
/// and its shear held while the moment is raised.
#[derive(Clone, Debug, Default)]
pub struct CrossCheck {
    /// the point from the proportional (fixed M/V) run, kNm and kN
    pub m: f64,
    pub v: f64,
    /// moment held: the shear reached (kN) and whether that run ended in real failure
    pub fixed_m: Option<(f64, bool)>,
    /// shear held: the moment reached (kNm) and whether that run ended in real failure
    pub fixed_v: Option<(f64, bool)>,
    /// "agree", "moved" (the point was moved out to another path's result) or "disagree"
    pub verdict: String,
    pub moved_to: Option<(f64, f64)>,
}

/// First load stage of a run at which: concrete has cracked, a longitudinal bar has yielded,
/// a stirrup has yielded (M, V of that stage).
fn run_events(m: &SModel, sr: &ShearResult) -> [Option<(f64, f64)>; 5] {
    // each event has an indicator that reaches 1 when it happens: principal tensile strain over
    // the cracking strain, bar strain over yield strain, stirrup strain over yield strain. The
    // event point is interpolated between the two load stages where the indicator passes 1.
    let ecr = m.conc.ecr.max(1e-9);
    let stir_ey = m.stirrup.as_ref().map(|s| s.0.fy() / s.0.modulus());
    let indicator = |p: &SectionState, k: usize| -> f64 {
        match k {
            0 => p.layers.iter().map(|l| l.e1.max(l.ex) / ecr).fold(0.0, f64::max),
            1 => p
                .bars
                .iter()
                .zip(&m.bars)
                .map(|(b, mb)| b.1.abs() / (mb.steel.fy() / mb.steel.modulus()))
                .fold(0.0, f64::max),
            2 => match stir_ey {
                Some(ey) => p
                    .layers
                    .iter()
                    .zip(&m.layers)
                    .filter(|(_, ml)| ml.rho_z > 0.0)
                    .map(|(l, _)| l.ez / ey)
                    .fold(0.0, f64::max),
                None => 0.0,
            },
            // the designer's limits: concrete compressive strain (longitudinal or principal) over
            // ecu, and bar strain over the bar's limit
            3 => p.layers.iter().map(|l| -l.ex / m.conc.ecu).fold(0.0, f64::max),
            _ => p.bars.iter().zip(&m.bars).map(|(b, mb)| b.1.abs() / mb.e_lim.max(1e-6)).fold(0.0, f64::max),
        }
    };
    let mut ev = [None; 5];
    for k in 0..5 {
        for w in sr.points.windows(2) {
            let (a, b) = (indicator(&w[0], k), indicator(&w[1], k));
            if a < 1.0 && b >= 1.0 {
                let t = ((1.0 - a) / (b - a)).clamp(0.0, 1.0);
                ev[k] = Some((w[0].m + t * (w[1].m - w[0].m), w[0].v + t * (w[1].v - w[0].v)));
                break;
            }
        }
    }
    ev
}

/// Moment/shear ratios (m) for `points` failure points: the first is pure shear; the rest are
/// spread evenly in angle around the curve, measured with moment scaled by the flexural capacity
/// and shear by the pure-shear capacity, so the points land evenly along the whole envelope.
pub fn mv_ratios(points: usize, m_cap: f64, v_cap: f64) -> Vec<f64> {
    let n = points.max(3);
    let scale = m_cap.max(1e-6) / v_cap.max(1e-6); // m
    let max_angle = 87.0f64.to_radians();
    (0..n - 1)
        .map(|k| (max_angle * k as f64 / (n - 2) as f64).tan() * scale)
        .collect()
}

/// Deformation-controlled run for bending-dominated ratios: the curvature is stepped and, at
/// each curvature, the strain at the centroid and the shear V are solved so that N is met and
/// M = (M/V) x V. The yield plateau is then crossed without stalling, and the capacity is the
/// true peak of V (V rises, then falls), not the point where a load step stops converging.
pub fn shear_response_curvature(m: &SModel, n_kn: f64, mv: f64, mphi: &MPhiResult) -> ShearResult {
    let mut res = ShearResult { mv, points: vec![], reason: String::new(), confirmed: false, rescued: 0, mech: None };
    if mphi.points.len() < 2 || mv <= 0.0 {
        res.reason = "No moment-curvature result.".into();
        return res;
    }
    let nl = m.layers.len();
    let nt = n_kn * 1000.0;
    let tol_n = 1e-4 * m.conc.fc * m.area + 50.0;
    let tol_m = 1e-4 * m.conc.fc * m.area * m.h + 5e4;
    let p0 = &mphi.points[0];
    res.points.push(m.uniaxial_state(p0.ec, p0.phi_raw));
    let mpk = mphi.points[mphi.peak].m.max(1e-6);
    // shear flow per kN, first from the flexure-only dual section at a small load
    let v0 = (0.05 * mpk / mv).max(1.0);
    let mut unit: Vec<f64> = match shear_flow(m, n_kn, (v0 * mv).min(0.5 * mpk), v0, mphi) {
        Some(t) => t.iter().map(|x| x / v0).collect(),
        None => {
            res.reason = "Flexural capacity reached.".into();
            return res;
        }
    };
    let mut guesses = vec![(0.0, 0.0); nl];
    // curvature targets: the moment-curvature points (dense around cracking and yield), carried on
    // past the flexural peak
    let phi_peak = mphi.points[mphi.peak].phi_raw;
    let mut targets: Vec<f64> = mphi.points.iter().map(|p| p.phi_raw).filter(|p| *p > 0.0).collect();
    let mut extra = phi_peak;
    while targets.len() < 400 && extra < 3.0 * phi_peak.max(1e-9) {
        extra *= 1.08;
        targets.push(extra);
    }
    targets.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut ec = p0.ec;
    let mut v = 0.0f64;
    let mut vmax = 0.0f64;
    let mut fails = 0;
    let mut k = 0;
    let mut phi_prev = 0.0;
    while k < targets.len() {
        let phi = targets[k];
        // first guess: previous state, or the flexure-only one at this curvature
        if v <= 0.0 {
            if let Some(e) = m.solve_ec(phi, nt) {
                ec = e;
                v = (m.forces_uniaxial(e, phi).1 / 1e6 / mv).max(0.1);
            }
        }
        let layer_failed = std::cell::Cell::new(false);
        let resid = |ec: f64, v: f64| -> Option<(f64, f64, Vec<LayerState>)> {
            let tau: Vec<f64> = unit.iter().map(|u| u * v).collect();
            match mcft_forces(m, ec, phi, &tau, &guesses) {
                Ok((n, mm, s)) => Some((n - nt, mm - mv * v * 1e6, s)),
                Err(_) => {
                    layer_failed.set(true);
                    None
                }
            }
        };
        let mut sol = None;
        let (mut e, mut vv) = (ec, v);
        if let Some(mut r) = resid(e, vv) {
            for _ in 0..30 {
                if r.0.abs() < tol_n && r.1.abs() < tol_m {
                    sol = Some((e, vv, r.2));
                    break;
                }
                let de = 2e-6;
                let dvv = (0.002 * vv).max(0.2);
                let (Some(a), Some(b)) = (resid(e + de, vv), resid(e, vv + dvv)) else { break };
                let j = [[(a.0 - r.0) / de, (b.0 - r.0) / dvv], [(a.1 - r.1) / de, (b.1 - r.1) / dvv]];
                let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
                if det.abs() < 1e-30 {
                    break;
                }
                let dx = (r.0 * j[1][1] - r.1 * j[0][1]) / det;
                let dy = (j[0][0] * r.1 - j[1][0] * r.0) / det;
                let norm = (r.0 / tol_n).abs() + (r.1 / tol_m).abs();
                let mut lam = 1.0;
                let mut moved = false;
                for _ in 0..10 {
                    let (ne, nv) = (e - lam * dx, (vv - lam * dy).max(0.0));
                    if let Some(nr) = resid(ne, nv) {
                        if (nr.0 / tol_n).abs() + (nr.1 / tol_m).abs() < norm {
                            e = ne;
                            vv = nv;
                            r = nr;
                            moved = true;
                            break;
                        }
                    }
                    lam *= 0.5;
                }
                if !moved {
                    break;
                }
            }
        }
        match sol {
            Some((e, vv, layers)) => {
                fails = 0;
                ec = e;
                v = vv;
                for (i, l) in layers.iter().enumerate() {
                    if l.v > 0.0 {
                        guesses[i] = (l.ez, l.gamma);
                    }
                }
                let tau: Vec<f64> = unit.iter().map(|u| u * vv).collect();
                let mut st = SectionState { ec, phi: phi * 1e6, n: n_kn, m: mv * vv, v: vv, tau: tau.clone(), closed: true, ..Default::default() };
                let (mut gs, mut ws) = (0.0, 0.0);
                for s in &layers {
                    if s.v > 0.0 {
                        gs += s.gamma * s.v;
                        ws += s.v;
                    }
                }
                st.gamma_avg = if ws > 0.0 { gs / ws * 1000.0 } else { 0.0 };
                st.layers = layers;
                let mut fractured = false;
                for b in &m.bars {
                    let eb = m.strain(ec, phi, b.z) + b.pre;
                    if eb.abs() > b.steel.eu() {
                        fractured = true;
                    }
                    st.bars.push((b.z, eb, b.steel.stress(eb)));
                }
                res.points.push(st);
                vmax = vmax.max(vv);
                // one closure pass of the shear flow for the next step (dual section)
                let dm = 0.02 * mpk;
                let mk = mv * vv;
                let here = (ec, phi);
                let near = |mm: f64| solve_shear_from(m, n_kn, mm, vv, &tau, &guesses, here);
                let other = if mk + dm < 0.97 * mpk { near(mk + dm).map(|s| (s, 1.0)) } else { Err(String::new()) }
                    .or_else(|_| near((mk - dm).max(0.0)).map(|s| (s, -1.0)));
                if let (Ok((o, sign)), Some(cur)) = (other, res.points.last()) {
                    let dl: Vec<f64> = m.layers.iter().enumerate().map(|(i, l)| sign * (o.layers[i].sx - cur.layers[i].sx) * l.b * l.t).collect();
                    let db: Vec<f64> = m.bars.iter().enumerate().map(|(i, b)| sign * (o.bars[i].2 - cur.bars[i].2) * b.area).collect();
                    let new = flow_from_changes(m, &dl, &db, vv);
                    if vv > 1e-9 {
                        unit = unit.iter().zip(&new).map(|(a, b)| 0.5 * a + 0.5 * b / vv).collect();
                    }
                }
                if fractured {
                    res.reason = "Reinforcement fractured.".into();
                    res.confirmed = true;
                    break;
                }
                if vv < 0.85 * vmax {
                    res.reason = "Peak passed: the shear fell while the curvature grew.".into();
                    res.confirmed = true;
                    break;
                }
                phi_prev = phi;
                k += 1;
            }
            None => {
                // no balance at this curvature: try a curvature closer to the last good one
                fails += 1;
                if fails > 5 || phi - phi_prev < 1e-12 {
                    // the stop persisted while the curvature step was halved five times: a real
                    // limit of the section at this ratio
                    res.reason = if layer_failed.get() {
                        "The web reached its shear capacity.".into()
                    } else {
                        "Section equilibrium could not be found (failure).".into()
                    };
                    res.confirmed = true;
                    break;
                }
                targets.insert(k, 0.5 * (phi_prev + phi));
            }
        }
    }
    if res.reason.is_empty() {
        res.reason = "Curvature limit reached.".into();
    }
    res
}

/// One point of the strength envelope and how it was reached.
#[derive(Clone, Copy, Debug)]
pub struct PointInfo {
    pub ratio: f64,
    pub mech: Mechanism,
    /// no equilibrium was found just above the failure load from several fresh starts
    pub confirmed: bool,
    /// lies well below the line through its neighbours, or inside the envelope, even after a
    /// re-run with smaller load steps: treat with care
    pub suspect: bool,
    /// the failure state needed shear sharing between web layers (an estimated rule)
    pub shared: bool,
    /// checked again with the moment held fixed and only the shear raised (agreeing, or the
    /// point raised to the fixed-moment result)
    pub cross_checked: bool,
    /// the shear the fixed-moment check reached when it disagreed with this point (kN)
    pub fixed_moment_v: Option<f64>,
}

#[derive(Clone)]
struct Run {
    ratio: f64,
    peak: Option<(f64, f64)>,
    events: [Option<(f64, f64)>; 5],
    mech: Mechanism,
    confirmed: bool,
    shared: bool,
}

fn run_one(m: &SModel, n_kn: f64, a: f64, mphi: &MPhiResult, step_scale: f64, v_cap: f64) -> Run {
    let t0 = std::time::Instant::now();
    // One rule per regime, so neighbouring ratios follow the same kind of path: bending-dominated
    // ratios use curvature control, shear-dominated ones load control. (The strain-driven
    // strain-driven solver in path.rs is used first; these are its fallback where it stalls.)
    let m_cap = mphi.points.get(mphi.peak).map(|p| p.m).unwrap_or(1.0).max(1e-6);
    let (sr, alt) = with_mesh_retry_sel(m, false, true, &|mm, primary| {
        if !primary {
            return crate::path::shear_path(mm, n_kn, a, mphi);
        }
        new_or_old(m, n_kn, a, mphi, || {
            if v_cap > 0.0 && a * v_cap / m_cap >= 0.35 {
                let c = shear_response_curvature(m, n_kn, a, mphi);
                if c.points.len() > 2 { c } else { shear_response_steps(m, n_kn, a, mphi, step_scale) }
            } else {
                shear_response_steps(m, n_kn, a, mphi, step_scale)
            }
        })
    });
    // events and mechanism read with the layer model the kept run used
    let m = alt.as_ref().unwrap_or(m);
    let best = sr.points.iter().filter(|p| p.v > 0.0).max_by(|x, y| x.v.partial_cmp(&y.v).unwrap());
    // cracking / yield events are read on the way up to the peak only
    let upto = sr
        .points
        .iter()
        .position(|p| Some(p.v) == best.map(|b| b.v))
        .map(|i| i + 1)
        .unwrap_or(sr.points.len());
    let rising = ShearResult { points: sr.points[..upto].to_vec(), ..sr.clone() };
    let mech = mechanism(m, &sr);
    if std::env::var("SR_INTER").is_ok() {
        if let Some(p) = sr.points.last() {
            eprintln!(
                "  flip {} a {:.3} m: V {:.1} M {:.1} steps {} rescued {} confirmed {} in {:.1} s -> {} [{}]",
                m.flip,
                a,
                p.v,
                p.m,
                sr.points.len(),
                sr.rescued,
                sr.confirmed,
                t0.elapsed().as_secs_f64(),
                sr.reason,
                mech.label()
            );
        }
    }
    Run {
        ratio: a,
        peak: best.map(|p| (p.m, p.v)),
        events: run_events(m, &rising),
        mech,
        confirmed: sr.confirmed,
        shared: best.map(|p| p.shared).unwrap_or(false),
    }
}

fn run_many(m: &SModel, n_kn: f64, ratios: &[f64], mphi: &MPhiResult, step_scale: f64, v_cap: f64) -> Vec<Run> {
    std::thread::scope(|s| {
        let hs: Vec<_> = ratios.iter().map(|&a| s.spawn(move || run_one(m, n_kn, a, mphi, step_scale, v_cap))).collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

pub fn interaction(m: &SModel, n_kn: f64, mphi: &MPhiResult, points: usize, progress: &dyn Fn(f32)) -> Interaction {
    progress(0.02);
    let m_cap = mphi.points.get(mphi.peak).map(|p| p.m).unwrap_or(1.0).max(1e-6);
    // pure shear first: its capacity sets the spacing of the other ratios
    let pure = run_one(m, n_kn, 0.0, mphi, 1.0, 0.0);
    let v_cap = pure.peak.map(|p| p.1).unwrap_or(0.1 * m_cap).max(1e-6);
    let ratios: Vec<f64> = mv_ratios(points, m_cap, v_cap).into_iter().skip(1).collect();
    progress(0.1);
    // every ratio is independent: run them all at once, one thread each
    let mut runs = vec![pure];
    runs.extend(run_many(m, n_kn, &ratios, mphi, 1.0, v_cap));
    runs.sort_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap());
    progress(0.5);
    // Adaptive refinement: where neighbouring points fail by different mechanisms, or lie far
    // apart along the curve, run a ratio in between, so a real change of mechanism shows as a
    // well-defined corner instead of one odd point.
    let norm = |p: (f64, f64)| (p.0 / m_cap, p.1 / v_cap);
    let extra: Vec<f64> = {
        let wp: Vec<&Run> = runs.iter().filter(|r| r.peak.is_some()).collect();
        let gaps: Vec<f64> = wp
            .windows(2)
            .map(|w| {
                let (a, b) = (norm(w[0].peak.unwrap()), norm(w[1].peak.unwrap()));
                ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
            })
            .collect();
        let mut sorted = gaps.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
        let mut e: Vec<f64> = wp
            .windows(2)
            .zip(&gaps)
            .filter(|(w, g)| w[0].mech != w[1].mech || **g > 1.6 * median)
            .map(|(w, _)| if w[0].ratio <= 0.0 { 0.5 * w[1].ratio } else { (w[0].ratio * w[1].ratio).sqrt() })
            .collect();
        e.truncate(points.max(4));
        e
    };
    if !extra.is_empty() {
        runs.extend(run_many(m, n_kn, &extra, mphi, 1.0, v_cap));
        runs.sort_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap());
    }
    progress(0.75);
    // A point well below the line through its neighbours is flagged (a diagnostic only: the
    // path-following solver gives one answer per ratio, so re-running would repeat it).
    let low = |runs: &Vec<Run>, i: usize| -> bool {
        let (Some(a), Some(b), Some(c)) = (runs[i - 1].peak, runs[i].peak, runs[i + 1].peak) else { return false };
        let expect = if (c.0 - a.0).abs() > 1e-9 {
            a.1 + (c.1 - a.1) * ((b.0 - a.0) / (c.0 - a.0)).clamp(0.0, 1.0)
        } else {
            0.5 * (a.1 + c.1)
        };
        b.1 < 0.9 * expect
    };
    let mut suspect = vec![false; runs.len()];
    for i in 1..runs.len().saturating_sub(1) {
        suspect[i] = low(&runs, i);
    }
    // as the ratio grows the failure moment should grow: a point at both a smaller moment and a
    // smaller shear than the previous one lies inside the envelope
    for i in 1..runs.len() {
        if let (Some(q), Some(p)) = (runs[i - 1].peak, runs[i].peak) {
            if q.0 >= p.0 + 0.5 && q.1 >= p.1 + 1.0 {
                suspect[i] = true;
            }
        }
    }
    progress(0.8);
    // Cross-check of the doubtful points with a second loading path: the moment found at the
    // point is applied first and held, and only the shear is raised (as the original's
    // "Constant + Increment" loading allows). With no unloading in the material model the
    // failure envelope should not depend much on the path, so:
    //  - if the fixed-moment run carries clearly more shear and ends with real failure evidence,
    //    the proportional run stopped early and the point is raised to it;
    //  - if both agree within 5 %, the point is taken as confirmed;
    //  - if the fixed-moment run gives less, nothing is concluded and the flag stays.
    // Points the envelope test would drop as early stops are doubtful too (the neighbour test
    // above misses a dip whose neighbour is itself low; this one is iterated).
    let early: Vec<bool> = {
        let idx: Vec<usize> = (0..runs.len()).filter(|&i| runs[i].peak.is_some()).collect();
        let mut pts: Vec<(f64, f64)> = idx.iter().map(|&i| runs[i].peak.unwrap()).collect();
        let mut sure: Vec<bool> = idx.iter().map(|&i| runs[i].confirmed && !suspect[i]).collect();
        pts.push((m_cap, 0.0));
        sure.push(true);
        let ins = inside_flags(&pts, &sure);
        let mut e = vec![false; runs.len()];
        for (k, &i) in idx.iter().enumerate() {
            e[i] = ins[k];
        }
        e
    };
    let doubtful: Vec<usize> = (0..runs.len())
        .filter(|&i| {
            let Some(p) = runs[i].peak else { return false };
            (suspect[i] || early[i] || !runs[i].confirmed || runs[i].mech == Mechanism::Unclear) && p.0 > 0.02 * m_cap && p.0 < 0.97 * m_cap && p.1 > 0.0
        })
        .collect();
    let mut checked = vec![None; runs.len()];
    let mut disagree: Vec<Option<f64>> = vec![None; runs.len()];
    let mut checks: Vec<CrossCheck> = vec![];
    if std::env::var("SR_NOXCHECK").is_err() && !doubtful.is_empty() {
        // both other loading paths for every doubtful point, all at once
        type Alt = Option<(f64, f64, bool, Mechanism)>; // (M, V) at the peak, real failure?, what failed
        let alts: Vec<(usize, Alt, Alt)> = std::thread::scope(|s| {
            let hs: Vec<_> = doubtful
                .iter()
                .map(|&i| {
                    let (pm, pv) = runs[i].peak.unwrap();
                    s.spawn(move || {
                        // moment held, shear raised
                        let r = crate::path::sectional_path(m, (n_kn, pm, 0.0), (0.0, 0.0, 1.0), mphi);
                        let ok = crate::path::failure_proof_m(Some(m), &r, false);
                        let a = r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap()).map(|p| (p.m, p.v, ok, mechanism(m, &r)));
                        // shear held, moment raised
                        let r = crate::path::sectional_path(m, (n_kn, 0.0, pv), (0.0, 1.0, 0.0), mphi);
                        let carried = !r.reason.starts_with("The constant loads alone");
                        let ok = crate::path::failure_proof_m(Some(m), &r, true);
                        let b = if carried {
                            r.points.iter().max_by(|a, b| a.m.partial_cmp(&b.m).unwrap()).map(|p| (p.m, p.v, ok, mechanism(m, &r)))
                        } else {
                            None
                        };
                        (i, a, b)
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (i, a, b) in alts {
            let (pm, pv) = runs[i].peak.unwrap();
            let agree_m = a.map(|x| x.2 && (x.1 - pv).abs() <= 0.05 * pv).unwrap_or(false);
            let agree_v = b.map(|x| x.2 && (x.0 - pm).abs() <= 0.05 * pm.abs().max(1.0)).unwrap_or(false);
            // Outward results: another path reached a balanced state further out than the
            // proportional run's peak. With no unloading in the material model a balanced state is
            // a load the section carries however the run that found it ended, so the envelope
            // passes at least that far out; the point is confirmed only if that run also ended in
            // real failure.
            let up_m = a.filter(|x| x.1 > 1.03 * pv).map(|x| ((x.0, x.1), x.1 / pv - 1.0, x.3, x.2));
            let up_v = b.filter(|x| x.0 > 1.03 * pm).map(|x| ((x.0, x.1), x.0 / pm - 1.0, x.3, x.2));
            let verdict;
            let mut moved = None;
            if up_m.is_some() || up_v.is_some() {
                // take the one that reaches furthest out
                let best = [up_m, up_v].into_iter().flatten().max_by(|x, y| x.1.partial_cmp(&y.1).unwrap()).unwrap();
                runs[i].peak = Some(best.0);
                runs[i].mech = best.2;
                runs[i].confirmed = best.3 || agree_m || agree_v;
                suspect[i] = false;
                checked[i] = Some(best.0 .1);
                moved = Some(best.0);
                verdict = "moved";
            } else if agree_m || agree_v {
                runs[i].confirmed = true;
                suspect[i] = false;
                checked[i] = Some(a.map(|x| x.1).unwrap_or(pv));
                verdict = "agree";
            } else {
                runs[i].confirmed = false;
                disagree[i] = a.map(|x| x.1);
                verdict = "disagree";
            }
            if std::env::var("SR_INTER").is_ok() {
                eprintln!(
                    "  cross-check flip {} point ({:.1}, {:.1}): fixed M -> V {} | fixed V -> M {} => {}",
                    m.flip,
                    pm,
                    pv,
                    a.map(|x| format!("{:.1}{}", x.1, if x.2 { "" } else { " (no failure proof)" })).unwrap_or("-".into()),
                    b.map(|x| format!("{:.1}{}", x.0, if x.2 { "" } else { " (no failure proof)" })).unwrap_or("-".into()),
                    verdict
                );
            }
            checks.push(CrossCheck {
                m: pm,
                v: pv,
                fixed_m: a.map(|x| (x.1, x.2)),
                fixed_v: b.map(|x| (x.0, x.2)),
                verdict: verdict.into(),
                moved_to: moved,
            });
        }
    }
    // Every event curve keeps one entry per run, with a gap (NaN) for a run that did not reach
    // the event before its peak, so a curve is never joined across runs that never got there
    // (a yield line would otherwise be drawn straight across the shear-dominated runs), and the
    // first entry is always the pure-shear run (which `interaction_both` drops on one side).
    let gapped = |k: usize| -> Vec<(f64, f64)> { runs.iter().map(|r| r.events[k].unwrap_or((f64::NAN, f64::NAN))).collect() };
    let (cracking, long_yield, trans_yield, conc_limit, steel_limit) = (gapped(0), gapped(1), gapped(2), gapped(3), gapped(4));
    let mut pts = vec![];
    let mut info = vec![];
    for (i, r) in runs.iter().enumerate() {
        if let Some(p) = r.peak {
            pts.push(p);
            info.push(PointInfo {
                ratio: r.ratio,
                mech: r.mech,
                confirmed: r.confirmed,
                suspect: suspect[i],
                shared: r.shared,
                cross_checked: checked[i].is_some(),
                fixed_moment_v: disagree[i],
            });
        }
    }
    pts.push((m_cap, 0.0));
    info.push(PointInfo { ratio: f64::INFINITY, mech: Mechanism::Flexure, confirmed: true, suspect: false, shared: false, cross_checked: false, fixed_moment_v: None });
    progress(1.0);
    let sure: Vec<bool> = info.iter().map(|i| i.confirmed && !i.suspect).collect();
    let inside = inside_flags(&pts, &sure);
    Interaction { points: pts, info, cracking, long_yield, trans_yield, conc_limit, steel_limit, checks, inside }
}


/// Full moment-shear interaction: positive moments, then negative moments (top in tension),
/// joined into one curve from the most negative to the most positive moment, as the original
/// draws it.
/// Which pair of loads a radial interaction diagram varies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pair {
    /// axial load and moment, no shear
    MN,
    /// axial load and shear, at the section's constant moment
    NV,
    /// moment and shear at the constant axial load, with the original's centred run layout
    MV,
}

/// M-V with the original's run layout (0x41dc80 / 0x41c610 / 0x413e60): pure positive and pure
/// negative bending first, then every other run holds the centre moment (their mean) and loads out
/// from it at angle pi k / n towards positive (theta = 0) through shear to negative bending, the
/// moment part scaled to half the moment range and the shear part to a pure-shear run from the
/// centre. Runs whose moment ends negative use the mirrored section (top in tension) so the solver
/// always works in positive bending. Points are (moment kNm, shear kN).
pub fn centred_mv(sec: &Section, points: usize, progress: &dyn Fn(f32)) -> Option<RadialDiagram> {
    let n_kn = sec.loads.axial;
    let mut pos_sec = sec.clone();
    pos_sec.loads.negative = false;
    let mut neg_sec = sec.clone();
    neg_sec.loads.negative = true;
    let mp = build_shear(&pos_sec).ok()?;
    let mn = build_shear(&neg_sec).ok()?;
    let fp = moment_curvature(&build(&pos_sec, 80).ok()?, n_kn);
    let fneg = moment_curvature(&build(&neg_sec, 80).ok()?, n_kn);
    let mplus = fp.points.get(fp.peak).map(|p| p.m).unwrap_or(1.0);
    let mminus = -fneg.points.get(fneg.peak).map(|p| p.m).unwrap_or(1.0);
    let centre = 0.5 * (mplus + mminus);
    let half = 0.5 * (mplus - mminus);
    let vb_est = std::sync::atomic::AtomicU64::new(f64::MAX.to_bits());
    struct Cell<'a>(&'a std::sync::atomic::AtomicU64);
    impl Cell<'_> {
        fn get(&self) -> f64 {
            f64::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed))
        }
        fn set(&self, v: f64) {
            self.0.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed)
        }
    }
    let vb_est = Cell(&vb_est);
    progress(0.1);
    // one run from the centre (cm, user frame) along (dm, dv); a run that heads for negative
    // moment is run on the mirrored section, where its moments change sign
    let one = |cm: f64, dm: f64, dv: f64| -> Option<RadialPoint> {
        let big = dm.abs().max(dv.abs()).max(1e-9);
        let (dm, dv) = (dm / big, dv / big);
        // which side it fails on: a ray heading for negative moment crosses zero moment at a shear
        // of cm dv / |dm|; below the shear capacity there (about vb) it fails in negative bending
        let neg = dm < 0.0 && cm.max(0.0) * dv.abs() / dm.abs().max(1e-9) < vb_est.get();
        let (m0, mphi, s) = if neg { (&mn, &fneg, -1.0) } else { (&mp, &fp, 1.0) };
        // with the automatic re-meshing: runs that stop without failure proof are tried again
        let (r, alt) = with_mesh_retry_sel(m0, false, true, &|mm, _| crate::path::sectional_path(mm, (n_kn, s * cm, 0.0), (0.0, s * dm, dv), mphi));
        let m = alt.as_ref().unwrap_or(m0);
        let pk = r.points.iter().skip(1).max_by(|a, b| {
            let pa = (s * a.m - cm) * dm + a.v * dv;
            let pb = (s * b.m - cm) * dm + b.v * dv;
            pa.partial_cmp(&pb).unwrap()
        })?;
        Some(RadialPoint {
            x: s * pk.m,
            y: pk.v,
            mech: mechanism(m, &r),
            confirmed: crate::path::failure_proof_m(Some(m), &r, false) || crate::path::strong_proof(&r),
            reason: r.reason.clone(),
        })
    };
    let vb = one(centre, 0.0, 1.0).map(|p| p.y).unwrap_or(100.0).max(1.0);
    vb_est.set(vb);
    progress(0.25);
    let n = points.max(4);
    let thetas: Vec<f64> = (1..2 * n).map(|k| std::f64::consts::PI * k as f64 / (2 * n) as f64).collect();
    let mut got: Vec<(f64, RadialPoint)> = std::thread::scope(|s| {
        let hs: Vec<_> = thetas
            .iter()
            .map(|&t| {
                let one = &one;
                s.spawn(move || one(centre, t.cos() * half, t.sin() * vb).map(|p| (t, p)))
            })
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    got.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); // negative bending first
    let mut pts = vec![RadialPoint { x: mminus, y: 0.0, mech: Mechanism::Flexure, confirmed: true, reason: "pure negative bending".into() }];
    pts.extend(got.into_iter().map(|g| g.1));
    pts.push(RadialPoint { x: mplus, y: 0.0, mech: Mechanism::Flexure, confirmed: true, reason: "pure positive bending".into() });
    if std::env::var("SR_MVC").is_ok() {
        eprintln!("   M-V centre {:.1} kNm, half range {:.1} kNm, shear scale {:.1} kN", centre, half, vb);
    }
    progress(1.0);
    Some(RadialDiagram { pair: Pair::MV, points: pts })
}

/// One point of an M-N or N-V diagram.
#[derive(Clone, Debug)]
pub struct RadialPoint {
    /// (moment kNm, axial kN) for M-N; (axial kN, shear kN) for N-V
    pub x: f64,
    pub y: f64,
    pub mech: Mechanism,
    pub confirmed: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct RadialDiagram {
    pub pair: Pair,
    pub points: Vec<RadialPoint>,
}

/// M-N or N-V interaction, done the way the original does all its interaction diagrams
/// (TMNV_baseInter, with TMNInter / TMVInter / TNVInter; control plots "Moment - Axial Load" and
/// "Axial Load - Shear"): a series of full sectional analyses, each loading the section along a
/// straight line in the plane of the two loads (the third held at its constant value), each
/// taken to its peak. The directions are spread evenly in a plot scaled by the pure-compression,
/// pure-tension and bending (or shear) capacities, from pure compression to pure tension, on
/// both moment directions for M-N.
pub fn radial_interaction(sec: &Section, pair: Pair, points: usize, progress: &dyn Fn(f32)) -> Option<RadialDiagram> {
    if pair == Pair::MV {
        return centred_mv(sec, points, progress);
    }
    let mut pos_sec = sec.clone();
    pos_sec.loads.negative = false;
    let mut neg_sec = sec.clone();
    neg_sec.loads.negative = true;
    let mp_model = build_shear(&pos_sec).ok()?;
    let mn_model = build_shear(&neg_sec).ok()?;
    progress(0.05);
    // the axial capacities, from a uniform strain sweep (no curvature): the largest compression
    // and the largest tension the section can carry
    let mut end_m = (0.0, 0.0);
    let (nc0, nt0) = {
        let m80 = build(&pos_sec, 80).ok()?;
        let eu = m80.bars.iter().map(|b| b.steel.eu()).fold(0.0, f64::max).max(0.005);
        // The original finds its ends with load-stepping runs of axial load alone (0x41dc80 sets
        // the increment to N only and runs the sectional solver). In tension the load peaks as
        // the bars yield (steel plus the tension still carried by the concrete) and then falls
        // along the yield plateau; a load-stepping run stops at that peak and never reaches strain
        // hardening (sample beam: 886.3 kN, not the 1140 kN of the bars at rupture). So the
        // tension end is the largest load before the first strain hardening of a mild-steel bar
        // (SR_TENSION_RUPTURE=1: the old sweep to rupture).
        let esh_min = m80
            .bars
            .iter()
            .filter_map(|b| match b.steel {
                SteelProps::Rebar { esh, eu, .. } if esh < eu => Some(esh),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        let et_max = if std::env::var("SR_TENSION_RUPTURE").is_ok() || !esh_min.is_finite() { eu } else { esh_min.min(eu) };
        let (mut c, mut cm, mut t, mut tm) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for k in 1..=400 {
            let e = -0.006 * k as f64 / 400.0;
            let (nn, mm) = m80.forces_uniaxial(e, 0.0);
            if -nn > c {
                c = -nn;
                cm = mm;
            }
            let e = et_max * k as f64 / 400.0;
            let (nn, mm) = m80.forces_uniaxial(e, 0.0);
            if nn > t {
                t = nn;
                tm = mm;
            }
        }
        // (with unequal top and bottom steel a uniform strain also gives a moment)
        end_m = (cm / 1e6, tm / 1e6);
        (c / 1000.0, t / 1000.0)
    };
    if pair == Pair::MN {
        // M-N: with no shear the envelope is the bending capacity at each axial load, found by
        // moment-curvature runs at constant axial load (curvature control, so the drop at
        // cracking under tension does not stall it, which the load-stepping runs did)
        let m80p = build(&pos_sec, 80).ok()?;
        let m80n = build(&neg_sec, 80).ok()?;
        let n = points.max(4);
        let mut levels: Vec<f64> = (1..n).map(|k| -nc0 * (1.0 - k as f64 / n as f64)).collect();
        let nt_steps = (n / 3).max(2);
        levels.extend((0..nt_steps).map(|k| nt0 * k as f64 / nt_steps as f64));
        let res: Vec<(f64, f64, f64, Mechanism, Mechanism)> = std::thread::scope(|s| {
            let hs: Vec<_> = levels
                .iter()
                .map(|&nk| {
                    let (mp, mn) = (&m80p, &m80n);
                    s.spawn(move || {
                        let a = moment_curvature(mp, nk);
                        let b = moment_curvature(mn, nk);
                        let pk = |r: &MPhiResult| r.points.get(r.peak).map(|p| p.m).unwrap_or(0.0);
                        let mech = |r: &MPhiResult| if r.stop_reason.contains("fractur") { Mechanism::LongHardening } else { Mechanism::Flexure };
                        if std::env::var("SR_MN").is_ok() {
                            eprintln!("   N {:.0}: pos {} pts, {} | neg {} pts, {}", nk, a.points.len(), a.stop_reason, b.points.len(), b.stop_reason);
                        }
                        (nk, pk(&a), pk(&b), mech(&a), mech(&b))
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        progress(0.95);
        let pt = |x: f64, y: f64, mech: Mechanism, why: &str| RadialPoint { x, y, mech, confirmed: true, reason: why.into() };
        let mut pts = vec![pt(end_m.0, -nc0, Mechanism::WebCrushing, "pure compression (uniform strain)")];
        for r in &res {
            pts.push(pt(r.1, r.0, r.3, "moment-curvature at constant axial load"));
        }
        pts.push(pt(end_m.1, nt0, Mechanism::LongYield, "pure tension (uniform strain, peak as the bars yield)"));
        for r in res.iter().rev() {
            pts.push(pt(-r.2, r.0, r.4, "moment-curvature at constant axial load"));
        }
        pts.push(pt(end_m.0, -nc0, Mechanism::WebCrushing, "pure compression (uniform strain)"));
        progress(1.0);
        return Some(RadialDiagram { pair, points: pts });
    }
    // scales: squash and tension capacities, bending capacity at zero axial load, shear scale
    let fc = mp_model.conc.phi * mp_model.conc.fc;
    let steel_c: f64 = mp_model.bars.iter().map(|b| b.area * b.steel.fy()).sum();
    let nc = 0.85 * fc * mp_model.area + steel_c; // N, compression
    let nt = steel_c.max(1.0); // N, tension
    let mphi_p = moment_curvature(&mp_model, 0.0);
    let mphi_n = moment_curvature(&mn_model, 0.0);
    let mcap = |r: &MPhiResult| r.points.get(r.peak).map(|p| p.m).unwrap_or(1.0).max(1.0);
    let (mp_cap, mn_cap) = (mcap(&mphi_p), mcap(&mphi_n));
    let bw = mp_model.layers.iter().filter(|l| l.b > 0.0).map(|l| l.b).fold(f64::MAX, f64::min);
    let vscale = (0.25 * fc.sqrt() * bw * 0.8 * mp_model.h).max(1000.0) * 1.5 / 1000.0; // kN
    let m_const = sec.loads.m_const_model();
    let n = points.max(4);
    // directions: alpha from 0 (pure compression) to pi (pure tension)
    let dirs: Vec<f64> = (0..=n).map(|k| std::f64::consts::PI * k as f64 / n as f64).collect();
    let run = |m: &SModel, mphi: &MPhiResult, alpha: f64, other_cap: f64, side: f64| -> Option<RadialPoint> {
        let (c, s) = (alpha.cos(), alpha.sin());
        let dn = if c >= 0.0 { -c * nc / 1000.0 } else { -c * nt / 1000.0 }; // kN, tension +
        let (cst, inc) = match pair {
            Pair::MN => ((0.0, 0.0, 0.0), (dn, s * other_cap, 0.0)),
            _ => ((0.0, m_const, 0.0), (dn, 0.0, s * other_cap)),
        };
        // normalise the increment so the larger part is 1 (the solver scales it anyway)
        let big = inc.0.abs().max(inc.1.abs()).max(inc.2.abs()).max(1e-9);
        let inc = (inc.0 / big, inc.1 / big, inc.2 / big);
        let r = crate::path::sectional_path(m, cst, inc, mphi);
        // the peak: the furthest point along the loading line, in the scaled plot
        let far = |p: &SectionState| -> f64 {
            let nn = if p.n < 0.0 { p.n * 1000.0 / nc } else { p.n * 1000.0 / nt };
            let o = match pair {
                Pair::MN => p.m.abs() / other_cap,
                _ => p.v / other_cap,
            };
            nn * nn + o * o
        };
        let pk = r.points.iter().skip(1).max_by(|a, b| far(a).partial_cmp(&far(b)).unwrap())?;
        let (x, y) = match pair {
            Pair::MN => (side * pk.m.abs(), pk.n),
            _ => (pk.n, pk.v),
        };
        Some(RadialPoint { x, y, mech: mechanism(m, &r), confirmed: crate::path::failure_proof_m(Some(m), &r, false) || crate::path::strong_proof(&r), reason: r.reason.clone() })
    };
    if pair == Pair::NV && std::env::var("SR_NV_ORIGIN").is_err() {
        // N-V the way the original lays out its interaction runs (decompiled: set-up 0x41dc80,
        // second run 0x41c610, the others 0x413e60): first the two extremes - pure compression,
        // then pure tension - at the section's constant moment; then every other run holds the
        // centre of the diagram (the mean of the extreme results: (N0 + N1) / 2, no shear) as its
        // constant load and pushes out from there at angle 2 pi k / n, the direction scaled like an
        // ellipse to the distances of the extremes from the centre (here: half the axial range,
        // and the shear reached by a pure-shear run from the centre).
        let peak_along = |r: &ShearResult, c: (f64, f64), d: (f64, f64)| -> Option<SectionState> {
            // the furthest point along the loading direction from the centre
            r.points.iter().skip(1).max_by(|a, b| {
                let pa = (a.n - c.0) * d.0 + (a.v - c.1) * d.1;
                let pb = (b.n - c.0) * d.0 + (b.v - c.1) * d.1;
                pa.partial_cmp(&pb).unwrap()
            }).cloned()
        };
        let one = |cst_n: f64, dn: f64, dv: f64| -> Option<RadialPoint> {
            let big = dn.abs().max(dv.abs()).max(1e-9);
            let r = crate::path::sectional_path(&mp_model, (cst_n, m_const, 0.0), (dn / big, 0.0, dv / big), &mphi_p);
            let pk = peak_along(&r, (cst_n, 0.0), (dn / big, dv / big))?;
            Some(RadialPoint {
                x: pk.n,
                y: pk.v,
                mech: mechanism(&mp_model, &r),
                confirmed: crate::path::failure_proof_m(Some(&mp_model), &r, false) || crate::path::strong_proof(&r),
                reason: r.reason.clone(),
            })
        };
        let (comp, tens) = std::thread::scope(|s| {
            let a = s.spawn(|| one(0.0, -1.0, 0.0));
            let b = s.spawn(|| one(0.0, 1.0, 0.0));
            (a.join().ok().flatten(), b.join().ok().flatten())
        });
        let (Some(comp), Some(tens)) = (comp, tens) else { return None };
        let nc_run = comp.x;
        let nt_run = tens.x;
        if std::env::var("SR_NV_CENTRED").is_err() {
            // the envelope, one constant axial load at a time: hold N, raise the shear (with the
            // re-meshing and the load-stepping fallback, which runs at constant N), take the peak.
            // The original fans its runs out from the centre; on the sample beam that layout and
            // runs from zero gave the same envelope, so the robust constant-N runs are used.
            let n = points.max(4);
            let levels: Vec<f64> = (1..2 * n).map(|k| nc_run + (nt_run - nc_run) * k as f64 / (2 * n) as f64).collect();
            let m80 = build(&pos_sec, 80).ok()?;
            let got: Vec<(f64, RadialPoint)> = std::thread::scope(|s| {
                let hs: Vec<_> = levels
                    .iter()
                    .map(|&nk| {
                        let (mm, m80) = (&mp_model, &m80);
                        s.spawn(move || {
                            let mphi = moment_curvature(m80, nk);
                            let (r, alt) = sectional_response_m(mm, (nk, m_const, 0.0), (0.0, 0.0, 1.0), &mphi);
                            // the load-stepping solver as well (it restarts from scratch past the
                            // drop at cracking, where the strain-driven one can stall): a stall
                            // only ever under-estimates, so the larger peak is kept
                            let old = if m_const.abs() < 1e-9 { Some(shear_response_steps(mm, nk, 0.0, &mphi, 1.0)) } else { None };
                            let vpk = |r: &ShearResult| r.points.iter().map(|p| p.v).fold(0.0, f64::max);
                            let (r, alt) = match old {
                                Some(o) if vpk(&o) > vpk(&r) => (o, None),
                                _ => (r, alt),
                            };
                            let used = alt.as_ref().unwrap_or(mm);
                            let pk = r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap())?;
                            Some((
                                nk,
                                RadialPoint {
                                    x: nk,
                                    y: pk.v,
                                    mech: mechanism(used, &r),
                                    confirmed: crate::path::failure_proof_m(Some(used), &r, false) || crate::path::strong_proof(&r) || r.reason.contains("reached its shear capacity"),
                                    reason: r.reason.clone(),
                                },
                            ))
                        })
                    })
                    .collect();
                hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
            });
            let mut got = got;
            got.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut pts = vec![comp];
            pts.extend(got.into_iter().map(|g| g.1));
            pts.push(tens);
            progress(1.0);
            return Some(RadialDiagram { pair, points: pts });
        }
        let centre = 0.5 * (nc_run + nt_run);
        let half = 0.5 * (nt_run - nc_run).abs().max(1.0);
        progress(0.2);
        // the shear scale: a pure-shear run from the centre
        let vb = one(centre, 0.0, 1.0).map(|p| p.y).unwrap_or(vscale).max(1.0);
        progress(0.3);
        let n = points.max(4);
        // the upper half of the circle (the lower one is the same with the shear reversed);
        // theta = 0 points to tension, pi to compression
        let thetas: Vec<f64> = (1..2 * n).map(|k| std::f64::consts::PI * k as f64 / (2 * n) as f64).collect();
        let mut got: Vec<(f64, RadialPoint)> = std::thread::scope(|s| {
            let hs: Vec<_> = thetas
                .iter()
                .map(|&t| {
                    let one = &one;
                    s.spawn(move || one(centre, t.cos() * half, t.sin() * vb).map(|p| (t, p)))
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
        });
        got.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); // from the compression side to tension
        let mut pts = vec![comp];
        pts.extend(got.into_iter().map(|g| g.1));
        pts.push(tens);
        if std::env::var("SR_NV").is_ok() {
            eprintln!("   N-V centre {:.1} kN, half range {:.1} kN, shear scale {:.1} kN", centre, half, vb);
        }
        progress(1.0);
        return Some(RadialDiagram { pair, points: pts });
    }
    if pair == Pair::NV {
        // N-V: directions from pure compression to pure tension; where neighbouring results lie
        // far apart (most directions meet the axial limit first) more directions are run in
        // between, twice, as the M-V diagram does
        let (mpm, mpp, runr) = (&mp_model, &mphi_p, &run);
        let run_all = |alphas: &[f64]| -> Vec<(f64, RadialPoint)> {
            std::thread::scope(|s| {
                let hs: Vec<_> = alphas.iter().map(|&a| s.spawn(move || runr(mpm, mpp, a, vscale, 1.0).map(|p| (a, p)))).collect();
                hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
            })
        };
        let mut got = run_all(&dirs);
        for pass in 0..2 {
            got.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let vmax = got.iter().map(|g| g.1.y).fold(1.0, f64::max);
            let norm = |p: &RadialPoint| (if p.x < 0.0 { p.x / nc0 } else { p.x / nt0 }, p.y / vmax);
            let gaps: Vec<f64> = got
                .windows(2)
                .map(|w| {
                    let (a, b) = (norm(&w[0].1), norm(&w[1].1));
                    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
                })
                .collect();
            let mut sorted = gaps.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
            let extra: Vec<f64> = got
                .windows(2)
                .zip(&gaps)
                .filter(|(_, g)| **g > (1.6 * median).max(0.08))
                .flat_map(|(w, _)| {
                    let (a, b) = (w[0].0, w[1].0);
                    if pass == 0 { vec![a + (b - a) / 3.0, a + 2.0 * (b - a) / 3.0] } else { vec![0.5 * (a + b)] }
                })
                .collect();
            if extra.is_empty() {
                break;
            }
            got.extend(run_all(&extra));
        }
        got.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // the ends are the pure-compression and pure-tension runs themselves, as in the original
        // (run on the sample beam: its N-V ends at -7392.3 and 476.8 kN; these runs: about
        // -7422 and 560 kN), not the uniform-strain capacities
        let pts: Vec<RadialPoint> = got.into_iter().map(|g| g.1).collect();
        progress(1.0);
        return Some(RadialDiagram { pair, points: pts });
    }
    let jobs: Vec<(usize, f64, bool)> = match pair {
        Pair::MN => dirs.iter().enumerate().flat_map(|(k, a)| [(k, *a, true), (k, *a, false)]).collect(),
        _ => dirs.iter().enumerate().map(|(k, a)| (k, *a, true)).collect(),
    };
    let mut got: Vec<(usize, bool, RadialPoint)> = std::thread::scope(|s| {
        let hs: Vec<_> = jobs
            .iter()
            .map(|&(k, a, pos)| {
                let (m, mphi, cap, side) = match (pair, pos) {
                    (Pair::MN, true) => (&mp_model, &mphi_p, mp_cap, 1.0),
                    (Pair::MN, false) => (&mn_model, &mphi_n, mn_cap, -1.0),
                    _ => (&mp_model, &mphi_p, vscale, 1.0),
                };
                s.spawn(move || run(m, mphi, a, cap, side).map(|p| (k, pos, p)))
            })
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    progress(0.95);
    // order along the curve: for M-N the negative-moment side from tension back to compression,
    // then the positive side from compression to tension; for N-V compression to tension
    got.sort_by(|a, b| {
        let key = |g: &(usize, bool, RadialPoint)| if g.1 { 1000 + g.0 as i64 } else { 999 - g.0 as i64 };
        key(a).cmp(&key(b))
    });
    // the pure-compression and pure-tension runs are the same on both moment sides
    let mut pts: Vec<RadialPoint> = got.into_iter().map(|g| g.2).collect();
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
    // the ends (no shear) from the uniform-strain sweep: a load-stepping run in pure tension
    // stalls at the drop after cracking
    if let Some(first) = pts.first_mut() {
        *first = RadialPoint { x: -nc0, y: 0.0, mech: Mechanism::WebCrushing, confirmed: true, reason: "pure compression (uniform strain)".into() };
    }
    if let Some(last) = pts.last_mut() {
        *last = RadialPoint { x: nt0, y: 0.0, mech: Mechanism::LongYield, confirmed: true, reason: "pure tension (uniform strain, peak as the bars yield)".into() };
    }
    progress(1.0);
    Some(RadialDiagram { pair, points: pts })
}

pub fn interaction_both(sec: &Section, n_kn: f64, points: usize, progress: &dyn Fn(f32)) -> Option<Interaction> {
    let mut pos_sec = sec.clone();
    pos_sec.loads.negative = false;
    let mut neg_sec = sec.clone();
    neg_sec.loads.negative = true;
    let mp_model = build_shear(&pos_sec).ok()?;
    let mn_model = build_shear(&neg_sec).ok()?;
    let mp = moment_curvature(&mp_model, n_kn);
    let mn = moment_curvature(&mn_model, n_kn);
    let (pos, neg) = std::thread::scope(|s| {
        let hn = s.spawn(|| interaction(&mn_model, n_kn, &mn, points, &|_| {}));
        let p = interaction(&mp_model, n_kn, &mp, points, progress);
        (p, hn.join().unwrap_or_default())
    });
    // negative side mirrored (its pure-shear entry duplicates the positive one), then positive
    let join = |n: &[(f64, f64)], p: &[(f64, f64)]| -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = n.iter().skip(1).rev().map(|(m, v)| (-m, *v)).collect();
        v.extend(p.iter().copied());
        v
    };
    let mut info: Vec<PointInfo> = neg.info.iter().skip(1).rev().copied().collect();
    info.extend(pos.info.iter().copied());
    let points = join(&neg.points, &pos.points);
    let sure: Vec<bool> = info.iter().map(|i| i.confirmed && !i.suspect).collect();
    let inside = inside_flags(&points, &sure);
    // the moment-curvature curves' own design-limit points, at zero shear, as the ends of the
    // M-V limit curves (negative side mirrored)
    let (pc, ps) = mphi_limits(&mp_model, &mp);
    let (nc, ns) = mphi_limits(&mn_model, &mn);
    let ends = |side: &[(f64, f64)], neg_end: Option<(f64, f64)>, pos_end: Option<(f64, f64)>| -> Vec<(f64, f64)> {
        let mut v = vec![];
        v.push(neg_end.map(|p| (-p.1, 0.0)).unwrap_or((f64::NAN, f64::NAN)));
        v.extend(side.iter().copied());
        v.push(pos_end.map(|p| (p.1, 0.0)).unwrap_or((f64::NAN, f64::NAN)));
        v
    };
    Some(Interaction {
        points,
        inside,
        info,
        cracking: join(&neg.cracking, &pos.cracking),
        long_yield: join(&neg.long_yield, &pos.long_yield),
        trans_yield: join(&neg.trans_yield, &pos.trans_yield),
        conc_limit: ends(&join(&neg.conc_limit, &pos.conc_limit), nc, pc),
        steel_limit: ends(&join(&neg.steel_limit, &pos.steel_limit), ns, ps),
        checks: neg
            .checks
            .iter()
            .map(|c| CrossCheck {
                m: -c.m,
                fixed_v: c.fixed_v.map(|(mm, ok)| (-mm, ok)),
                moved_to: c.moved_to.map(|(mm, vv)| (-mm, vv)),
                ..c.clone()
            })
            .chain(pos.checks.iter().cloned())
            .collect(),
    })
}

#[derive(Clone, Debug, Default)]
pub struct MemberResult {
    pub points: Vec<(f64, f64)>, // (load kN, deflection mm)
    pub p_max: f64,
    pub span: f64,
    pub a: f64,
}

/// Simply supported span with one point load: load-deflection from the moment-curvature curve.
pub fn member_response(mem: &Member, mphi: &MPhiResult) -> MemberResult {
    let l = (mem.right_sup - mem.left_sup).abs().max(1.0);
    let a = (mem.load_at - mem.left_sup).clamp(0.05 * l, 0.95 * l);
    let b = l - a;
    let pts = &mphi.points;
    let mut out = MemberResult { span: l, a, ..Default::default() };
    if pts.is_empty() {
        return out;
    }
    let mpeak = pts[mphi.peak].m;
    let phi_of = |mm: f64| -> f64 {
        let mm = mm.abs();
        for i in 0..mphi.peak {
            let (p1, p2) = (&pts[i], &pts[i + 1]);
            if mm >= p1.m && mm <= p2.m && p2.m > p1.m {
                return p1.phi_raw + (p2.phi_raw - p1.phi_raw) * (mm - p1.m) / (p2.m - p1.m);
            }
        }
        pts[mphi.peak].phi_raw
    };
    let p_max = mpeak * 1e6 / (a * b / l) / 1000.0; // kN
    out.p_max = p_max;
    let steps = 60;
    let nx = 200;
    for s in 0..=steps {
        let p = p_max * s as f64 / steps as f64 * 0.999;
        let mut defl = 0.0;
        for i in 0..nx {
            let x = (i as f64 + 0.5) * l / nx as f64;
            let mbar = if x < a { x * b / l } else { (l - x) * a / l }; // mm per unit load
            let mx = p * 1000.0 * mbar / 1e6; // kNm
            defl += phi_of(mx) * mbar * l / nx as f64;
        }
        out.points.push((p, defl));
    }
    out
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    /// A smooth M-V curve (negative bending, shear, positive bending) with one dip `d` (fraction
    /// of the distance) at index `k`.
    fn curve(k: usize, d: f64) -> Vec<(f64, f64)> {
        // 37 points, 5 degrees apart (about what 20 points per side give)
        (0..=36)
            .map(|i| {
                let t = std::f64::consts::PI * (1.0 - i as f64 / 36.0); // pi .. 0
                let s = if i == k { 1.0 - d } else { 1.0 };
                let (mcap, vcap) = (if t.cos() < 0.0 { 120.0 } else { 350.0 }, 300.0);
                (s * mcap * t.cos(), (s * vcap * t.sin()).max(0.0))
            })
            .collect()
    }

    #[test]
    fn small_dip_dropped_only_when_unconfirmed() {
        let pts = curve(26, 0.05);
        let mut sure = vec![true; pts.len()];
        assert!(!inside_flags(&pts, &sure)[26], "a confirmed 5 % bend stays on the envelope");
        sure[26] = false;
        assert!(inside_flags(&pts, &sure)[26], "an unconfirmed 5 % dip is an early stop");
        assert_eq!(inside_flags(&pts, &sure).iter().filter(|x| **x).count(), 1, "only the dip goes");
    }

    #[test]
    fn large_dip_dropped_even_when_confirmed() {
        let pts = curve(10, 0.15);
        let sure = vec![true; pts.len()];
        let ins = inside_flags(&pts, &sure);
        assert!(ins[10]);
        assert_eq!(ins.iter().filter(|x| **x).count(), 1, "only the dip goes");
    }

    #[test]
    fn wide_gap_does_not_drop_a_confirmed_neighbour() {
        // two early stops next to each other leave a wide gap; the confirmed point beside them
        // lies on the true curve and must stay
        let mut pts = curve(99, 0.0);
        let mut sure = vec![true; pts.len()];
        for k in [19, 20, 21, 22] {
            pts[k] = (0.6 * pts[k].0, 0.6 * pts[k].1);
            sure[k] = false;
        }
        let ins = inside_flags(&pts, &sure);
        assert!([19, 20, 21, 22].iter().all(|k| ins[*k]));
        assert_eq!(ins.iter().filter(|x| **x).count(), 4);
    }

    #[test]
    fn envelope_is_in_direction_order() {
        // a point moved out of run order (as a cross-check can do) must not make the line double back
        let mut pts = curve(99, 0.0);
        pts.swap(3, 29);
        let it = Interaction { inside: vec![false; pts.len()], points: pts, ..Default::default() };
        let env = it.envelope();
        let th: Vec<f64> = mv_polar(&env).iter().map(|p| p.0).collect();
        assert!(th.windows(2).all(|w| w[1] <= w[0]), "from negative bending (pi) to positive (0)");
        assert!(env.first().unwrap().0 < 0.0 && env.last().unwrap().0 > 0.0);
    }
}
