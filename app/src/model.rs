//! The cross-section as the person defines it (units: mm, MPa, kN, kNm, strains in mm/m where noted).

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Softening {
    None,
    VecchioCollins1986,
    Default2011, // the program's default (type 18)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BaseCurve {
    Parabolic,
    Popovics,
}

#[derive(Clone, Debug)]
pub struct Concrete {
    pub name: String,
    pub fc: f64,
    pub ft: Option<f64>,
    pub e0: Option<f64>, // mm/m, positive
    pub agg: f64,
    pub tsfactor: f64,
    pub softening: Softening,
    pub base: BaseCurve,
    pub tension_stiffening: bool,
    /// Designer's limit on the concrete compressive strain (mm/m, positive): where a run reaches
    /// it is marked on the M-V and moment-curvature plots. Not an analysis cut-off.
    pub ecu: f64,
}

impl Concrete {
    pub fn new(name: &str, fc: f64) -> Self {
        Concrete {
            name: name.into(),
            fc,
            ft: None,
            e0: None,
            agg: 19.0,
            tsfactor: 1.0,
            softening: Softening::Default2011,
            base: BaseCurve::Popovics,
            tension_stiffening: true,
            ecu: 3.5,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rebar {
    pub name: String,
    pub fy: f64,
    pub fu: f64,
    pub es: f64,
    pub esh: f64, // mm/m
    pub eu: f64,  // mm/m
    /// Designer's limit on the bar strain (mm/m): None = the start of strain hardening (esh).
    /// Marked on the plots, not an analysis cut-off.
    pub e_lim: Option<f64>,
}

impl Rebar {
    pub fn new(name: &str, fy: f64) -> Self {
        Rebar { name: name.into(), fy, fu: 600.0, es: 200000.0, esh: 15.0, eu: 80.0, e_lim: None }
    }
}

#[derive(Clone, Debug)]
pub struct Prestress {
    pub name: String,
    pub fpu: f64,
    pub ep: f64,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub eu: f64, // mm/m
}

impl Prestress {
    pub fn new(name: &str, fpu: f64) -> Self {
        Prestress { name: name.into(), fpu, ep: 200000.0, a: 0.025, b: 118.0, c: 10.0, eu: 43.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect { b: f64, h: f64 },
    Tee { bf: f64, hf: f64, bw: f64, h: f64 },
    IBeam { bt: f64, ht: f64, bw: f64, bb: f64, hb: f64, h: f64 },
    Circle { d: f64 },
    Custom,
}

impl Shape {
    pub fn label(&self) -> &'static str {
        match self {
            Shape::Rect { .. } => "Rectangle",
            Shape::Tee { .. } => "T-beam",
            Shape::IBeam { .. } => "I-beam",
            Shape::Circle { .. } => "Circle",
            Shape::Custom => "Custom outline",
        }
    }

    /// Width profile as (z from bottom, width) points, bottom to top.
    pub fn outline(&self) -> Vec<(f64, f64)> {
        match *self {
            Shape::Rect { b, h } => vec![(0.0, b), (h, b)],
            Shape::Tee { bf, hf, bw, h } => vec![(0.0, bw), (h - hf, bw), (h - hf, bf), (h, bf)],
            Shape::IBeam { bt, ht, bw, bb, hb, h } => vec![
                (0.0, bb),
                (hb, bb),
                (hb, bw),
                (h - ht, bw),
                (h - ht, bt),
                (h, bt),
            ],
            Shape::Circle { d } => {
                // the original's circle: 12 straight pieces (every 15 degrees), 0.1 mm wide at
                // the top and bottom points (its Quick Define, shots/cases/quick-circ.rsp)
                let r = d / 2.0;
                (0..=12)
                    .map(|i| {
                        let th = std::f64::consts::PI * (12 - i) as f64 / 12.0;
                        let z = r + r * th.cos();
                        ((z * 10.0).round() / 10.0, ((d * th.sin()).max(0.1) * 10.0).round() / 10.0)
                    })
                    .collect()
            }
            Shape::Custom => vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub struct LongLayer {
    pub name: String,
    pub z: f64,
    pub num: u32,
    pub area: f64, // total area of the layer, mm2
    pub steel: String,
    pub bartitle: String,
    pub dep: f64, // locked-in prestrain, mm/m
    /// Bars on a ring (the original's file "pattern 1"): the ring's number of bars ("nround",
    /// 0 when the layer is a plain row) and which ring it belongs to ("index").
    pub ring: u32,
    pub ring_id: u32,
    /// the layer's position is given as a distance from the top face (h - z) instead of its
    /// height from the bottom; z stays the height either way (this program's own setting)
    pub from_top: bool,
}

impl LongLayer {
    pub fn bar_dia(&self) -> f64 {
        let n = self.num.max(1) as f64;
        (4.0 * self.area / (n * std::f64::consts::PI)).sqrt()
    }
    /// The nominal diameter for drawing: the bar size's own (25M 25.2 mm) when known.
    pub fn draw_dia(&self) -> f64 {
        bar_db_csa(&self.bartitle).unwrap_or_else(|| match self.bartitle.trim() {
            "S13" => 12.7,
            "S15" => 15.2,
            _ => self.bar_dia(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Stirrups {
    pub name: String,
    pub area: f64, // area of one stirrup (all legs), mm2
    pub steel: String,
    pub spacing: f64,
    pub z_top: f64,
    pub z_bot: f64,
    pub db: f64,
    pub bartitle: String,
    /// The original's stirrup type ("pattern" in its files): 1 single leg, 2 open stirrup,
    /// 3 closed stirrup, 4 hoop, 5 T-headed single leg, 6 interlocking hoops.
    pub pattern: u32,
}

/// Stirrup types in the order of the original's Quick Define list (index = file "pattern").
pub const STIRRUP_TYPES: [&str; 7] = ["None", "Single Leg", "Open Stirrup", "Closed Stirrup", "Hoop", "T Headed single leg", "Interlocking Hoops"];

/// Legs of one stirrup of each type (area = legs x bar area).
pub fn stirrup_legs(pattern: u32) -> f64 {
    match pattern {
        1 | 5 => 1.0,
        // each of the two interlocking hoops covers its own part of the depth (2 legs there)
        6 => 2.0,
        _ => 2.0,
    }
}

#[derive(Clone, Debug)]
pub struct Loads {
    pub axial: f64,    // kN, tension positive
    pub mv_ratio: f64, // m (moment per unit shear of the increment)
    pub negative: bool,
    /// Constant moment applied first and held (kNm, + puts the bottom in tension), as in the
    /// original's "Constant + Increment" loading; the increment (mv_ratio, negative) is added on
    /// top of it until the section fails.
    pub m_const: f64,
    /// Constant shear (kN), and the increments of axial load, moment and shear (kN, kNm, kN):
    /// loads = constant + load factor x increment, as in the original (file BV, MN, MM, MV).
    pub v_const: f64,
    pub n_inc: f64,
    pub m_inc: f64,
    pub v_inc: f64,
}

impl Loads {
    pub fn new(axial: f64, mv_ratio: f64, negative: bool) -> Self {
        let m_inc = if negative { -mv_ratio } else { mv_ratio };
        Loads { axial, mv_ratio, negative, m_const: 0.0, v_const: 0.0, n_inc: 0.0, m_inc, v_inc: 1.0 }
    }
    /// Keep the derived ratio and moment direction in step with the increments.
    pub fn sync(&mut self) {
        self.mv_ratio = if self.v_inc.abs() > 1e-12 { (self.m_inc / self.v_inc).abs() } else { 1000.0 };
        self.negative = if self.m_inc.abs() > 1e-12 { self.m_inc < 0.0 } else { self.m_const < 0.0 };
    }
    /// Constants and increments in the analysis frame (the section is mirrored for negative
    /// moment, so moments change sign there; shear is taken by its size).
    pub fn model_spec(&self) -> ((f64, f64, f64), (f64, f64, f64)) {
        let s = if self.negative { -1.0 } else { 1.0 };
        ((self.axial, s * self.m_const, self.v_const.abs()), (self.n_inc, s * self.m_inc, self.v_inc.abs()))
    }
    /// The simple case the M-V tools use: no constants but axial load, no axial increment.
    pub fn is_plain_ratio(&self) -> bool {
        self.m_const.abs() < 1e-9 && self.v_const.abs() < 1e-9 && self.n_inc.abs() < 1e-12 && self.v_inc.abs() > 1e-12
    }
    /// Increment of moment per kN of shear increment (kNm/kN = m), signed.
    pub fn inc_ratio(&self) -> f64 {
        if self.negative { -self.mv_ratio } else { self.mv_ratio }
    }
    /// The constant moment in the analysis frame (the section is mirrored for negative moment).
    pub fn m_const_model(&self) -> f64 {
        if self.negative { -self.m_const } else { self.m_const }
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    pub title: String,
    pub done_by: String,
    pub date: String,
    pub concretes: Vec<Concrete>,
    pub rebars: Vec<Rebar>,
    pub prestress: Vec<Prestress>,
    pub shape: Shape,
    pub outline: Vec<(f64, f64)>,
    pub long: Vec<LongLayer>,
    pub stirrups: Option<Stirrups>,
    pub loads: Loads,
    pub member: Option<Member>,
    /// material (resistance) factors [concrete, reinforcing steel, prestressing steel], as the
    /// original's Options > General "Material Factors" (globals 0x569f00/08/10): 1, 1, 1 by
    /// default, CSA A23.3 0.65, 0.85, 0.90. Any factor below 1 runs the whole analysis with
    /// factored materials (see materials.rs)
    pub phi: [f64; 3],
    /// ACI 318 strength reduction factors: the analysis stays nominal (unfactored materials) and
    /// the design curves are the nominal ones times phi (see design.rs). None: not used
    pub aci: Option<AciFactors>,
}

/// Strength reduction (resistance) factors on the nominal response, ACI 318-19 (Table 21.2.1,
/// 21.2.2, 22.4.2.1) or AASHTO LRFD (9th ed., 5.5.4.2, 5.6.2.1, 5.6.4.4). The same structure
/// holds both: `code` 0 ACI 318, 1 AASHTO LRFD.
#[derive(Clone, Debug, PartialEq)]
pub struct AciFactors {
    /// tension-controlled sections (moment, axial with moment); reinforced concrete
    pub phi_tension: f64,
    /// compression-controlled, with ties (other) / with spirals
    pub phi_tied: f64,
    pub phi_spiral: f64,
    /// the section's transverse reinforcement is a spiral (ACI 25.7.3)
    pub spiral: bool,
    /// shear
    pub phi_shear: f64,
    /// concrete strain at nominal strength, at the extreme compression fibre
    pub ecu: f64,
    /// ACI: tension-controlled once the net tensile strain passes ety + this
    pub tc_extra: f64,
    /// maximum axial compression: Pn,max = alpha P0, alpha 0.80 with ties, 0.85 with spirals
    pub pmax_tied: f64,
    pub pmax_spiral: f64,
    /// 0 ACI 318, 1 AASHTO LRFD
    pub code: u8,
    /// tension-controlled prestressed sections (AASHTO 1.00; ACI 0.90, the same as reinforced)
    pub phi_tension_ps: f64,
    /// torsion (ACI 0.75, AASHTO 0.90)
    pub phi_torsion: f64,
}

impl Default for AciFactors {
    fn default() -> Self {
        AciFactors {
            phi_tension: 0.90,
            phi_tied: 0.65,
            phi_spiral: 0.75,
            spiral: false,
            phi_shear: 0.75,
            ecu: 0.003,
            tc_extra: 0.003,
            pmax_tied: 0.80,
            pmax_spiral: 0.85,
            code: 0,
            phi_tension_ps: 0.90,
            phi_torsion: 0.75,
        }
    }
}

impl AciFactors {
    /// AASHTO LRFD (9th edition, 5.5.4.2): tension-controlled 0.90 reinforced / 1.00
    /// prestressed, compression-controlled 0.75 with ties or spirals, shear and torsion 0.90
    /// (normal-weight concrete), Pn,max 0.80 / 0.85 P0 (5.6.4.4), ecu 3.0 mm/m (5.6.2.1).
    pub fn aashto() -> Self {
        AciFactors {
            phi_tension: 0.90,
            phi_tied: 0.75,
            phi_spiral: 0.75,
            spiral: false,
            phi_shear: 0.90,
            ecu: 0.003,
            tc_extra: 0.003,
            pmax_tied: 0.80,
            pmax_spiral: 0.85,
            code: 1,
            phi_tension_ps: 1.00,
            phi_torsion: 0.90,
        }
    }
    pub fn name(&self) -> &'static str {
        if self.code == 1 { "AASHTO LRFD" } else { "ACI 318" }
    }
    /// The factors for one section: AASHTO's tension-controlled factor is 1.00 for a
    /// prestressed section (one with prestressing steel).
    pub fn for_section(&self, sec: &Section) -> AciFactors {
        let mut a = self.clone();
        let ps = sec.long.iter().any(|l| sec.find_prestress(&l.steel).is_some());
        if ps {
            a.phi_tension = a.phi_tension_ps;
        }
        a
    }
    /// phi for a compression-controlled section (ties or spiral).
    pub fn phi_compression(&self) -> f64 {
        if self.spiral { self.phi_spiral } else { self.phi_tied }
    }
    /// The tension-controlled strain limit for steel with yield strain ety (2 mm/m means
    /// prestressing steel). ACI 318-19 Table 21.2.2: ety + 0.003. AASHTO 5.6.2.1: 0.005 for
    /// reinforcement up to 517 MPa (75 ksi) and for prestressing steel, rising linearly to 0.008
    /// at 690 MPa (100 ksi).
    pub fn tension_limit(&self, ety: f64) -> f64 {
        if self.code == 1 {
            let fy = ety * 200000.0;
            if (ety - 0.002).abs() < 1e-12 || fy <= 517.0 {
                0.005
            } else {
                0.005 + 0.003 * ((fy - 517.0) / (690.0 - 517.0)).clamp(0.0, 1.0)
            }
        } else {
            ety + self.tc_extra
        }
    }
    /// phi from the net tensile strain et of the extreme tension steel at nominal strength; ety
    /// its yield strain (fy/Es, 0.002 for prestressing steel): compression-controlled up to ety
    /// (AASHTO's compression-controlled limit), tension-controlled from tension_limit, linear
    /// between (ACI 318-19 Table 21.2.2, AASHTO LRFD 5.5.4.2).
    pub fn phi_flexure(&self, et: f64, ety: f64) -> f64 {
        let pc = self.phi_compression();
        let etl = self.tension_limit(ety).max(ety + 1e-9);
        if et <= ety {
            pc
        } else if et >= etl {
            self.phi_tension
        } else {
            pc + (self.phi_tension - pc) * (et - ety) / (etl - ety)
        }
    }
    pub fn pmax_alpha(&self) -> f64 {
        if self.spiral { self.pmax_spiral } else { self.pmax_tied }
    }
}
/// The original's test for a factored run: |phic + phis + phip - 3| > 0.001 (0x52e028, 0x52e030).
pub fn is_factored(phi: &[f64; 3]) -> bool {
    (phi[0] + phi[1] + phi[2] - 3.0).abs() > 0.001
}

pub const PHI_CSA: [f64; 3] = [0.65, 0.85, 0.90];

#[derive(Clone, Debug)]
pub struct Member {
    pub length: f64,     // mm
    pub left_sup: f64,   // mm
    pub right_sup: f64,  // mm
    pub load_at: f64,    // mm, point load position
}

pub fn bar_area_csa(title: &str) -> Option<f64> {
    let t = title.trim().to_uppercase();
    let a = match t.as_str() {
        "10M" => 100.0,
        "15M" => 200.0,
        "20M" => 300.0,
        "25M" => 500.0,
        "30M" => 700.0,
        "35M" => 1000.0,
        "45M" => 1500.0,
        "55M" => 2500.0,
        _ => return None,
    };
    Some(a)
}

pub const BAR_TITLES: [&str; 8] = ["10M", "15M", "20M", "25M", "30M", "35M", "45M", "55M"];

/// Nominal bar diameters (mm) of the Canadian bar sizes, as the original places bars with them
/// (its Quick Define puts a 25M centre at cover + stirrup + 25.2/2).
pub fn bar_db_csa(title: &str) -> Option<f64> {
    let t = title.trim().to_uppercase();
    Some(match t.as_str() {
        "10M" => 11.3,
        "15M" => 16.0,
        "20M" => 19.5,
        "25M" => 25.2,
        "30M" => 29.9,
        "35M" => 35.7,
        "45M" => 43.7,
        "55M" => 56.4,
        _ => return None,
    })
}

/// Prestressing choices of the original's Quick Define, step 1 (index 0 = none).
pub const STRAND_TYPES: [&str; 5] = [
    "None",
    "1860 MPa Low Relaxation 13 mm strands",
    "1860 MPa Stress Relieved 13 mm strands",
    "1860 MPa Low Relaxation 15 mm strands",
    "1860 MPa Stress Relieved 15 mm strands",
];

/// Everything the original's Quick Define asks for (4 steps), with its defaults.
#[derive(Clone, Debug)]
pub struct QuickDefine {
    pub title: String,
    pub done_by: String,
    pub fc: f64,
    pub fy: f64,
    pub fyt: f64,
    /// index into STRAND_TYPES
    pub strand_type: usize,
    /// 0 rectangle, 1 circle, 2 T-beam, 3 I-beam, 4 column (a rectangle with bars on all four
    /// faces; this program's addition to the original's list)
    pub shape: usize,
    /// b, h (rectangle); d (circle); btop, h, bweb, ttop (T); btop, h, bweb, ttop, tbot, bbot (I)
    pub dims: [f64; 6],
    /// top bars (circle: outer ring): number, designation, or the area of one bar when by_area
    pub top_n: u32,
    pub top_bar: String,
    pub top_by_area: bool,
    pub top_area: f64,
    /// bottom bars (circle: inner ring)
    pub bot_n: u32,
    pub bot_bar: String,
    pub bot_by_area: bool,
    pub bot_area: f64,
    /// side bars (this program's addition; not circles): bars on each side face, spread evenly
    /// between the top and bottom rows, one bar a face in each layer (2 a layer)
    pub side_n: u32,
    pub side_bar: String,
    pub side_by_area: bool,
    pub side_area: f64,
    /// index into STIRRUP_TYPES
    pub stirrup_type: usize,
    pub st_bar: String,
    pub st_by_area: bool,
    pub st_area: f64,
    pub st_space: f64,
    pub cover: f64,
    pub strands: u32,
    pub dep: f64,
}

impl Default for QuickDefine {
    fn default() -> Self {
        QuickDefine {
            title: "New Cross Section".into(),
            done_by: String::new(),
            fc: 35.0,
            fy: 400.0,
            fyt: 400.0,
            strand_type: 1,
            shape: 0,
            dims: [250.0, 600.0, 0.0, 0.0, 0.0, 0.0],
            top_n: 1,
            top_bar: "20M".into(),
            top_by_area: false,
            top_area: 300.0,
            bot_n: 3,
            bot_bar: "25M".into(),
            bot_by_area: false,
            bot_area: 500.0,
            side_n: 0,
            side_bar: "20M".into(),
            side_by_area: false,
            side_area: 300.0,
            stirrup_type: 3,
            st_bar: "10M".into(),
            st_by_area: false,
            st_area: 100.0,
            st_space: 250.0,
            cover: 40.0,
            strands: 0,
            dep: 6.5,
        }
    }
}

impl QuickDefine {
    /// The step-2 parameter labels the original shows for a shape.
    pub fn dim_labels(shape: usize) -> &'static [&'static str] {
        match shape {
            1 => &["d"],
            2 => &["btop", "h", "bweb", "ttop"],
            3 => &["btop", "h", "bweb", "ttop", "tbot", "bbot"],
            _ => &["b", "h"],
        }
    }

    /// Area and diameter of one bar from a designation or an area.
    fn bar(title: &str, by_area: bool, area: f64) -> Result<(f64, f64, String), String> {
        if by_area {
            if area <= 0.0 {
                return Err("Give the bar area (mm²).".into());
            }
            return Ok((area, (4.0 * area / std::f64::consts::PI).sqrt(), String::new()));
        }
        let t = title.trim().to_uppercase();
        match (bar_area_csa(&t), bar_db_csa(&t)) {
            (Some(a), Some(d)) => Ok((a, d, t)),
            _ => Err(format!("'{}' is not a bar designation this program knows (10M to 55M), or tick 'Select bar by area'.", title.trim())),
        }
    }
}

impl Section {
    /// Build a section the way the original's Quick Define does (r2k 0x476eb0 and the routines
    /// it calls; checked against files the original saved: shots/cases/quick-*.rsp):
    /// * materials "Concrete 1" (aggregate 19 mm), "Steel 1" for the bars and "Trans" for the
    ///   stirrups, both with esh 7 and eu 100 mm/m; the strand material "Low Relax" or
    ///   "Relieved" (A 0.03, B 121, C 6) at 1860 MPa;
    /// * bars in rows from the stirrup's inner face: first centre at clear cover + stirrup
    ///   diameter + db/2; a row holds floor(((b - 2 cover - 2 db_stirrup)/db + 1)/2) bars (clear
    ///   spacing about one diameter), at least one; further rows 2 db further in, named
    ///   "bot", "bot<k>" / "top", "top<k>" with k one more than the layers so far (0x475cf0,
    ///   0x476710);
    /// * circles: bars on rings, the outer at radius d/2 - cover - stirrup - db/2, bar i at angle
    ///   2 pi i / n from the top, mirrored pairs in one layer, named "outer", "outer<i>"; the
    ///   inner ring at d/2 - cover - 2 stirrup - 50 mm - 1.5 db, "inner", "inner<i>";
    /// * strands (13 or 15 mm) in rows 50.8 mm apart from the stirrup's inner face,
    ///   round((b - 2 cover - 2 db_stirrup)/50.8) + 1 a row, named "pbot", with locked-in
    ///   strain dep (0x4764a0);
    /// * the stirrup between its legs' centrelines (cover + db/2 from each face), legs by type;
    /// * a circle is the original's 12-sided outline; loading the original's default
    ///   (moment only); a member of length 13 h / 3, support at h / 3, load at 3 h.
    /// * a column (shape 4) is a rectangle; side bars (any shape but the circle) go on both side
    ///   faces, spread evenly between the first top and first bottom rows (this program's
    ///   addition), named "side", "side<k>".
    pub fn from_quick(q: &QuickDefine) -> Result<Section, String> {
        let shape = match q.shape {
            1 => Shape::Circle { d: q.dims[0] },
            2 => Shape::Tee { bf: q.dims[0], hf: q.dims[3], bw: q.dims[2], h: q.dims[1] },
            3 => Shape::IBeam { bt: q.dims[0], ht: q.dims[3], bw: q.dims[2], bb: q.dims[5], hb: q.dims[4], h: q.dims[1] },
            _ => Shape::Rect { b: q.dims[0], h: q.dims[1] },
        };
        let needed = QuickDefine::dim_labels(q.shape).len();
        if q.dims[..needed].iter().any(|v| *v <= 0.0) {
            return Err(format!("Give every dimension of the {} (none can be 0).", shape.label()));
        }
        let outline = shape.outline();
        let h = outline.last().map(|p| p.0).unwrap_or(0.0);
        let mut c = Concrete::new("Concrete 1", q.fc);
        c.agg = 19.0;
        let mut r = Rebar::new("Steel 1", q.fy);
        r.esh = 7.0;
        r.eu = 100.0;
        let mut rt = Rebar::new("Trans", q.fyt);
        rt.esh = 7.0;
        rt.eu = 100.0;
        let relieved = q.strand_type == 2 || q.strand_type == 4;
        let mut p = Prestress::new(if relieved { "Relieved" } else { "Low Relax" }, 1860.0);
        if relieved {
            p.a = 0.03;
            p.b = 121.0;
            p.c = 6.0;
            p.eu = 43.0;
        }
        let mut sec = Section {
            title: q.title.clone(),
            done_by: q.done_by.clone(),
            date: String::new(),
            concretes: vec![c],
            rebars: vec![r, rt],
            prestress: vec![p.clone()],
            shape: shape.clone(),
            outline,
            long: vec![],
            stirrups: None,
            loads: Loads { axial: 0.0, mv_ratio: 1000.0, negative: false, m_const: 0.0, v_const: 0.0, n_inc: 0.0, m_inc: 1.0, v_inc: 0.0 },
            member: Some(Member { length: 13.0 * h / 3.0, left_sup: h / 3.0, right_sup: 13.0 * h / 3.0, load_at: 3.0 * h }),
            phi: [1.0; 3],
            aci: None,
        };
        sec.loads.sync();
        let cover = q.cover;
        let pattern = q.stirrup_type as u32;
        // the bars are placed from the (first) stirrup set's top and bottom (0x476710, 0x475cf0)
        let mut bar_top = h - cover;
        let mut bar_bot = cover;
        let st_db = if pattern > 0 {
            let (a, d, t) = QuickDefine::bar(&q.st_bar, q.st_by_area, q.st_area)?;
            let (top, bot) = (h - cover - d / 2.0, cover + d / 2.0);
            // a hoop sits on the middle of the section, as big as the width allows; interlocking
            // hoops are two of them, one against the top cover and one against the bottom
            // (0x476eb0, stirrup types 4 and 6)
            let half_w = sec.width_at(0.5 * (top + bot)) / 2.0 - cover - d / 2.0;
            let (z_top, z_bot, first) = match pattern {
                4 => {
                    let c = 0.5 * (top + bot);
                    let r = ((top - bot) / 2.0).min(half_w);
                    (c + r, c - r, (c + r, c - r))
                }
                6 => {
                    let r = ((top - bot) / 2.0).min(half_w);
                    (top, bot, (top, top - 2.0 * r))
                }
                _ => (top, bot, (top, bot)),
            };
            bar_top = first.0 - d / 2.0;
            bar_bot = first.1 + d / 2.0;
            sec.stirrups = Some(Stirrups {
                name: if pattern == 4 || pattern == 6 { "hoop".into() } else { "stirrup".into() },
                area: stirrup_legs(pattern) * a,
                steel: "Trans".into(),
                spacing: q.st_space.max(1.0),
                z_top,
                z_bot,
                db: d,
                bartitle: t,
                pattern,
            });
            d
        } else {
            0.0
        };
        let mut push = |sec: &mut Section, name: String, z: f64, n: u32, a: f64, t: &str| {
            sec.long.push(LongLayer { name, z, num: n, area: n as f64 * a, steel: "Steel 1".into(), bartitle: t.into(), dep: 0.0, ring: 0, ring_id: 0, from_top: false });
        };
        if q.shape == 1 {
            let d = q.dims[0];
            let ring = |sec: &mut Section, push: &mut dyn FnMut(&mut Section, String, f64, u32, f64, &str), n: u32, radius: f64, a: f64, t: &str, base: &str| {
                if n == 0 || radius <= 0.0 {
                    return;
                }
                let step = 2.0 * std::f64::consts::PI / n as f64;
                for i in 0..n {
                    let th = i as f64 * step;
                    if th - std::f64::consts::PI > 1e-4 {
                        break;
                    }
                    let single = th.abs() < 1e-4 || (th - std::f64::consts::PI).abs() < 1e-4;
                    let name = if i == 0 { base.to_string() } else { format!("{}{}", base, i) };
                    push(sec, name, d / 2.0 + radius * th.cos(), if single { 1 } else { 2 }, a, t);
                    if let Some(l) = sec.long.last_mut() {
                        l.ring = n;
                        l.ring_id = if base == "outer" { 1 } else { 2 };
                    }
                }
            };
            if q.top_n > 0 {
                let (a, db, t) = QuickDefine::bar(&q.top_bar, q.top_by_area, q.top_area)?;
                ring(&mut sec, &mut push, q.top_n, d / 2.0 - cover - st_db - db / 2.0, a, &t, "outer");
            }
            if q.bot_n > 0 {
                let (a, db, t) = QuickDefine::bar(&q.bot_bar, q.bot_by_area, q.bot_area)?;
                ring(&mut sec, &mut push, q.bot_n, d / 2.0 - cover - 2.0 * st_db - 50.0 - 1.5 * db, a, &t, "inner");
            }
        } else {
            let w2 = if pattern > 0 { 2.0 * st_db } else { 0.2 };
            // top bars, rows downward
            if q.top_n > 0 {
                let (a, db, t) = QuickDefine::bar(&q.top_bar, q.top_by_area, q.top_area)?;
                let mut z = bar_top - db / 2.0;
                let mut left = q.top_n;
                let mut first = true;
                while left > 0 && z > 0.0 {
                    let fit = ((((sec.width_at(z) - 2.0 * cover - w2) / db + 1.0) * 0.5).floor() as i64).max(1) as u32;
                    let n = fit.min(left);
                    let name = if first { "top".to_string() } else { format!("top{}", sec.long.len() + 1) };
                    push(&mut sec, name, z, n, a, &t);
                    // shown as a distance from the top face in the bar dialog
                    if let Some(l) = sec.long.last_mut() {
                        l.from_top = true;
                    }
                    first = false;
                    left -= n;
                    z -= 2.0 * db;
                }
            }
            if q.bot_n > 0 {
                let (a, db, t) = QuickDefine::bar(&q.bot_bar, q.bot_by_area, q.bot_area)?;
                let mut z = bar_bot + db / 2.0;
                let mut left = q.bot_n;
                let mut first = true;
                while left > 0 && z < h {
                    let fit = ((((sec.width_at(z) - 2.0 * cover - w2) / db + 1.0) * 0.5).floor() as i64).max(1) as u32;
                    let n = fit.min(left);
                    let name = if first { "bot".to_string() } else { format!("bot{}", sec.long.len() + 1) };
                    push(&mut sec, name, z, n, a, &t);
                    first = false;
                    left -= n;
                    z += 2.0 * db;
                }
            }
            // side bars: one on each side face per layer, evenly between the first top and the
            // first bottom row (without those rows, between the stirrup's inner faces)
            if q.side_n > 0 {
                let (a, db, t) = QuickDefine::bar(&q.side_bar, q.side_by_area, q.side_area)?;
                let zt = sec.long.iter().find(|l| l.name == "top").map(|l| l.z).unwrap_or(bar_top - db / 2.0);
                let zb = sec.long.iter().find(|l| l.name == "bot").map(|l| l.z).unwrap_or(bar_bot + db / 2.0);
                let step = (zt - zb) / (q.side_n as f64 + 1.0);
                if step < 2.0 * db {
                    return Err(format!(
                        "{} side bars a face don't fit between the top and bottom bars: they would be {:.0} mm apart, less than two bar diameters ({:.0} mm).",
                        q.side_n,
                        step.max(0.0),
                        2.0 * db
                    ));
                }
                for i in 1..=q.side_n {
                    let name = if i == 1 { "side".to_string() } else { format!("side{}", sec.long.len() + 1) };
                    push(&mut sec, name, zb + step * i as f64, 2, a, &t);
                }
            }
        }
        // strands (bottom tendons)
        if q.strands > 0 && q.strand_type > 0 {
            let big = q.strand_type >= 3;
            let (a, db, t) = if big { (140.0, 15.2, "S15") } else { (99.0, 12.7, "S13") };
            let pitch = 50.8;
            let mut z = bar_bot + db / 2.0;
            let mut left = q.strands;
            let mut first = true;
            let w2 = 2.0 * st_db;
            while left > 0 && z < h {
                let fit = ((sec.width_at(z) - 2.0 * cover - w2) / pitch + 0.5 + 1.0).floor().max(1.0) as u32;
                let n = fit.min(left);
                let name = if first { "pbot".to_string() } else { format!("pbot{}", sec.long.len() + 1) };
                sec.long.push(LongLayer { name, z, num: n, area: n as f64 * a, steel: p.name.clone(), bartitle: t.into(), dep: q.dep, ring: 0, ring_id: 0, from_top: false });
                first = false;
                left -= n;
                z += pitch;
            }
        }
        Ok(sec)
    }
}

impl Section {
    pub fn quick(
        title: &str,
        fc: f64,
        agg: f64,
        fy: f64,
        shape: Shape,
        bottom: (u32, &str, f64),
        top: Option<(u32, &str, f64)>,
        stirrup: Option<(&str, f64, f64)>, // bar, spacing, cover
    ) -> Section {
        let mut c = Concrete::new("Concrete", fc);
        c.agg = agg;
        let r = Rebar::new("Long steel", fy);
        let mut long = vec![];
        let h = shape.outline().last().map(|p| p.0).unwrap_or(0.0);
        let (n, t, cz) = bottom;
        long.push(LongLayer {
            name: "bottom".into(),
            z: cz,
            num: n,
            area: n as f64 * bar_area_csa(t).unwrap_or(500.0),
            steel: "Long steel".into(),
            bartitle: t.into(),
            dep: 0.0, ring: 0, ring_id: 0, from_top: false,
        });
        if let Some((n, t, cz)) = top {
            long.push(LongLayer {
                name: "top".into(),
                z: h - cz,
                num: n,
                area: n as f64 * bar_area_csa(t).unwrap_or(200.0),
                steel: "Long steel".into(),
                bartitle: t.into(),
                dep: 0.0, ring: 0, ring_id: 0, from_top: false,
            });
        }
        let stirrups = stirrup.map(|(t, s, cover)| {
            let a1 = bar_area_csa(t).unwrap_or(100.0);
            Stirrups {
                name: "stirrups".into(),
                area: 2.0 * a1,
                steel: "Long steel".into(),
                spacing: s,
                z_top: h - cover,
                z_bot: cover,
                db: (4.0 * a1 / std::f64::consts::PI).sqrt(),
                bartitle: t.into(),
                pattern: 2,
            }
        });
        Section {
            title: title.into(),
            done_by: String::new(),
            date: String::new(),
            concretes: vec![c],
            rebars: vec![r],
            prestress: vec![Prestress::new("P-Steel", 1860.0)],
            outline: shape.outline(),
            shape,
            long,
            stirrups,
            loads: Loads::new(0.0, 1.5, false),
            member: None,
            phi: [1.0; 3],
            aci: None,
        }
    }

    /// The section is the same upside down (within 0.1 mm and 0.1 % of an area): outline, bar
    /// layers and stirrup zone mirror about mid-height. Used only to measure how closely the
    /// solvers agree between positive and negative moment (`--bench-mv`); results are never
    /// copied from one side to the other.
    pub fn is_symmetric(&self) -> bool {
        let h = self.height();
        if h <= 0.0 {
            return false;
        }
        let tol = 0.1;
        let mut zs: Vec<f64> = (0..=40).map(|i| h * i as f64 / 80.0).collect();
        zs.extend(self.outline.iter().flat_map(|p| [p.0 + 0.2, p.0 - 0.2]).filter(|z| *z > 0.0 && *z < h));
        if zs.iter().any(|&z| (self.width_at(z) - self.width_at(h - z)).abs() > tol) {
            return false;
        }
        let twin = |l: &LongLayer| {
            self.long.iter().any(|o| {
                (o.z - (h - l.z)).abs() <= tol && (o.area - l.area).abs() <= 1e-3 * l.area.max(1.0) && o.steel == l.steel && (o.dep - l.dep).abs() < 1e-9 && o.num == l.num
            })
        };
        if !self.long.iter().all(twin) {
            return false;
        }
        match &self.stirrups {
            Some(s) => (s.z_top + s.z_bot - h).abs() <= 2.0 * tol,
            None => true,
        }
    }

    pub fn height(&self) -> f64 {
        self.outline.iter().map(|p| p.0).fold(0.0, f64::max)
    }

    pub fn max_width(&self) -> f64 {
        self.outline.iter().map(|p| p.1).fold(0.0, f64::max)
    }

    /// Width at height z (linear between outline points; at a step the upper part wins when going up).
    pub fn width_at(&self, z: f64) -> f64 {
        let o = &self.outline;
        if o.len() < 2 {
            return o.first().map(|p| p.1).unwrap_or(0.0);
        }
        if z < o[0].0 || z > o[o.len() - 1].0 {
            return 0.0;
        }
        for w in o.windows(2) {
            let (z1, b1) = w[0];
            let (z2, b2) = w[1];
            if z >= z1 && z <= z2 && z2 > z1 {
                return b1 + (b2 - b1) * (z - z1) / (z2 - z1);
            }
        }
        o[o.len() - 1].1
    }

    pub fn gross_area(&self) -> f64 {
        let n = 400;
        let h = self.height();
        (0..n)
            .map(|i| self.width_at((i as f64 + 0.5) * h / n as f64) * h / n as f64)
            .sum()
    }

    pub fn find_rebar(&self, name: &str) -> Option<&Rebar> {
        self.rebars.iter().find(|r| r.name == name)
    }
    pub fn find_prestress(&self, name: &str) -> Option<&Prestress> {
        self.prestress.iter().find(|r| r.name == name)
    }
}
