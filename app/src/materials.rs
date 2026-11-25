//! Material stress-strain rules (see MECHANICS.md). Stress in MPa, strain plain, tension positive.

use crate::model::*;

#[derive(Clone, Debug)]
pub struct ConcreteProps {
    pub fc: f64,
    pub ft: f64,
    pub e0: f64, // positive magnitude
    pub ec: f64,
    pub ecr: f64,
    pub agg_eff: f64,
    pub ts: f64,
    pub tension_stiffening: bool,
    pub soft: Softening,
    pub base: BaseCurve,
    /// designer's compressive strain limit, plain strain (positive)
    pub ecu: f64,
    /// material factor phi_c: every concrete stress is phi_c times the unfactored curve's, in
    /// compression and tension alike (the original's 0x4ad9c0 returns phi_c f; its tangent
    /// 0x4ac960 and the crack floor 0x4ac920 are scaled the same). `ft` and `ec` here already
    /// carry it; strains (e0, ecr) do not change
    pub phi: f64,
}

impl ConcreteProps {
    pub fn new(c: &Concrete) -> Self {
        let fc = c.fc.max(5.0);
        let ft = c
            .ft
            .filter(|v| *v > 0.0)
            .unwrap_or_else(|| (0.15 * fc).min(1.4 * (1.0 + 0.01 * fc)));
        let e0 = c.e0.filter(|v| *v > 0.0).map(|v| v / 1000.0).unwrap_or_else(|| {
            let ecp = 3320.0 * fc.sqrt() + 6900.0;
            let n = (0.8 + fc / 17.0).max(1.01);
            fc / ecp * n / (n - 1.0)
        });
        let agg_eff = {
            let mut a = c.agg;
            if fc >= 60.0 {
                a = if fc <= 70.0 { (70.0 - fc) * a / 10.0 } else { 0.0 };
            }
            a.min(25.0)
        };
        let mut p = ConcreteProps {
            fc,
            ft,
            e0,
            ec: 1.0,
            ecr: 1.0,
            agg_eff,
            ts: c.tsfactor.clamp(0.0, 1.0),
            tension_stiffening: c.tension_stiffening,
            ecu: (c.ecu / 1000.0).max(1e-4),
            soft: c.softening,
            base: c.base,
            phi: 1.0,
        };
        p.ec = -p.compression(-1e-4, 0.0) / 1e-4;
        p.ecr = ft / p.ec;
        p
    }

    /// The same concrete with its stresses scaled by the material factor phi_c (strains kept).
    pub fn factored(mut self, phi: f64) -> Self {
        let phi = phi.clamp(0.01, 10.0);
        self.ft *= phi / self.phi;
        self.ec *= phi / self.phi;
        self.phi = phi;
        self
    }

    /// Peak of the (softened) compression curve, factored, as a positive number.
    pub fn peak(&self, e1: f64) -> f64 {
        self.phi * self.beta(e1.max(0.0)) * self.fc
    }

    pub fn beta(&self, e1: f64) -> f64 {
        if e1 <= 0.0 {
            return 1.0;
        }
        let b = match self.soft {
            Softening::None => 1.0,
            Softening::VecchioCollins1986 => 1.0 / (0.8 + 0.34 * e1 / self.e0),
            Softening::Default2011 => 1.0 / (0.8 + (3.0 * self.fc).max(170.0) * e1),
        };
        b.min(1.0)
    }

    /// Principal compressive stress (negative) at strain e2 (negative) with transverse tensile strain e1.
    pub fn compression(&self, e2: f64, e1: f64) -> f64 {
        if e2 >= 0.0 {
            return 0.0;
        }
        let beta = self.beta(e1);
        let fp = beta * self.fc;
        let r = -e2 / self.e0;
        let f = match self.base {
            BaseCurve::Parabolic => fp * (2.0 * r - r * r),
            BaseCurve::Popovics => {
                let n = (0.8 + fp / 17.0).max(1.1);
                let k = if r < 1.0 { 1.0 } else { (0.67 + fp / 62.0).max(1.0) };
                fp * n * r / (n - 1.0 + r.powf(n * k))
            }
        };
        -self.phi * f.max(0.0)
    }

    /// Strain at the peak of the (softened) curve, as a negative number.
    pub fn peak_strain(&self) -> f64 {
        -self.e0
    }

    /// Average tensile stress at tensile strain e1; `m` is the bond parameter in mm.
    pub fn tension(&self, e1: f64, m: f64) -> f64 {
        if e1 <= 0.0 {
            return 0.0;
        }
        if e1 <= self.ecr {
            return self.ec * e1;
        }
        if !self.tension_stiffening {
            return 0.0;
        }
        self.ts * self.ft / (1.0 + (3.6 * m * e1).sqrt())
    }

