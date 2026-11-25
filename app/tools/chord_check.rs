// Independent check of the sample beam's moment capacity under a constant shear: a plain fibre
// model (plane sections, uniaxial materials) plus the classic truss term for shear - the web's
// diagonal compression adds a longitudinal compression V cot(theta) at mid-depth of the web,
// which the fibres (bars and chords) must balance with net tension (N = 0 overall).
// Nothing here comes from the program under test except the input material properties.
//
// Section: 300 x 600, f'c 40 (e'c 2.10 mm/m, Popovics), ft 1.96, Ec 27897;
// bottom 3-25M (1500 mm2) at 60 mm from the bottom, top 2-15M (400 mm2) at 50 mm from the top;
// steel fy 400, Es 200000, esh 15 mm/m, fu 600, eu 80 mm/m (quadratic hardening).

const B: f64 = 300.0;
const H: f64 = 600.0;
const FC: f64 = 40.0;
const E0: f64 = 0.0021;
const FT: f64 = 1.96;
const EC: f64 = 27897.0;

fn concrete(e: f64, ts: bool) -> f64 {
    if e >= 0.0 {
        let ecr = FT / EC;
        if e <= ecr {
            return EC * e;
        }
        // tension stiffening (Bentz form, bond parameter m about 190 mm for the bottom bars)
        return if ts { FT / (1.0 + (3.6 * 190.0 * e).sqrt()) } else { 0.0 };
    }
    let r = -e / E0;
    let n: f64 = (0.8 + FC / 17.0).max(1.1);
    let k: f64 = if r < 1.0 { 1.0 } else { (0.67 + FC / 62.0).max(1.0) };
    -(FC * n * r / (n - 1.0 + r.powf(n * k)))
}

fn steel(e: f64) -> f64 {
    let (fy, es, esh, fu, eu) = (400.0, 200000.0, 0.015, 600.0, 0.080);
    let a = e.abs();
    let s = e.signum();
    if a <= fy / es {
        es * e
    } else if a < esh {
        s * fy
    } else if a <= eu {
        let d = a - eu;
        let d0 = esh - eu;
        s * (fu + (fy - fu) * d * d / (d0 * d0))
    } else {
        0.0 // ruptured
    }
}

struct Sec {
    nf: usize,
}

impl Sec {
    // strain at height z (from the bottom): e = e_bot + kappa * (... ) written with the top strain
    fn forces(&self, etop: f64, kappa: f64, ts: bool) -> (f64, f64, f64, f64) {
        // returns (N, M about mid-depth, bottom bar strain, bottom bar stress)
        let strain = |z: f64| etop + kappa * (H - z);
        let dz = H / self.nf as f64;
        let (mut n, mut m) = (0.0, 0.0);
        for i in 0..self.nf {
            let z = (i as f64 + 0.5) * dz;
            // tension stiffening only in the effective zone around the bottom bars (7.5 db)
            let f = concrete(strain(z), ts && z < 187.5) * B * dz;
            n += f;
            m -= f * (z - H / 2.0);
        }
        let eb = strain(60.0);
        let sb = steel(eb);
        for (a, z) in [(1500.0, 60.0), (400.0, 550.0)] {
            let f = steel(strain(z)) * a;
            n += f;
            m -= f * (z - H / 2.0);
        }
        (n, m, eb, sb)
    }
}

fn main() {
    let sec = Sec { nf: 600 };
    let args: Vec<String> = std::env::args().collect();
    let ts = !args.iter().any(|a| a == "nots");
    let v_kn: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    println!("V = {} kN, tension stiffening {}", v_kn, if ts { "on" } else { "off" });
    println!("theta(deg)  Nv(kN)  Mpeak(kNm)  kappa(rad/km)  top(mm/m)  bar(mm/m)  bar stress  end of curve");
    for theta_deg in [90.0, 70.0, 65.0, 60.0, 50.0, 45.0, 35.0] {
        let cot = 1.0 / (theta_deg as f64).to_radians().tan();
        let nv = v_kn * 1000.0 * cot; // N, web longitudinal compression; fibres carry it in tension
        let mut best = (f64::MIN, 0.0, 0.0, 0.0, 0.0);
        let mut ended = "curvature range";
        let mut k = 0.5e-6;
        while k < 120e-6 {
            // top strain so that the fibres carry net tension nv (bisection on the top strain)
            let (mut lo, mut hi) = (-0.02, 0.005);
            let f = |et: f64| sec.forces(et, k, ts).0 - nv;
            if f(lo) > 0.0 || f(hi) < 0.0 {
                ended = "no axial balance";
                break;
            }
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if f(mid) > 0.0 { hi = mid } else { lo = mid }
            }
            let et = 0.5 * (lo + hi);
            let (_, m, eb, sb) = sec.forces(et, k, ts);
            // the web's compression acts about mid-depth of the web between the chords; its
            // moment about mid-depth is taken as zero (its centre is within a few cm of it)
            let mm = m / 1e6;
            if eb > 0.080 {
                ended = "bar rupture";
                break;
            }
            if mm > best.0 {
                best = (mm, k * 1e6, et * 1e3, eb * 1e3, sb);
            }
            if mm < 0.8 * best.0 {
                ended = "past the peak";
                break;
            }
            k *= 1.02;
        }
        println!(
            "{:9.0}  {:6.1}  {:10.1}  {:13.1}  {:9.2}  {:9.2}  {:10.0}  {}",
            theta_deg, nv / 1000.0, best.0, best.1, best.2, best.3, best.4, ended
        );
    }
}