    /// Continuous version of `tension` for the strain-driven solver: instead of dropping from ft
    /// to the tension-stiffening curve right after cracking, it goes linearly from (ecr, ft) to
    /// the curve's value at 2 ecr (a change of under 9 % of ft over about 65 microstrain,
    /// **estimated** smoothing; the original's own curve is used beyond 2 ecr).
    pub fn tension_c(&self, e1: f64, m: f64) -> f64 {
        if e1 <= 0.0 {
            return 0.0;
        }
        let at2 = if self.tension_stiffening { self.ts * self.ft / (1.0 + (3.6 * m * 2.0 * self.ecr).sqrt()) } else { 0.0 };
        if e1 <= 0.5 * self.ecr {
            return self.ec * e1;
        }
        if e1 <= 2.0 * self.ecr {
            // rising elastic line and falling post-cracking line, joined by a smooth minimum so
            // the peak at cracking has no corner (lowers the peak by about 2 % of ft)
            let up = self.ec * e1;
            let down = self.ft + (at2 - self.ft) * (e1 - self.ecr) / self.ecr;
            return smin(up, down, 0.04 * self.ft);
        }
        if !self.tension_stiffening {
            return 0.0;
        }
        self.ts * self.ft / (1.0 + (3.6 * m * e1).sqrt())
    }

    /// The original's reduction of the cracking stress under compression (r2k 0x46a960, on by
    /// default): factor clamp(1.05 - f2 / fp, 0.05, 1) on the cracking stress, with f2 the
    /// principal compressive stress and fp the peak of the (softened) compression curve
    /// (1.05 at 0x52f5e0, 0.05 at 0x52f040). Concrete compressed near its strength cracks at a
    /// small fraction of ft, as biaxial tests (Kupfer) show.
    pub fn crack_factor(&self, f2_mag: f64, e1: f64) -> f64 {
        // f2 comes from the factored curve, so it is measured against the factored peak
        let fp = self.peak(e1).max(1e-9);
        (1.05 - f2_mag / fp).clamp(0.05, 1.0)
    }

    /// `tension_c` with the cracking stress and strain scaled by `k` (see `crack_factor`); the
    /// tension-stiffening branch after cracking is scaled the same way (**estimated**: the
    /// original keeps its own post-cracking branch, but its crack check governs there).
    pub fn tension_ck(&self, e1: f64, m: f64, k: f64) -> f64 {
        if k >= 1.0 - 1e-12 {
            return self.tension_c(e1, m);
        }
        let k = k.max(0.05);
        // the same curve drawn for a concrete with ft and ecr reduced by k
        let mut scaled = self.clone();
        scaled.ft *= k;
        scaled.ecr *= k;
        scaled.tension_c(e1, m)
    }

    pub fn uniaxial(&self, e: f64, m: f64) -> f64 {
        if e >= 0.0 {
            self.tension(e, m)
        } else {
            self.compression(e, 0.0)
        }
    }

    /// Maximum shear stress that a crack of width w (mm) can carry.
    pub fn vci_max(&self, w: f64) -> f64 {
        // scaled by phi_c with the other concrete stresses (inferred: the original multiplies
        // concrete quantities by phi_c at 0x406737 and 0x41e751 as well, not decoded further)
        self.phi * 0.18 * self.fc.sqrt() / (0.31 + 24.0 * w / (self.agg_eff + 16.0))
    }
}

/// Smooth minimum of a and b: equals the smaller one away from the crossing, rounded within
/// about k of it.
pub fn smin(a: f64, b: f64, k: f64) -> f64 {
    0.5 * (a + b - ((a - b) * (a - b) + k * k).sqrt())
}

/// Smooth max(x, 0), rounded within about k of zero.
pub fn splus(x: f64, k: f64) -> f64 {
    0.5 * (x + (x * x + k * k).sqrt())
}

#[derive(Clone, Debug)]
pub enum SteelProps {
    /// `phi`: material factor on every stress; `cap`: a factored run, where the original
    /// (0x4aea40) holds the stress at fy (elastic-perfectly plastic, no strain hardening)
    Rebar { fy: f64, fu: f64, es: f64, esh: f64, eu: f64, phi: f64, cap: bool, rup: f64 },
    Prestress { fpu: f64, ep: f64, a: f64, b: f64, c: f64, eu: f64, phi: f64, cap: bool, rup: f64 },
}

impl SteelProps {
    pub fn from_rebar(r: &Rebar) -> Self {
        SteelProps::Rebar {
            fy: r.fy,
            fu: r.fu.max(r.fy),
            es: r.es,
            esh: (r.esh / 1000.0).max(r.fy / r.es),
            eu: r.eu / 1000.0,
            phi: 1.0,
            cap: false,
            rup: f64::INFINITY,
        }
    }
    pub fn from_prestress(p: &Prestress) -> Self {
        SteelProps::Prestress { fpu: p.fpu, ep: p.ep, a: p.a, b: p.b, c: p.c, eu: p.eu / 1000.0, phi: 1.0, cap: false, rup: f64::INFINITY }
    }
    /// The factored steel of the original (0x4aea40): stress phi times the curve's, held at fy
    /// in a factored run. The factor is phi_p for a steel stronger than 750 MPa (0x4ac800,
    /// 750e6 at 0x5311f8: strands), phi_s otherwise.
    pub fn factored(mut self, phis: f64, phip: f64, factored_run: bool) -> Self {
        let strong = self.fy_plain() > 750.0;
        let f = if strong { phip } else { phis }.clamp(0.01, 10.0);
        match &mut self {
            SteelProps::Rebar { phi, cap, .. } | SteelProps::Prestress { phi, cap, .. } => {
                *phi = f;
                *cap = factored_run;
            }
        }
        self
    }
    fn fy_plain(&self) -> f64 {
        match *self {
            SteelProps::Rebar { fy, .. } => fy,
            SteelProps::Prestress { fpu, .. } => 0.9 * fpu,
        }
    }
    /// The same steel, ruptured (no stress) past total strain e: rupture at a crack, where the
    /// strain is higher than the average (r2k 0x4ae520); the stress-strain curve is unchanged.
    pub fn with_rupture(mut self, e: f64) -> Self {
        match &mut self {
            SteelProps::Rebar { rup, .. } | SteelProps::Prestress { rup, .. } => *rup = e,
        }
        self
    }
    /// Total strain at which the bar ruptures: at a crack, or its own eu.
    pub fn rupture(&self) -> f64 {
        match *self {
            SteelProps::Rebar { rup, eu, .. } | SteelProps::Prestress { rup, eu, .. } => rup.min(eu),
        }
    }
    /// The steel's stress factor.
    pub fn phi(&self) -> f64 {
        match *self {
            SteelProps::Rebar { phi, .. } | SteelProps::Prestress { phi, .. } => phi,
        }
    }
    /// Yield stress, factored (phi fy), so it compares with `stress`; fy / modulus stays the
    /// plain yield strain.
    pub fn fy(&self) -> f64 {
        self.phi() * self.fy_plain()
    }
    pub fn eu(&self) -> f64 {
        match *self {
            SteelProps::Rebar { eu, .. } => eu,
            SteelProps::Prestress { eu, .. } => eu,
        }
    }
    /// Elastic modulus, factored like the stresses.
    pub fn modulus(&self) -> f64 {
        self.phi()
            * match *self {
                SteelProps::Rebar { es, .. } => es,
                SteelProps::Prestress { ep, .. } => ep,
            }
    }
    pub fn stress(&self, e: f64) -> f64 {
        let (phi, cap, rup) = match *self {
            SteelProps::Rebar { phi, cap, rup, .. } | SteelProps::Prestress { phi, cap, rup, .. } => (phi, cap, rup),
        };
        // ruptured at a crack (in tension): its force drops out (r2k 0x4201e0)
        if e > rup {
            return 0.0;
        }
        let f = self.stress_plain(e);
        let f = if cap { f.clamp(-self.fy_plain(), self.fy_plain()) } else { f };
        phi * f
    }
    fn stress_plain(&self, e: f64) -> f64 {
        let a = e.abs();
        let s = e.signum();
        match *self {
            SteelProps::Rebar { fy, fu, es, esh, eu, .. } => {
                let ey = fy / es;
                if a <= ey {
                    es * e
                } else if a < esh || eu <= esh {
                    if a > eu {
                        0.0
                    } else {
                        s * fy
                    }
                } else if a <= eu {
                    let d = a - eu;
                    let d0 = esh - eu;
                    s * (fu + (fy - fu) * d * d / (d0 * d0))
                } else {
                    0.0
                }
            }
            SteelProps::Prestress { fpu, ep, a: ra, b, c, eu, .. } => {
                if a > eu {
                    return 0.0;
                }
                let f = ep * a * (ra + (1.0 - ra) / (1.0 + (b * a).powf(c)).powf(1.0 / c));
                s * f.min(fpu)
            }
        }
    }
}
