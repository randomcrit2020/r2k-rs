//! The main window: menu, toolbar, section panel, result views and editing dialogs.

use crate::analysis::{self, *};
use crate::model::*;
use crate::plots::{self, Series, BLUE, GREY, RED};
use crate::rsp;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, StrokeKind, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

#[derive(PartialEq, Clone, Copy)]
enum View {
    Welcome,
    Section,
    Nine,
    MPhi,
    Shear,
    Interaction,
    Member,
    Cases,
    Radial,
    Biaxial,
    Shear2d,
    LoadCases,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Flexure(usize),
    Shear(usize),
}

enum JobMsg {
    Progress(f32, String),
    Flexure(SModel, MPhiResult),
    Shear(SModel, ShearResult),
    Inter(Interaction),
    Member(MemberResult),
    Cases(Vec<CaseRow>),
    Radial(RadialDiagram),
    Biaxial(crate::biaxial::BiDiagram),
    Shear2d(crate::shear2d::VDiagram),
    LoadCases(Vec<crate::demand::CaseResult>),
    Error(String),
    Done,
}

#[derive(Clone, Copy, PartialEq)]
enum Job {
    Flexure,
    Shear,
    Interaction,
    Member,
    Cases,
    MN,
    NV,
    MVC,
    Biaxial,
    Shear2d,
    LoadCases,
}

/// One of the original's own sectional runs next to this program's result for the same loads.
#[derive(Clone)]
pub struct CaseRow {
    case: crate::compare::Case,
    ours: Option<(f64, f64)>,
    reason: String,
    secs: f64,
}

/// The Design Factors dialog being edited: code 0 nominal, 1 CSA material factors, 2 ACI 318.
#[derive(Clone)]
struct DesignForm {
    code: u8,
    phi: [f64; 3],
    aci: AciFactors,
}

/// The Quick Define wizard: the original's 4 steps (general and materials, concrete section,
/// longitudinal bars, transverse steel and tendons), with its fields and defaults.
#[derive(Default)]
struct QuickForm {
    q: QuickDefine,
    step: usize,
    error: Option<String>,
    /// the shape the circle defaults were last set for (the original switches the stirrup to a
    /// hoop and the bars to rings for a circle)
    last_shape: usize,
}

pub struct App {
    sections: Vec<Section>,
    sec: Option<Section>,
    path: Option<PathBuf>,
    dirty: bool,
    view: View,
    flex: Option<(SModel, MPhiResult)>,
    shear: Option<(SModel, ShearResult)>,
    inter: Option<Interaction>,
    member: Option<MemberResult>,
    stage: Option<Stage>,
    job: Option<(Job, Receiver<JobMsg>)>,
    progress: (f32, String),
    status: String,
    quick_open: bool,
    quick: QuickForm,
    mat_open: bool,
    shape_open: bool,
    long_open: bool,
    trans_open: bool,
    loads_open: bool,
    about_open: bool,
    /// the Material Factors dialog: open, and the values being edited (concrete, steel, prestress)
    factors: Option<DesignForm>,
    /// ACI 318 plane-sections models for the design curves (built on first use)
    aci_models: Option<std::rc::Rc<crate::design::AciModels>>,
    pick_open: bool,
    error: Option<String>,
    shot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    mv_points: usize,
    /// which extra layers the M-V interaction plot draws on top of the strength envelope
    /// (all off by default: only the envelope is shown)
    mv_show: MvShow,
    /// results from the original program to compare the M-V envelope with (None: offer the
    /// built-in sample-beam readings when the section is the sample beam)
    cases: Option<Vec<CaseRow>>,
    radial: Option<RadialDiagram>,
    /// biaxial bending: the result, and its axial load (kN), load angle (degrees) and number of directions
    biax: Option<crate::biaxial::BiDiagram>,
    bi_n: f64,
    bi_alpha: f64,
    bi_dirs: usize,
    /// shear in two directions: the result, its axial load (kN), M/V (m), directions, and the
    /// demand (Vux, Vuy) to check (kN)
    s2: Option<crate::shear2d::VDiagram>,
    s2_n: f64,
    s2_mv: f64,
    s2_dirs: usize,
    s2_vu: (f64, f64),
    /// load cases: the pasted table and the results
    lc_text: String,
    lc_rows: Option<Vec<crate::demand::CaseResult>>,
    started: std::time::Instant,
}

/// Optional layers of the M-V interaction plot; the strength envelope is always drawn.
#[derive(Clone, Copy, Default)]
struct MvShow {
    cracking: bool,
    long_yield: bool,
    trans_yield: bool,
    conc_limit: bool,
    steel_limit: bool,
    design: bool,
    points: bool,
    checks: bool,
    shear_peak: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>, open: Option<PathBuf>, solve: Option<String>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let mut app = App {
            sections: vec![],
            sec: None,
            path: None,
            dirty: false,
            view: View::Welcome,
            flex: None,
            shear: None,
            inter: None,
            member: None,
            stage: None,
            job: None,
            progress: (0.0, String::new()),
            status: "Ready".into(),
            quick_open: false,
            quick: QuickForm::default(),
            mat_open: false,
            shape_open: false,
            long_open: false,
            trans_open: false,
            loads_open: false,
            about_open: false,
            factors: None,
            aci_models: None,
            pick_open: false,
            error: None,
            shot,
            frames: 0,
            shot_requested: false,
            started: std::time::Instant::now(),
            mv_points: std::env::var("SR_POINTS").ok().and_then(|v| v.parse().ok()).unwrap_or(12),
            mv_show: MvShow::default(),
            cases: None,
            radial: None,
            biax: None,
            bi_n: 0.0,
            bi_alpha: 30.0,
            bi_dirs: 36,
            s2: None,
            s2_n: 0.0,
            s2_mv: 0.0,
            s2_dirs: 16,
            s2_vu: (100.0, 150.0),
            lc_text: "Case	Pu	Mx	My	Vx	Vy	Tu\nLC1	500	150	50	60	120	10\nLC2	1500	80	120	90	40	5\nLC3	0	250	0	0	180	20\n".into(),
            lc_rows: None,
        };
        if let Some(p) = open {
            app.open_path(p);
            app.pick_open = false;
        }
        match solve.as_deref() {
            Some("flexure") => app.start(Job::Flexure),
            Some("shear") => app.start(Job::Shear),
            Some("interaction") => app.start(Job::Interaction),
            Some("member") => app.start(Job::Member),
            Some("mn") => app.start(Job::MN),
            Some("nv") => app.start(Job::NV),
            // --solve biaxial: the Mx-My diagram at N = 0 and alpha = 30 degrees
            Some("biaxial") => app.start(Job::Biaxial),
            // --solve shear2d: the Vx-Vy shear diagram at N = 0, shear only
            Some("shear2d") => app.start(Job::Shear2d),
            // --solve cases: the load-case table's D/C ratios
            Some("cases") => app.start(Job::LoadCases),
            // --solve bars: open the Longitudinal Reinforcement dialog (for pictures)
            Some("bars") => app.long_open = app.sec.is_some(),
            // --solve factors: open the Design Factors dialog (for pictures)
            Some("factors") => {
                app.factors = app.sec.as_ref().map(|s| DesignForm {
                    code: match &s.aci { Some(a) if a.code == 1 => 3, Some(_) => 2, None => if is_factored(&s.phi) { 1 } else { 0 } },
                    phi: if is_factored(&s.phi) { s.phi } else { PHI_CSA },
                    aci: s.aci.clone().unwrap_or_default(),
                });
            }
            // --solve quick1..quick4: open the Quick Define wizard at that step (for pictures)
            Some(s) if s.starts_with("quick") => {
                app.quick = QuickForm::default();
                app.quick.step = s[5..].parse::<usize>().unwrap_or(1).clamp(1, 4) - 1;
                app.quick_open = true;
            }
            _ => {}
        }
        app
    }

    fn title(&self) -> String {
        let name = self.sec.as_ref().map(|s| s.title.clone()).unwrap_or_else(|| "Newbeam".into());
        format!("Section Response - {}{}", name, if self.dirty { " *" } else { "" })
    }

    fn clear_results(&mut self) {
        self.flex = None;
        self.shear = None;
        self.inter = None;
        self.member = None;
        self.stage = None;
        self.aci_models = None;
        self.biax = None;
        self.s2 = None;
        self.lc_rows = None;
        self.view = if self.sec.is_some() { View::Section } else { View::Welcome };
    }

    /// The ACI 318 models for the design curves, when the section uses ACI 318 (cached).
    fn aci(&mut self) -> Option<std::rc::Rc<crate::design::AciModels>> {
        let sec = self.sec.as_ref()?;
        let a = sec.aci.as_ref()?;
        if self.aci_models.as_ref().map(|m| &m.chosen != a).unwrap_or(true) {
            self.aci_models = Some(std::rc::Rc::new(crate::design::AciModels::new(sec, a)));
        }
        self.aci_models.clone()
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.clear_results();
    }

    fn open_path(&mut self, p: PathBuf) {
        match std::fs::read(&p) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                match rsp::load(&text) {
                    Ok(list) => {
                        self.sections = list;
                        self.sec = Some(self.sections[0].clone());
                        self.path = Some(p.clone());
                        self.dirty = false;
                        self.clear_results();
                        if self.sections.len() > 1 {
                            self.pick_open = true;
                        }
                        self.status = format!(
                            "Opened {}",
                            p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
                        );
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Err(e) => self.error = Some(format!("Could not read the file: {}", e)),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Section files", &["rsp"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.open_path(p);
        }
    }

    fn save(&mut self, ask: bool) {
        let Some(sec) = &self.sec else { return };
        let path = if ask || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("Section files", &["rsp"])
                .set_file_name("section.rsp")
                .save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            match std::fs::write(&p, rsp::save(sec)) {
                Ok(_) => {
                    self.path = Some(p);
                    self.dirty = false;
                    self.status = "Saved".into();
                }
                Err(e) => self.error = Some(format!("Could not save: {}", e)),
            }
        }
    }

    fn new_file(&mut self) {
        self.sec = None;
        self.sections.clear();
        self.path = None;
        self.dirty = false;
        self.clear_results();
    }

    fn start(&mut self, job: Job) {
        let Some(sec) = self.sec.clone() else {
            self.error = Some("Define a section first (Define > Quick Define).".into());
            return;
        };
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let have_flex = self.flex.as_ref().map(|f| f.1.clone());
        let member = sec.member.clone().unwrap_or(Member {
            length: 10.0 * sec.height(),
            left_sup: 0.0,
            right_sup: 10.0 * sec.height(),
            load_at: 5.0 * sec.height(),
        });
        self.progress = (0.0, "Starting".into());
        let mv_points = self.mv_points;
        let (bi_n, bi_alpha, bi_dirs) = (self.bi_n, self.bi_alpha, self.bi_dirs);
        let (s2_n, s2_mv, s2_dirs) = (self.s2_n, self.s2_mv, self.s2_dirs);
        let lc_cases = crate::demand::parse(&self.lc_text);
        std::thread::spawn(move || {
            let n = sec.loads.axial;
            let fm = match analysis::build(&sec, 80) {
                Ok(m) => m,
                Err(e) => {
                    let _ = tx.send(JobMsg::Error(e));
                    let _ = tx.send(JobMsg::Done);
                    return;
                }
            };
            let _ = tx.send(JobMsg::Progress(0.05, "Moment-curvature".into()));
            let mphi = match have_flex {
                Some(r) if job != Job::Flexure => r,
                _ => moment_curvature(&fm, n),
            };
            if mphi.points.len() < 2 {
                let _ = tx.send(JobMsg::Error(format!("The section could not be analysed: {}", mphi.stop_reason)));
                let _ = tx.send(JobMsg::Done);
                return;
            }
            let _ = tx.send(JobMsg::Flexure(fm.clone(), mphi.clone()));
            match job {
                Job::Flexure => {}
                Job::LoadCases => {
                    let _ = tx.send(JobMsg::Progress(0.05, format!("{} load cases", lc_cases.len())));
                    let txp = std::sync::Mutex::new(tx.clone());
                    match crate::demand::evaluate(&sec, &lc_cases, &|p| {
                        if let Ok(t) = txp.lock() {
                            let _ = t.send(JobMsg::Progress(0.05 + 0.95 * p, "Load cases: D/C ratios".into()));
                        }
                    }) {
                        Ok(rows) => {
                            let _ = tx.send(JobMsg::LoadCases(rows));
                        }
                        Err(e) => {
                            let _ = tx.send(JobMsg::Error(e));
                        }
                    }
                }
                Job::Shear2d => {
                    let _ = tx.send(JobMsg::Progress(0.05, "Shear in two directions (Vx-Vy)".into()));
                    let txp = std::sync::Mutex::new(tx.clone());
                    let d = crate::shear2d::diagram(&sec, s2_n, s2_mv, s2_dirs, &|p| {
                        if let Ok(t) = txp.lock() {
                            let _ = t.send(JobMsg::Progress(0.05 + 0.95 * p, "Shear in two directions (Vx-Vy)".into()));
                        }
                    });
                    let _ = tx.send(JobMsg::Shear2d(d));
                }
                Job::Biaxial => {
                    let _ = tx.send(JobMsg::Progress(0.1, "Biaxial bending (Mx-My)".into()));
                    let txp = std::sync::Mutex::new(tx.clone());
                    match crate::biaxial::diagram(&sec, bi_n, bi_alpha, bi_dirs, &|p| {
                        if let Ok(t) = txp.lock() {
                            let _ = t.send(JobMsg::Progress(0.1 + 0.9 * p, "Biaxial bending (Mx-My)".into()));
                        }
                    }) {
                        Ok(d) => {
                            let _ = tx.send(JobMsg::Biaxial(d));
                        }
                        Err(e) => {
                            let _ = tx.send(JobMsg::Error(e));
                        }
                    }
                }
                Job::Shear => {
                    let _ = tx.send(JobMsg::Progress(0.3, "Shear analysis (MCFT)".into()));
                    if let Ok(s) = analysis::build_shear(&sec) {
                        let mp = moment_curvature(&s, n);
                        let (c, d) = sec.loads.model_spec();
                        let (r, alt) = analysis::sectional_response_m(&s, c, d, &mp);
                        let _ = tx.send(JobMsg::Shear(alt.unwrap_or(s), r));
                    }
                }
                Job::Interaction => {
                    let label = format!("M-V interaction ({} points per side)", mv_points);
                    let r = interaction_both(&sec, n, mv_points, &|p| {
                        let _ = tx.send(JobMsg::Progress(0.1 + 0.9 * p, label.clone()));
                    });
                    if let Some(r) = r {
                        let _ = tx.send(JobMsg::Inter(r));
                    }
                }
                Job::Member => {
                    let r = member_response(&member, &mphi);
                    let _ = tx.send(JobMsg::Member(r));
                }
                Job::MN | Job::NV | Job::MVC => {
                    let pair = match job {
                        Job::MN => Pair::MN,
                        Job::NV => Pair::NV,
                        _ => Pair::MV,
                    };
                    let label = match job {
                        Job::MN => "M-N interaction",
                        Job::NV => "N-V interaction",
                        _ => "M-V interaction (original's run layout)",
                    };
                    let r = radial_interaction(&sec, pair, mv_points, &|p| {
                        let _ = tx.send(JobMsg::Progress(0.1 + 0.9 * p, label.into()));
                    });
                    match r {
                        Some(r) => {
                            let _ = tx.send(JobMsg::Radial(r));
                        }
                        None => {
                            let _ = tx.send(JobMsg::Error("The interaction diagram could not be computed.".into()));
                        }
                    }
                }
                Job::Cases => {
                    // the original's own runs on the sample beam, repeated here with the same loads
                    let _ = tx.send(JobMsg::Progress(0.2, "Running the original's load cases".into()));
                    let rows: Vec<CaseRow> = std::thread::scope(|s| {
                        let hs: Vec<_> = crate::compare::sample_cases()
                            .into_iter()
                            .map(|case| {
                                let sec = sec.clone();
                                s.spawn(move || {
                                    let t0 = std::time::Instant::now();
                                    let mut sc = sec.clone();
                                    sc.loads.axial = case.cst.0;
                                    sc.loads.m_const = case.cst.1;
                                    sc.loads.v_const = case.cst.2;
                                    sc.loads.n_inc = case.inc.0;
                                    sc.loads.m_inc = case.inc.1;
                                    sc.loads.v_inc = case.inc.2;
                                    sc.loads.sync();
                                    let Ok(sm) = analysis::build_shear(&sc) else {
                                        return CaseRow { case, ours: None, reason: "The section could not be built.".into(), secs: 0.0 };
                                    };
                                    let mp = moment_curvature(&sm, sc.loads.axial);
                                    let (c, d) = sc.loads.model_spec();
                                    let r = analysis::sectional_response(&sm, c, d, &mp);
                                    // the peak: largest shear, or largest moment when only the moment rises
                                    let pk = if d.2.abs() > 1e-12 {
                                        r.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap())
                                    } else {
                                        r.points.iter().max_by(|a, b| a.m.abs().partial_cmp(&b.m.abs()).unwrap())
                                    };
                                    CaseRow { ours: pk.map(|p| (p.m.abs(), p.v)), reason: r.reason.clone(), secs: t0.elapsed().as_secs_f64(), case }
                                })
                            })
                            .collect();
                        hs.into_iter().filter_map(|h| h.join().ok()).collect()
                    });
                    let _ = tx.send(JobMsg::Cases(rows));
                }
            }
            let _ = tx.send(JobMsg::Done);
        });
        self.job = Some((job, rx));
    }

    fn poll(&mut self) {
        let mut done = false;
        let mut msgs = vec![];
        if let Some((_, rx)) = &self.job {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        let job = self.job.as_ref().map(|j| j.0);
        for m in msgs {
            match m {
                JobMsg::Progress(p, s) => self.progress = (p, s),
                JobMsg::Flexure(m, r) => {
                    let peak = r.peak;
                    self.flex = Some((m, r));
                    if job == Some(Job::Flexure) {
                        self.view = View::MPhi;
                        self.stage = Some(Stage::Flexure(peak));
                    }
                }
                JobMsg::Shear(m, r) => {
                    // open at the peak, as the original does (the largest shear, or the largest
                    // moment when only the moment rises)
                    let by_v = r.points.iter().any(|p| p.v > 1e-6) && r.points.iter().map(|p| p.v).fold(0.0, f64::max) - r.points.iter().map(|p| p.v).fold(f64::MAX, f64::min) > 1e-6;
                    let last = r
                        .points
                        .iter()
                        .enumerate()
                        .max_by(|a, b| if by_v { a.1.v.partial_cmp(&b.1.v).unwrap() } else { a.1.m.abs().partial_cmp(&b.1.m.abs()).unwrap() })
                        .map(|x| x.0)
                        .unwrap_or(0);
                    self.status = format!(
                        "Shear capacity {:.1} kN ({})",
                        r.points.iter().map(|p| p.v).fold(0.0, f64::max),
                        r.reason
                    );
                    self.shear = Some((m, r));
                    self.stage = Some(Stage::Shear(last));
                    self.view = View::Nine;
                }
                JobMsg::Inter(r) => {
                    self.inter = Some(r);
                    self.view = View::Interaction;
                }
                JobMsg::Member(r) => {
                    self.member = Some(r);
                    self.view = View::Member;
                }
                JobMsg::LoadCases(r) => {
                    self.lc_rows = Some(r);
                    self.view = View::LoadCases;
                }
                JobMsg::Shear2d(d) => {
                    self.s2 = Some(d);
                    self.view = View::Shear2d;
                }
                JobMsg::Biaxial(d) => {
                    self.biax = Some(d);
                    self.view = View::Biaxial;
                }
                JobMsg::Radial(r) => {
                    self.radial = Some(r);
                    self.view = View::Radial;
                }
                JobMsg::Cases(r) => {
                    self.cases = Some(r);
                    self.view = View::Cases;
                }
                JobMsg::Error(e) => self.error = Some(e),
                JobMsg::Done => done = true,
            }
        }
        if done {
            self.job = None;
            if job == Some(Job::Flexure) {
                if let Some((_, r)) = &self.flex {
                    let p = &r.points[r.peak];
                    self.status = format!("Peak moment {:.1} kNm at {:.1} rad/km ({})", p.m, p.phi, r.stop_reason);
                }
            }
        }
    }

    // ------------------------------------------------------------------ menus

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_file();
                    ui.close();
                }
                if ui.button("Open...").clicked() {
                    ui.close();
                    self.open_dialog();
                }
                if ui.add_enabled(self.sec.is_some(), egui::Button::new("Save")).clicked() {
                    ui.close();
                    self.save(false);
                }
                if ui.add_enabled(self.sec.is_some(), egui::Button::new("Save As...")).clicked() {
                    ui.close();
                    self.save(true);
                }
                if ui
                    .add_enabled(self.sections.len() > 1, egui::Button::new("Choose section in file..."))
                    .clicked()
                {
                    self.pick_open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Define", |ui| {
                if ui.button("Quick Define...").clicked() {
                    self.quick_open = true;
                    self.quick = QuickForm::default();
                    ui.close();
                }
                ui.separator();
                let has = self.sec.is_some();
                if ui.add_enabled(has, egui::Button::new("Materials...")).clicked() {
                    self.mat_open = true;
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Concrete Section...")).clicked() {
                    self.shape_open = true;
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Longitudinal Reinforcement...")).clicked() {
                    self.long_open = true;
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Transverse Reinforcement...")).clicked() {
                    self.trans_open = true;
                    ui.close();
                }
            });
            ui.menu_button("Loads", |ui| {
                if ui.add_enabled(self.sec.is_some(), egui::Button::new("Loads...")).clicked() {
                    self.loads_open = true;
                    ui.close();
                }
            });
            ui.menu_button("Catalog", |ui| {
                ui.label("Sample sections:");
                for (name, f) in samples() {
                    if ui.button(name).clicked() {
                        let s = f();
                        self.sections = vec![s.clone()];
                        self.sec = Some(s);
                        self.path = None;
                        self.changed();
                        ui.close();
                    }
                }
            });
            ui.menu_button("Solve", |ui| {
                let ok = self.sec.is_some() && self.job.is_none();
                if ui.add_enabled(ok, egui::Button::new("Moment-Curvature")).clicked() {
                    self.start(Job::Flexure);
                    ui.close();
                }
                if ui.add_enabled(ok, egui::Button::new("Shear Response (at loads M/V)")).clicked() {
                    self.start(Job::Shear);
                    ui.close();
                }
                if ui.add_enabled(ok, egui::Button::new("M-N Interaction")).clicked() {
                    self.start(Job::MN);
                    ui.close();
                }
                if ui.add_enabled(ok, egui::Button::new("M-V Interaction")).clicked() {
                    self.start(Job::Interaction);
                    ui.close();
                }
                if ui.add_enabled(ok, egui::Button::new("N-V Interaction")).clicked() {
                    self.start(Job::NV);
                    ui.close();
                }
                if ui
                    .add_enabled(ok, egui::Button::new("M-V Interaction, original's run layout"))
                    .on_hover_text("Runs fanning out from the centre moment, as the original lays them out")
                    .clicked()
                {
                    self.start(Job::MVC);
                    ui.close();
                }
                if ui.add_enabled(ok, egui::Button::new("Member Load-Deflection")).clicked() {
                    self.start(Job::Member);
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(self.sec.is_some(), egui::Button::new("Biaxial Bending (Mx-My)..."))
                    .on_hover_text("Bending about both axes at a chosen load angle, and the Mx-My capacity diagram at an axial load")
                    .clicked()
                {
                    self.view = View::Biaxial;
                    ui.close();
                }
                if ui
                    .add_enabled(self.sec.is_some(), egui::Button::new("Shear in Two Directions (Vx-Vy)..."))
                    .on_hover_text("The shear capacity in every direction at an axial load and M/V, and a check of Vx and Vy acting together")
                    .clicked()
                {
                    self.view = View::Shear2d;
                    ui.close();
                }
                if ui
                    .add_enabled(self.sec.is_some(), egui::Button::new("Load Cases (D/C ratios)..."))
                    .on_hover_text("Paste a table of Pu, Mx, My, Vx, Vy, Tu and get the demand/capacity ratios")
                    .clicked()
                {
                    self.view = View::LoadCases;
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                for (v, name, ok) in [
                    (View::Welcome, "Title Page", true),
                    (View::Section, "Cross Section", self.sec.is_some()),
                    (View::Nine, "Sectional Response (9 plots)", self.stage.is_some()),
                    (View::MPhi, "Moment-Curvature", self.flex.is_some()),
                    (View::Shear, "Shear - Shear Strain", self.shear.is_some()),
                    (View::Interaction, "M-V Interaction", self.inter.is_some()),
                    (View::Member, "Member Load-Deflection", self.member.is_some()),
                    (View::Radial, "M-N / N-V Interaction", self.radial.is_some()),
                    (View::Biaxial, "Biaxial Bending (Mx-My)", self.sec.is_some()),
                    (View::Shear2d, "Shear in Two Directions (Vx-Vy)", self.sec.is_some()),
                    (View::LoadCases, "Load Cases (D/C ratios)", self.sec.is_some()),
                ] {
                    if ui.add_enabled(ok, egui::Button::selectable(self.view == v, name)).clicked() {
                        self.view = v;
                        ui.close();
                    }
                }
            });
            ui.menu_button("Options", |ui| {
                ui.label("Units: SI (mm, MPa, kN, kNm)");
                if ui.add_enabled(self.sec.is_some(), egui::Button::new("Design Factors (CSA / ACI 318 / AASHTO)...")).clicked() {
                    self.factors = self.sec.as_ref().map(|s| DesignForm {
                        code: match &s.aci { Some(a) if a.code == 1 => 3, Some(_) => 2, None => if is_factored(&s.phi) { 1 } else { 0 } },
                        phi: if is_factored(&s.phi) { s.phi } else { PHI_CSA },
                        aci: s.aci.clone().unwrap_or_default(),
                    });
                    ui.close();
                }
                let mut ch = false;
                if let Some(sec) = &mut self.sec {
                    let mut neg = sec.loads.negative;
                    ch = ui.checkbox(&mut neg, "Negative moment (top in tension)").changed();
                    if ch {
                        // flip the moments so they put the top in tension
                        sec.loads.m_const = -sec.loads.m_const;
                        sec.loads.m_inc = -sec.loads.m_inc;
                        sec.loads.sync();
                        if sec.loads.m_inc.abs() < 1e-12 && sec.loads.m_const.abs() < 1e-12 {
                            sec.loads.negative = neg;
                        }
                    }
                }
                if ch {
                    self.changed();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About...").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let b = |ui: &mut egui::Ui, t: &str, tip: &str, en: bool| {
                ui.add_enabled(en, egui::Button::new(RichText::new(t).size(11.5)).min_size(Vec2::new(24.0, 20.0)))
                    .on_hover_text(tip)
                    .clicked()
            };
            let has = self.sec.is_some();
            let idle = self.job.is_none();
            if b(ui, "New", "New section", true) {
                self.new_file();
            }
            if b(ui, "Open", "Open a section file", true) {
                self.open_dialog();
            }
            if b(ui, "Save", "Save the section", has) {
                self.save(false);
            }
            ui.separator();
            if b(ui, "Section", "View the cross section and its properties", has) {
                self.view = View::Section;
            }
            if b(ui, "Quick", "Quick Define", true) {
                self.quick_open = true;
                    self.quick = QuickForm::default();
            }
            if b(ui, "Shape", "Concrete section", has) {
                self.shape_open = true;
            }
            if b(ui, "Bars", "Longitudinal reinforcement", has) {
                self.long_open = true;
            }
            if b(ui, "Stirrups", "Transverse reinforcement", has) {
                self.trans_open = true;
            }
            if b(ui, "Materials", "Materials", has) {
                self.mat_open = true;
            }
            if b(ui, "Loads", "Loads", has) {
                self.loads_open = true;
            }
            ui.separator();
            if b(ui, "M-φ", "Solve moment-curvature", has && idle) {
                self.start(Job::Flexure);
            }
            if b(ui, "Shear", "Solve shear response (9 plots)", has && idle) {
                self.start(Job::Shear);
            }
            if b(ui, "M-N", "Solve M-N (moment - axial load) interaction", has && idle) {
                self.start(Job::MN);
            }
            if b(ui, "M-V", "Solve M-V interaction", has && idle) {
                self.start(Job::Interaction);
            }
            if b(ui, "N-V", "Solve N-V (axial load - shear) interaction", has && idle) {
                self.start(Job::NV);
            }
            if b(ui, "Mx-My", "Biaxial bending: a load angle, and the Mx-My capacity diagram", has) {
                self.view = View::Biaxial;
            }
            if b(ui, "Vx-Vy", "Shear in two directions: the Vx-Vy capacity diagram and a check of Vx and Vy together", has) {
                self.view = View::Shear2d;
            }
            if b(ui, "D/C", "Load cases: paste Pu, Mx, My, Vx, Vy, Tu and get the demand/capacity ratios", has) {
                self.view = View::LoadCases;
            }
            if b(ui, "Beam", "Member load-deflection", has && idle) {
                self.start(Job::Member);
            }
            ui.separator();
            if b(ui, "?", "About", true) {
                self.about_open = true;
            }
        });
    }

    // ------------------------------------------------------------------ panels

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let rect = Rect::from_min_max(full.min + Vec2::new(14.0, 14.0), full.max - Vec2::new(14.0, 14.0));
        let painter = ui.painter_at(full);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(160, 160, 160)), StrokeKind::Inside);
        ui.allocate_rect(full, egui::Sense::hover());
        let Some(sec) = self.sec.clone() else {
            return;
        };
        painter.rect_filled(rect.shrink(1.0), 0.0, Color32::WHITE);
        let pic = Rect::from_min_max(
            rect.min + Vec2::new(10.0, 30.0),
            Pos2::new(rect.max.x - 10.0, rect.min.y + rect.height() * 0.5),
        );
        painter.text(
            Pos2::new(rect.center().x, rect.top() + 8.0),
            Align2::CENTER_TOP,
            &sec.title,
            FontId::proportional(14.0),
            Color32::BLACK,
        );
        draw_section(&painter, pic, &sec);
        let x = rect.left() + 12.0;
        let mut y = pic.bottom() + 14.0;
        let mut line = |t: String, bold: bool| {
            painter.text(
                Pos2::new(x, y),
                Align2::LEFT_TOP,
                t,
                FontId::proportional(if bold { 13.0 } else { 12.0 }),
                if bold { Color32::BLACK } else { Color32::from_rgb(50, 50, 50) },
            );
            y += if bold { 18.0 } else { 16.0 };
        };
        if let Some(c) = sec.concretes.first() {
            let p = crate::materials::ConcreteProps::new(c);
            line(format!("Concrete \"{}\"", c.name), true);
            line(format!("f'c = {:.1} MPa    a = {:.0} mm", c.fc, c.agg), false);
            line(format!("ft = {:.2} MPa    e'c = {:.2} mm/m", p.ft, p.e0 * 1000.0), false);
            line(format!("Ec = {:.0} MPa", p.ec), false);
        }
        for r in sec.rebars.iter().take(3) {
            line(format!("Rebar \"{}\"", r.name), true);
            line(format!("fy = {:.0} MPa    fu = {:.0} MPa", r.fy, r.fu), false);
            line(format!("esh = {:.1}    eu = {:.0} mm/m", r.esh, r.eu), false);
        }
        let ast: f64 = sec.long.iter().map(|l| l.area).sum();
        line("Section".into(), true);
        line(format!("Ag = {:.0} mm²    As = {:.0} mm²", sec.gross_area(), ast), false);
        if let Some(st) = &sec.stirrups {
            line(format!("Stirrups {:.0} mm² @ {:.0} mm", st.area, st.spacing), false);
        }
        line(
            format!(
                "Loads: {:.0}, {:.0}, {:.0} + {:.2}, {:.2}, {:.2} (N kN, M kNm, V kN)",
                sec.loads.axial, sec.loads.m_const, sec.loads.v_const, sec.loads.n_inc, sec.loads.m_inc, sec.loads.v_inc
            ),
            false,
        );
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_BOTTOM,
            "All dimensions in millimetres",
            FontId::proportional(11.0),
            GREY,
        );
    }

    fn welcome(&self, ui: &mut egui::Ui) {
        let r = ui.available_rect_before_wrap();
        let p = ui.painter_at(r);
        p.rect_filled(r, 0.0, Color32::WHITE);
        let img = Rect::from_min_size(
            Pos2::new(r.left() + r.width() * 0.64, r.top() + r.height() * 0.21),
            Vec2::new(r.width() * 0.23, r.height() * 0.54),
        );
        draw_column_art(&p, img);
        let tx = img.left() - r.width() * 0.027;
        let mut y = r.top() + r.height() * 0.21;
        let s = (r.height() / 1000.0).clamp(0.55, 1.4);
        let dark = Color32::from_rgb(30, 30, 30);
        let txt = |y: f32, t: &str, size: f32, c: Color32| {
            p.text(Pos2::new(tx, y), Align2::RIGHT_TOP, t, FontId::proportional(size), c);
        };
        // bold look: the default font has no bold face, so draw twice with a small offset
        let bold = |y: f32, t: &str, size: f32| {
            let off = (size / 30.0).max(0.8);
            p.text(Pos2::new(tx, y), Align2::RIGHT_TOP, t, FontId::proportional(size), Color32::BLACK);
            p.text(Pos2::new(tx - off, y), Align2::RIGHT_TOP, t, FontId::proportional(size), Color32::BLACK);
        };
        bold(y, "Section Response", 52.0 * s);
        y += 100.0 * s;
        bold(y, "Reinforced Concrete Sectional Analysis", 23.0 * s);
        y += 46.0 * s;
        bold(y, "using the Modified Compression Field Theory", 23.0 * s);
        y += 140.0 * s;
        txt(y, concat!("Version ", env!("CARGO_PKG_VERSION")), 23.0 * s, dark);
        y += 92.0 * s;
        txt(y, "Define a section with Define > Quick Define,", 22.0 * s, dark);
        y += 37.0 * s;
        txt(y, "or open a section file with File > Open.", 22.0 * s, dark);
        y += 37.0 * s;
        txt(y, "Then use Solve to analyse it.", 22.0 * s, dark);
        y += 55.0 * s;
        txt(y, "An independent rebuild for study and checking", 22.0 * s, dark);
        ui.allocate_rect(r, egui::Sense::hover());
    }

    fn stage_state(&self) -> Option<(SectionState, SModel)> {
        match self.stage? {
            Stage::Flexure(i) => {
                let (m, r) = self.flex.as_ref()?;
                let p = r.points.get(i)?;
                Some((m.uniaxial_state(p.ec, p.phi_raw), m.clone()))
            }
            Stage::Shear(i) => {
                let (m, r) = self.shear.as_ref()?;
                Some((r.points.get(i)?.clone(), m.clone()))
            }
        }
    }

    fn nine(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Load stage:");
            match self.stage {
                Some(Stage::Flexure(mut i)) => {
                    let n = self.flex.as_ref().map(|f| f.1.points.len()).unwrap_or(1);
                    if ui
                        .add(egui::Slider::new(&mut i, 0..=n.saturating_sub(1)).text("moment-curvature step"))
                        .changed()
                    {
                        self.stage = Some(Stage::Flexure(i));
                    }
                }
                Some(Stage::Shear(mut i)) => {
                    let n = self.shear.as_ref().map(|f| f.1.points.len()).unwrap_or(1);
                    if ui.add(egui::Slider::new(&mut i, 0..=n.saturating_sub(1)).text("shear step")).changed() {
                        self.stage = Some(Stage::Shear(i));
                    }
                }
                None => {}
            }
            if self.shear.is_some() && ui.button("Shear stages").clicked() {
                let n = self.shear.as_ref().unwrap().1.points.len();
                self.stage = Some(Stage::Shear(n.saturating_sub(1)));
            }
            if self.flex.is_some() && ui.button("Flexure stages").clicked() {
                let p = self.flex.as_ref().unwrap().1.peak;
                self.stage = Some(Stage::Flexure(p));
            }
        });
        let Some((st, m)) = self.stage_state() else {
            ui.label("Solve the section to see its response.");
            return;
        };
        use crate::nine::{self as nn, Curve, LIMIT_RED, LINE_BLUE};
        let r = ui.available_rect_before_wrap();
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, Color32::WHITE);
        // left column: the original's "Nine Graphs" panel with two control plots
        let lw = (r.width() * 0.24).clamp(220.0, 480.0);
        let left = Rect::from_min_max(r.min, Pos2::new(r.left() + lw, r.bottom()));
        let right = Rect::from_min_max(Pos2::new(r.left() + lw + 8.0, r.top()), r.max);
        painter.rect_filled(left, 0.0, Color32::from_rgb(240, 240, 240));
        painter.line_segment([Pos2::new(left.right() + 4.0, r.top()), Pos2::new(left.right() + 4.0, r.bottom())], Stroke::new(1.0, Color32::from_gray(150)));
        let head = Rect::from_min_max(left.min + Vec2::new(8.0, 8.0), Pos2::new(left.right() - 8.0, left.top() + left.height() * 0.2));
        painter.rect_stroke(head, 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
        let hs = (head.height() * 0.2).clamp(14.0, 30.0);
        painter.text(Pos2::new(head.center().x, head.top() + hs * 0.6), Align2::CENTER_TOP, "Nine Graphs", FontId::proportional(hs), Color32::BLACK);
        painter.text(
            Pos2::new(head.center().x, head.bottom() - 8.0),
            Align2::CENTER_BOTTOM,
            format!("N {:.1} kN   M {:.1} kNm   V {:.1} kN", st.n, st.m, st.v),
            FontId::proportional((hs * 0.55).max(11.0)),
            Color32::from_gray(40),
        );
        let ch = (left.bottom() - head.bottom() - 16.0) / 2.0;
        let c1 = Rect::from_min_size(Pos2::new(left.left() + 8.0, head.bottom() + 8.0), Vec2::new(left.width() - 16.0, ch - 4.0));
        let c2 = Rect::from_min_size(Pos2::new(left.left() + 8.0, c1.bottom() + 8.0), Vec2::new(left.width() - 16.0, ch - 4.0));
        match self.stage {
            Some(Stage::Shear(i)) => {
                if let Some((_, sr)) = &self.shear {
                    let g: Vec<(f64, f64)> = sr.points.iter().map(|p| (p.gamma_avg, p.v)).collect();
                    let f: Vec<(f64, f64)> = sr.points.iter().map(|p| (p.phi, p.m.abs())).collect();
                    // the shear-strain control when the shear rises, the curvature one always
                    if g.iter().any(|q| q.1 > 1e-6) {
                        nn::control(&painter, c1, "Control : Shear - \u{3b3}xy", &g, g.get(i).copied());
                    } else {
                        let e: Vec<(f64, f64)> = sr.points.iter().map(|p| (p.layers.iter().map(|l| l.ex).fold(0.0, f64::max) * 1000.0, p.m.abs())).collect();
                        nn::control(&painter, c1, "Control : Moment - \u{3b5}x", &e, e.get(i).copied());
                    }
                    nn::control(&painter, c2, "Control : Moment - \u{3c6}", &f, f.get(i).copied());
                }
            }
            Some(Stage::Flexure(i)) => {
                if let Some((_, fr)) = &self.flex {
                    let e: Vec<(f64, f64)> = fr.points.iter().map(|p| (p.bot_steel * 1000.0, p.m)).collect();
                    let f: Vec<(f64, f64)> = fr.points.iter().map(|p| (p.phi, p.m)).collect();
                    nn::control(&painter, c1, "Control : Moment - \u{3b5}x", &e, e.get(i).copied());
                    nn::control(&painter, c2, "Control : Moment - \u{3c6}", &f, f.get(i).copied());
                }
            }
            None => {}
        }
        // the 3 x 3 grid, in the original's order
        let cw = right.width() / 3.0;
        let chh = right.height() / 3.0;
        let cell = |i: usize| {
            let (cx, cy) = ((i % 3) as f32, (i / 3) as f32);
            Rect::from_min_size(Pos2::new(right.left() + cx * cw, right.top() + cy * chh), Vec2::new(cw, chh)).shrink(6.0)
        };
        let h = m.h;
        let prof = |f: &dyn Fn(&LayerState, usize) -> f64| -> Vec<(f64, f64)> {
            let mut v: Vec<(f64, f64)> = st.layers.iter().enumerate().map(|(i, l)| (f(l, i), m.display_z(l.z))).collect();
            v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            v
        };
        let blue = |pts| Curve { pts, color: LINE_BLUE };
        let red = |pts| Curve { pts, color: LIMIT_RED };
        if let Some(sec) = &self.sec {
            nn::section_state(&painter, cell(0), sec, &st, &m);
        }
        nn::profile(&painter, cell(1), "Longitudinal Strain", &[blue(prof(&|l, _| l.ex * 1000.0))], h);
        nn::profile(&painter, cell(2), "Transverse Strain", &[blue(prof(&|l, _| l.ez * 1000.0))], h);
        nn::crack_diagram(&painter, cell(3), &st, &m, h);
        nn::profile(&painter, cell(4), "Shear Strain", &[blue(prof(&|l, _| l.gamma * 1000.0))], h);
        nn::profile(&painter, cell(5), "Shear Stress", &[blue(prof(&|l, _| l.v))], h);
        // the stirrups' reserve at a crack, per layer: rho_z (fy_z - f_sz)
        let fyz = m.stirrup.as_ref().map(|s| s.0.fy()).unwrap_or(0.0);
        let reserve = |l: &LayerState, i: usize| -> f64 { m.layers.get(i).map(|ml| ml.rho_z * (fyz - l.fsz).max(0.0)).unwrap_or(0.0) };
        let tan_t = |l: &LayerState| -> f64 { l.theta.abs().clamp(0.05, 1.52).tan() };
        nn::profile(
            &painter,
            cell(6),
            "Principal Compressive Stress",
            &[red(prof(&|l, _| -m.conc.fc * m.conc.beta(l.e1))), blue(prof(&|l, _| -l.f2))],
            h,
        );
        // shear on the crack, as the original computes it (0x46aaa0, decompiled): the steel
        // crossing the crack can add rho (fy - fs) in each direction; the direction with the smaller
        // reserve governs - longitudinal: vci = (f1 - reserve_x) tan(theta), transverse:
        // vci = (f1 - reserve_z) / tan(theta). A web layer without longitudinal bars has no x
        // reserve, so there vci = f1 tan(theta). Red: +- vci,max for the crack width.
        let reserve_x = |l: &LayerState, i: usize| -> f64 {
            let Some(ml) = m.layers.get(i) else { return 0.0 };
            let ar = (ml.b * ml.t).max(1e-9);
            m.bars
                .iter()
                .zip(&st.bars)
                .filter(|(b, _)| (b.z - ml.z).abs() <= 0.5 * ml.t + 1e-9)
                .map(|(b, sb)| b.area * (b.steel.fy() - sb.2.abs()).max(0.0) / ar)
                .sum::<f64>()
                .max(0.0)
                + 0.0 * l.f1
        };
        let vci = |l: &LayerState, i: usize| -> f64 {
            if !l.cracked {
                return 0.0;
            }
            let (rx, rz) = (reserve_x(l, i), reserve(l, i));
            // no steel crossing the crack in either direction: the original sets it to zero
            if rx < 1e-6 && rz < 1e-6 {
                return 0.0;
            }
            let v = if rx <= rz { (l.f1 - rx).max(0.0) * l.theta.abs().clamp(1e-6, 1.570795).tan() } else { (l.f1 - rz).max(0.0) / l.theta.abs().clamp(1e-6, 1.570795).tan() };
            // the original's crack check keeps f1 low enough that this never passes vci,max; this
            // program's solver checks the stirrup direction only, so the plot caps it there
            v.min(m.conc.vci_max(l.w))
        };
        nn::profile(
            &painter,
            cell(7),
            "Shear on Crack",
            &[red(prof(&|l, _| m.conc.vci_max(l.w))), red(prof(&|l, _| -m.conc.vci_max(l.w))), blue(prof(&vci))],
            h,
        );
        // principal tension: what the layer carries against its limit (cracking strength before
        // cracking; after it the smaller of tension stiffening and the crack check)
        let f1_lim = |l: &LayerState, i: usize| -> f64 {
            if !l.cracked {
                m.conc.ft
            } else {
                let mb = m.layers.get(i).map(|ml| ml.m_bond).unwrap_or(100.0);
                m.conc.tension(l.e1, mb).min(m.conc.vci_max(l.w) * tan_t(l) + reserve(l, i))
            }
        };
        nn::profile(&painter, cell(8), "Principal Tensile Stress", &[red(prof(&f1_lim)), blue(prof(&|l, _| l.f1))], h);
        ui.allocate_rect(r, egui::Sense::hover());
    }

    fn mphi_view(&mut self, ui: &mut egui::Ui) {
        let dm = self.aci();
        let (n_axial, neg) = self.sec.as_ref().map(|s| (s.loads.axial, s.loads.negative)).unwrap_or((0.0, false));
        let Some((fm, r)) = &self.flex else { return };
        let r = r.clone();
        let (lim_c, lim_s) = analysis::mphi_limits(fm, &r);
        let (ecu, esl) = (fm.conc.ecu * 1000.0, fm.bars.iter().map(|b| b.e_lim).fold(f64::INFINITY, f64::min) * 1000.0);
        let pk = &r.points[r.peak];
        ui.label(
            RichText::new(format!(
                "Peak moment {:.1} kNm at curvature {:.2} rad/km.    Cracking {}    Yield {}    End: {}",
                pk.m,
                pk.phi,
                r.crack_m.map(|m| format!("{:.1} kNm", m)).unwrap_or("-".into()),
                r.yield_m.map(|m| format!("{:.1} kNm", m)).unwrap_or("-".into()),
                r.stop_reason
            ))
            .strong(),
        );
        ui.label(
            RichText::new(format!(
                "Design limits:  concrete ecu = {:.2} mm/m reached at {}    steel es = {:.1} mm/m reached at {}",
                ecu,
                lim_c.map(|p| format!("M = {:.1} kNm (curvature {:.2} rad/km)", p.1, p.0)).unwrap_or("- (not reached)".into()),
                esl,
                lim_s.map(|p| format!("M = {:.1} kNm (curvature {:.2} rad/km)", p.1, p.0)).unwrap_or("- (not reached)".into()),
            ))
            .strong(),
        );
        let aci_state = dm.as_ref().map(|dm| dm.state(n_axial, neg));
        if let (Some(dm), Some(s)) = (&dm, aci_state) {
            ui.label(
                RichText::new(format!(
                    "{} design moment φMn = {:.3} × {:.1} = {:.1} kNm (εt = {:.2} mm/m at εcu = {:.1} mm/m, {}); black curve = φ × nominal",
                    dm.a.name(),
                    s.phi,
                    pk.m,
                    s.phi * pk.m,
                    s.et * 1000.0,
                    dm.a.ecu * 1000.0,
                    s.class(&dm.a)
                ))
                .strong(),
            );
        }
        ui.horizontal(|ui| {
            if let Some(Stage::Flexure(mut i)) = self.stage {
                if ui.add(egui::Slider::new(&mut i, 0..=r.points.len() - 1).text("selected step")).changed() {
                    self.stage = Some(Stage::Flexure(i));
                }
            }
            if ui.button("Show 9 plots for this step").clicked() {
                if !matches!(self.stage, Some(Stage::Flexure(_))) {
                    self.stage = Some(Stage::Flexure(r.peak));
                }
                self.view = View::Nine;
            }
        });
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let pts: Vec<[f64; 2]> = r.points.iter().map(|p| [p.phi, p.m]).collect();
        let mut series = vec![Series::line(pts, BLUE)];
        let design_red = Color32::BLACK;
        if let Some(s) = aci_state {
            // ACI 318: the design moment phi Mn, phi from et at this axial load
            let d: Vec<[f64; 2]> = r.points.iter().map(|p| [p.phi, s.phi * p.m]).collect();
            series.push(Series { width: 2.0, ..Series::line(d, design_red) });
        }
        if let Some(Stage::Flexure(i)) = self.stage {
            if let Some(p) = r.points.get(i) {
                series.push(Series { dots: true, ..Series::line(vec![[p.phi, p.m]], RED) });
            }
        }
        let (c_col, s_col) = (Color32::from_rgb(230, 120, 0), Color32::from_rgb(0, 140, 140));
        if let Some(p) = lim_c {
            series.push(Series { dots: true, width: 0.0, ..Series::line(vec![[p.0, p.1]], c_col) });
        }
        if let Some(p) = lim_s {
            series.push(Series { dots: true, width: 0.0, ..Series::line(vec![[p.0, p.1]], s_col) });
        }
        let fr = plots::draw(&painter, rect.shrink(10.0), "Moment-Curvature", "Curvature (rad/km)", "Moment (kNm)", &series, None, None);
        for (p, col, name) in [(lim_c, c_col, "concrete ecu"), (lim_s, s_col, "steel limit es")] {
            if let Some(p) = p {
                let at = fr.map(p.0, p.1);
                painter.circle_filled(at, 5.0, col);
                painter.text(at + Vec2::new(8.0, 10.0), Align2::LEFT_TOP, name, FontId::proportional(13.0), col);
            }
        }
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    fn shear_view(&mut self, ui: &mut egui::Ui) {
        let Some((_, r)) = &self.shear else { return };
        let r = r.clone();
        let vmax = r.points.iter().map(|p| p.v).fold(0.0, f64::max);
        ui.label(
            RichText::new(format!(
                "Shear capacity V = {:.1} kN with M = {:.1} kNm (M/V = {:.2} m).    {}",
                vmax,
                vmax * r.mv,
                r.mv,
                r.reason
            ))
            .strong(),
        );
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let pts: Vec<[f64; 2]> = r.points.iter().map(|p| [p.gamma_avg, p.v]).collect();
        let mut series = vec![Series { dots: true, ..Series::line(pts, BLUE) }];
        if let Some(Stage::Shear(i)) = self.stage {
            if let Some(p) = r.points.get(i) {
                series.push(Series { dots: true, ..Series::line(vec![[p.gamma_avg, p.v]], RED) });
            }
        }
        plots::draw(&painter, rect.shrink(10.0), "Shear - Shear Strain", "Average shear strain (mm/m)", "Shear (kN)", &series, None, None);
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    fn inter_view(&mut self, ui: &mut egui::Ui) {
        let dm = self.aci();
        let n_axial = self.sec.as_ref().map(|s| s.loads.axial).unwrap_or(0.0);
        let Some(r) = &self.inter else { return };
        let r = r.clone();
        ui.label(RichText::new("Moment-shear interaction: combinations of M and V that fail the section at the given axial load (negative moments put the top in tension)").strong());
        ui.horizontal(|ui| {
            ui.label("Points per side:");
            ui.add(egui::Slider::new(&mut self.mv_points, 4..=40));
            let idle = self.job.is_none();
            if ui
                .add_enabled(idle, egui::Button::new("Recalculate"))
                .on_hover_text("More points give a smoother curve and take longer (each point is a full analysis to failure)")
                .clicked()
            {
                self.start(Job::Interaction);
            }
            ui.label(format!("{} points shown", r.points.len()));
        });
        ui.label(RichText::new("How to read this plot (hover)").small().weak()).on_hover_text(
            "Each point is a separate analysis in which shear and moment rise together (at one fixed ratio) until the section \
             fails; its colour says what failed. Filled = confirmed failure; hollow = the solver stopped without proving failure \
             (capacity may be higher); grey x = inside the envelope (its run stopped early). Doubtful points are checked with \
             the moment held and with the shear held. The orange and teal curves show where the concrete reaches its design \
             strain limit ecu and where a bar reaches its design strain limit (Define > Materials).",
        );
        // the strength envelope is always drawn; everything else is the user's choice
        let has_shear = self.shear.is_some();
        ui.horizontal_wrapped(|ui| {
            let s = &mut self.mv_show;
            ui.label("Show:");
            ui.checkbox(&mut s.cracking, "Cracking");
            ui.checkbox(&mut s.long_yield, "Longitudinal yield");
            ui.checkbox(&mut s.trans_yield, "Transverse yield");
            ui.checkbox(&mut s.conc_limit, "Concrete at ecu");
            ui.checkbox(&mut s.steel_limit, "Steel at its limit");
            if dm.is_some() {
                ui.checkbox(&mut s.design, "Design curve");
            }
            ui.checkbox(&mut s.points, "Failure points").on_hover_text("Each envelope point coloured by what failed");
            if !r.checks.is_empty() {
                ui.checkbox(&mut s.checks, "Check runs").on_hover_text("Doubtful points re-run with the moment held / the shear held");
            }
            if has_shear {
                ui.checkbox(&mut s.shear_peak, "Shear analysis peak");
            }
            ui.separator();
            if ui.small_button("All").clicked() {
                *s = MvShow {
                    cracking: true,
                    long_yield: true,
                    trans_yield: true,
                    conc_limit: true,
                    steel_limit: true,
                    design: true,
                    points: true,
                    checks: true,
                    shear_peak: true,
                };
            }
            if ui.small_button("Envelope only").clicked() {
                *s = MvShow::default();
            }
        });
        let show = self.mv_show;
        if !r.checks.is_empty() {
            egui::CollapsingHeader::new(format!(
                "Doubtful points checked two other ways ({}: {} agree, {} moved out, {} still disagree)",
                r.checks.len(),
                r.checks.iter().filter(|c| c.verdict == "agree").count(),
                r.checks.iter().filter(|c| c.verdict == "moved").count(),
                r.checks.iter().filter(|c| c.verdict == "disagree").count()
            ))
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("xchk").num_columns(5).spacing([18.0, 2.0]).striped(true).show(ui, |ui| {
                    for h in ["Point (M/V held)", "Moment held, shear raised", "Shear held, moment raised", "Result", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    let alt = |x: Option<(f64, bool)>, unit: &str| -> String {
                        match x {
                            Some((v, true)) => format!("{:.1} {}", v, unit),
                            Some((v, false)) => format!("{:.1} {} (stopped, no failure proof)", v, unit),
                            None => "could not be carried".into(),
                        }
                    };
                    for c in &r.checks {
                        ui.label(format!("M {:.1} kNm, V {:.1} kN", c.m, c.v));
                        ui.label(format!("V = {}", alt(c.fixed_m, "kN")));
                        ui.label(format!("M = {}", alt(c.fixed_v, "kNm")));
                        let (txt, col) = match c.verdict.as_str() {
                            "agree" => ("agree - confirmed".to_string(), Color32::from_rgb(0, 130, 0)),
                            "moved" => (
                                c.moved_to.map(|p| format!("moved out to ({:.1}, {:.1})", p.0, p.1)).unwrap_or_default(),
                                Color32::from_rgb(20, 90, 200),
                            ),
                            _ => ("disagree - hollow".to_string(), Color32::from_rgb(190, 30, 30)),
                        };
                        ui.colored_label(col, txt);
                        ui.label("");
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new(
                        "Each doubtful point is run again with its moment held while the shear rises, and with its shear held \
                         while the moment rises (constant + increment loading, as the original allows). On the plot: triangles \
                         = moment held, diamonds = shear held; filled when that run ended in a real failure.",
                    )
                    .weak()
                    .small(),
                );
            });
        }
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let to = |v: &[(f64, f64)]| -> Vec<[f64; 2]> { v.iter().map(|p| [p.0, p.1]).collect() };
        let magenta = Color32::from_rgb(220, 0, 220);
        let green = Color32::from_rgb(0, 170, 0);
        let orig_c = Color32::from_rgb(110, 110, 110);
        let (lim_c, lim_s) = (Color32::from_rgb(230, 120, 0), Color32::from_rgb(0, 140, 140));
        let mut series = vec![];
        // the name of each series, for reading values off the plot
        let mut names: Vec<&str> = vec![];
        let _ = orig_c; // the original's points are compared in the table, not drawn
        if show.cracking {
            series.push(Series { width: 1.2, ..Series::line(to(&r.cracking), RED) });
            names.push("Cracking");
        }
        if show.long_yield {
            series.push(Series { width: 1.2, ..Series::line(to(&r.long_yield), green) });
            names.push("Longitudinal yield");
        }
        if show.trans_yield {
            series.push(Series { width: 1.2, ..Series::line(to(&r.trans_yield), magenta) });
            names.push("Transverse yield");
        }
        series.push(Series { width: 2.0, ..Series::line(to(&r.envelope()), BLUE) });
        names.push("Strength envelope");
        let design_red = Color32::BLACK;
        let dm = if show.design { dm } else { None };
        if let Some(dm) = &dm {
            // ACI 318: (phi(et) M, phi_v V) for each envelope point
            series.push(Series { width: 2.0, ..Series::line(to(&dm.mv(&r.envelope(), n_axial)), design_red) });
            names.push(if dm.a.code == 1 { "AASHTO LRFD design" } else { "ACI 318 design" });
        }
        if show.shear_peak {
            if let Some((_, s)) = &self.shear {
                if let Some(p) = s.points.iter().max_by(|a, b| a.v.partial_cmp(&b.v).unwrap()) {
                    series.push(Series { dots: true, ..Series::line(vec![[p.m, p.v]], Color32::BLACK) });
                    names.push("Shear analysis peak");
                }
            }
        }
        // the legend sits to the right of the plot, so it never hides the curve
        let legend_w = if rect.width() > 900.0 { 320.0 } else { 0.0 };
        let plot_rect = Rect::from_min_max(rect.min, Pos2::new(rect.max.x - legend_w, rect.max.y)).shrink(10.0);
        let fr = plots::draw(&painter, plot_rect, "M-V Interaction", "Moment (kNm)", "Shear (kN)", &series, None, None);
        // legend, as in the original
        let lx = if legend_w > 0.0 { rect.max.x - legend_w + 8.0 } else { fr.plot.right() - 290.0 };
        let mut ly = fr.plot.top() + 10.0;
        let mut legend = vec![("Strength envelope", BLUE)];
        for (on, name, c) in [
            (show.cracking, "Cracking", RED),
            (show.long_yield, "Longitudinal yield", green),
            (show.trans_yield, "Transverse yield", magenta),
            (show.conc_limit, "Concrete at ecu", lim_c),
            (show.steel_limit, "Steel at its limit", lim_s),
        ] {
            if on {
                legend.push((name, c));
            }
        }
        if dm.is_some() {
            legend.push((if dm.as_ref().map(|m| m.a.code == 1).unwrap_or(false) { "AASHTO LRFD design (φM, φv V)" } else { "ACI 318 design (φM, φv V)" }, design_red));
        }
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(lx - 8.0, ly - 6.0), Vec2::new(192.0, 20.0 * legend.len() as f32 + 12.0)),
            0.0,
            Color32::from_white_alpha(230),
        );
        let legend_h = 20.0 * legend.len() as f32;
        for (name, c) in legend {
            painter.line_segment([Pos2::new(lx, ly + 8.0), Pos2::new(lx + 28.0, ly + 8.0)], Stroke::new(2.0, c));
            painter.text(Pos2::new(lx + 36.0, ly + 8.0), Align2::LEFT_CENTER, name, FontId::proportional(13.0), Color32::BLACK);
            ly += 20.0;
        }
        // each envelope point coloured by what ended its run; hollow = not confirmed or out of
        // line with its neighbours (treat with care)
        let mech_colour = |mch: Mechanism| match mch {
            Mechanism::Flexure => Color32::from_rgb(20, 60, 200),
            Mechanism::LongYield => Color32::from_rgb(0, 150, 0),
            Mechanism::LongHardening => Color32::from_rgb(0, 110, 110),
            Mechanism::WebShear => Color32::from_rgb(230, 120, 0),
            Mechanism::WebCrushing => Color32::from_rgb(170, 0, 0),
            Mechanism::Unclear => Color32::from_rgb(130, 130, 130),
        };
        let clip = painter.with_clip_rect(fr.plot.expand(4.0));
        let mut shown = std::collections::BTreeMap::new();
        let (mut sure, mut unsure, mut inside_n) = (0, 0, 0);
        for (k, p) in r.points.iter().enumerate().filter(|_| show.points) {
            let pos = fr.map(p.0, p.1);
            if r.inside.get(k).copied().unwrap_or(false) {
                // inside the envelope: its run stopped early (another point carries more of both)
                inside_n += 1;
                let g = Color32::from_gray(150);
                clip.line_segment([pos + Vec2::new(-3.5, -3.5), pos + Vec2::new(3.5, 3.5)], Stroke::new(1.3, g));
                clip.line_segment([pos + Vec2::new(-3.5, 3.5), pos + Vec2::new(3.5, -3.5)], Stroke::new(1.3, g));
                continue;
            }
            let inf = r.info.get(k);
            let c = inf.map(|i| mech_colour(i.mech)).unwrap_or(BLUE);
            let doubtful = inf.map(|i| !i.confirmed || i.suspect || i.mech == Mechanism::Unclear).unwrap_or(false);
            if doubtful {
                unsure += 1;
                clip.circle_filled(pos, 4.0, Color32::WHITE);
                clip.circle_stroke(pos, 4.0, Stroke::new(1.6, c));
            } else {
                sure += 1;
                clip.circle_filled(pos, 4.0, c);
            }
            if let Some(i) = inf {
                shown.insert(i.mech.label(), c);
            }
        }
        // the design limits: a marker for each run that reached the limit before its peak, joined
        // only to a neighbouring run that reached it too
        for (on, pts, col) in [(show.conc_limit, &r.conc_limit, lim_c), (show.steel_limit, &r.steel_limit, lim_s)] {
            if !on {
                continue;
            }
            let clip = painter.with_clip_rect(fr.plot.expand(2.0));
            for w in pts.windows(2) {
                if w[0].0.is_finite() && w[1].0.is_finite() {
                    clip.line_segment([fr.map(w[0].0, w[0].1), fr.map(w[1].0, w[1].1)], Stroke::new(1.8, col));
                }
            }
            for q in pts.iter().filter(|q| q.0.is_finite()) {
                let c = fr.map(q.0, q.1);
                clip.rect_filled(Rect::from_center_size(c, Vec2::splat(6.0)), 0.0, col);
            }
        }
        // the two other loading paths at the doubtful points
        let xc = Color32::from_rgb(120, 40, 160);
        let tri = |c: Pos2| vec![c + Vec2::new(0.0, -6.0), c + Vec2::new(5.5, 4.0), c + Vec2::new(-5.5, 4.0)];
        let dia = |c: Pos2| vec![c + Vec2::new(0.0, -6.0), c + Vec2::new(6.0, 0.0), c + Vec2::new(0.0, 6.0), c + Vec2::new(-6.0, 0.0)];
        for c in r.checks.iter().filter(|_| show.checks) {
            let at = fr.map(c.m, c.v);
            let mut shape = |pts: Vec<Pos2>, ok: bool, to: Pos2| {
                clip.line_segment([at, to], Stroke::new(1.0, xc.gamma_multiply(0.6)));
                if ok {
                    clip.add(egui::Shape::convex_polygon(pts, xc, Stroke::NONE));
                } else {
                    clip.add(egui::Shape::closed_line(pts, Stroke::new(1.4, xc)));
                }
            };
            // only checks that ended in a real failure are drawn (the others are in the table):
            // a stopped check says nothing about where the envelope is
            if let Some((v, true)) = c.fixed_m {
                let p = fr.map(c.m, v);
                shape(tri(p), true, p);
            }
            if let Some((mm, true)) = c.fixed_v {
                let p = fr.map(mm, c.v);
                shape(dia(p), true, p);
            }
        }
        // the marker legend lists only the layers that are switched on
        let show_checks = show.checks && !r.checks.is_empty();
        let mut my = fr.plot.top() + 30.0 + legend_h;
        let rows = if show.points { shown.len() as f32 + 1.0 } else { 0.0 } + if show_checks { 2.0 } else { 0.0 };
        if rows > 0.0 {
            let mh = 20.0 * rows + 8.0;
            painter.rect_filled(Rect::from_min_size(Pos2::new(lx - 8.0, my - 6.0), Vec2::new(300.0, mh)), 0.0, Color32::from_white_alpha(230));
        }
        if show.points {
            for (name, c) in &shown {
                painter.circle_filled(Pos2::new(lx + 6.0, my + 8.0), 4.0, *c);
                painter.text(Pos2::new(lx + 18.0, my + 8.0), Align2::LEFT_CENTER, *name, FontId::proportional(12.0), Color32::BLACK);
                my += 20.0;
            }
            painter.circle_stroke(Pos2::new(lx + 6.0, my + 8.0), 4.0, Stroke::new(1.6, Color32::GRAY));
            painter.text(Pos2::new(lx + 18.0, my + 8.0), Align2::LEFT_CENTER, "hollow: solver stop, capacity may be higher", FontId::proportional(12.0), Color32::BLACK);
            my += 20.0;
        }
        if show_checks {
            painter.add(egui::Shape::convex_polygon(tri(Pos2::new(lx + 6.0, my + 9.0)), xc, Stroke::NONE));
            painter.text(Pos2::new(lx + 18.0, my + 8.0), Align2::LEFT_CENTER, "check: moment held, shear raised", FontId::proportional(12.0), Color32::BLACK);
            my += 20.0;
            painter.add(egui::Shape::convex_polygon(dia(Pos2::new(lx + 6.0, my + 8.0)), xc, Stroke::NONE));
            painter.text(Pos2::new(lx + 18.0, my + 8.0), Align2::LEFT_CENTER, "check: shear held, moment raised", FontId::proportional(12.0), Color32::BLACK);
        }
        // the counts sit in the legend column, not over the plot
        if legend_w > 0.0 && show.points {
            painter.text(
                Pos2::new(lx, fr.plot.bottom() - 4.0),
                Align2::LEFT_BOTTOM,
                format!("{} confirmed, {} uncertain,\n{} inside the envelope", sure, unsure, inside_n),
                FontId::proportional(12.0),
                Color32::DARK_GRAY,
            );
        }
        // values read off the lines under the pointer (exact at computed points, interpolated between)
        let lim_pts: Vec<(bool, &str, Vec<[f64; 2]>, Color32)> = vec![
            (show.conc_limit, "Concrete at ecu", to(&r.conc_limit), lim_c),
            (show.steel_limit, "Steel at its limit", to(&r.steel_limit), lim_s),
        ];
        let mut probes: Vec<plots::Probe> = series.iter().zip(&names).map(|(s, n)| plots::Probe { name: n, pts: &s.pts, color: s.color }).collect();
        probes.extend(lim_pts.iter().filter(|l| l.0).map(|l| plots::Probe { name: l.1, pts: &l.2, color: l.3 }));
        plots::readout(ui, &painter, &fr, "mv", &probes, ("M", "kNm"), ("V", "kN"), None);
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    /// Biaxial bending: the inputs (axial load, load angle, directions), the Mx-My capacity
    /// diagram at that axial load with the chosen angle marked, the moment-curvature curve at
    /// that angle, and the section with the neutral axis at its peak (biaxial.rs).
    fn biaxial_view(&mut self, ui: &mut egui::Ui) {
        let Some(sec) = self.sec.clone() else { return };
        ui.label(
            RichText::new(
                "Biaxial bending: the moment acts at a load angle α, Mx = M cos α (about the horizontal axis, + compresses the top) \
                 and My = M sin α (about the vertical axis, + compresses the right side). No shear.",
            )
            .strong(),
        );
        let idle = self.job.is_none();
        ui.horizontal(|ui| {
            ui.label("Axial load N");
            ui.add(egui::DragValue::new(&mut self.bi_n).speed(10.0).suffix(" kN"));
            ui.label("(tension +)");
            ui.separator();
            ui.label("Load angle α");
            ui.add(egui::DragValue::new(&mut self.bi_alpha).speed(1.0).range(-360.0..=360.0).suffix("°"));
            for a in [0.0, 30.0, 45.0, 60.0, 90.0] {
                if ui.small_button(format!("{:.0}°", a)).clicked() {
                    self.bi_alpha = a;
                }
            }
            ui.separator();
            ui.label("Directions");
            ui.add(egui::Slider::new(&mut self.bi_dirs, 8..=72));
            if ui.add_enabled(idle, egui::Button::new(RichText::new("Calculate").strong())).clicked() {
                self.start(Job::Biaxial);
            }
            if ui.small_button("N from Loads").on_hover_text("Use the section's constant axial load (Define Loading)").clicked() {
                self.bi_n = sec.loads.axial;
            }
        });
        let Some(d) = self.biax.clone() else {
            ui.add_space(20.0);
            ui.label("Set the axial load and the load angle, then Calculate. Each direction is a full plane-sections run to failure.");
            return;
        };
        let aci = sec.aci.as_ref().map(|a| a.for_section(&sec));
        let run = &d.chosen;
        if let Some(p) = run.peak_point() {
            let mut line = format!(
                "At α = {:.1}° and N = {:.0} kN: capacity M = {:.1} kNm (Mx = {:.1}, My = {:.1}) at {:.2} rad/km; curvature direction {:.1}° (neutral axis tilted {:.1}° from the load angle). End: {}",
                run.alpha,
                d.n,
                p.m,
                p.mx,
                p.my,
                p.kappa,
                p.beta,
                p.beta - run.alpha,
                run.stop
            );
            if let Some(a) = &aci {
                if let Some((phi, et)) = crate::biaxial::aci_phi(run, a) {
                    line.push_str(&format!("   {}: φ = {:.3} (εt = {:.2} mm/m), φM = {:.1} kNm", a.name(), phi, et * 1000.0, phi * p.m));
                }
            }
            ui.label(RichText::new(line).strong());
        }
        if (run.alpha - self.bi_alpha).abs() > 1e-9 || (d.n - self.bi_n).abs() > 1e-9 {
            ui.colored_label(Color32::from_rgb(170, 60, 0), "The inputs changed since this result: Calculate again to update it.");
        }
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let sk_w = (rect.width() * 0.22).clamp(160.0, 320.0);
        let gap = 8.0;
        let rest = rect.width() - sk_w - 2.0 * gap;
        let r_sk = Rect::from_min_size(rect.min, Vec2::new(sk_w, rect.height()));
        // Mx-My capacity diagram (equal scales, so the shape reads true)
        let cont = d.contour();
        let mut series = vec![Series { width: 2.0, ..Series::line(cont.iter().map(|p| [p.0, p.1]).collect(), BLUE) }];
        if let Some(a) = &aci {
            let des: Vec<[f64; 2]> = d
                .runs
                .iter()
                .chain(std::iter::once(&d.runs[0]))
                .filter_map(|r| {
                    let p = r.peak_point()?;
                    let phi = crate::biaxial::aci_phi(r, a).map(|x| x.0).unwrap_or(a.phi_compression());
                    Some([phi * p.mx, phi * p.my])
                })
                .collect();
            series.push(Series { width: 2.0, ..Series::line(des, Color32::BLACK) });
        }
        if let Some(p) = run.peak_point() {
            series.push(Series { width: 1.2, ..Series::line(vec![[0.0, 0.0], [p.mx, p.my]], RED) });
            series.push(Series { dots: true, width: 0.0, ..Series::line(vec![[p.mx, p.my]], RED) });
        }
        // layout, scaled to the window: a wide diagram goes on top of the moment-curvature plot,
        // a tall (or round) one beside it; either way it gets as much room as its shape can use
        let ratio = plots::data_aspect(&series) as f32;
        let right = Rect::from_min_max(Pos2::new(rect.min.x + sk_w + gap, rect.min.y), rect.max);
        let (r_c, r_m) = if ratio > 1.3 {
            let h = (rest / ratio + 60.0).clamp(rect.height() * 0.4, rect.height() * 0.65);
            let r_c = Rect::from_min_size(right.min, Vec2::new(rest + gap, h));
            (r_c, Rect::from_min_max(Pos2::new(right.min.x, r_c.max.y + gap), right.max))
        } else {
            let w = (rect.height() * ratio + 60.0).clamp(rest * 0.4, rest * 0.65);
            let r_c = Rect::from_min_size(right.min, Vec2::new(w, rect.height()));
            (r_c, Rect::from_min_max(Pos2::new(r_c.max.x + gap, right.min.y), right.max))
        };
        let (xr, yr) = plots::equal_ranges(r_c.shrink(6.0), &series, 44.0);
        let fr = plots::draw(&painter, r_c.shrink(6.0), &format!("Mx-My capacity at N = {:.0} kN", d.n), "Mx (kNm)", "My (kNm)", &series, Some(yr), Some(xr));
        let clip = painter.with_clip_rect(fr.plot);
        clip.line_segment([fr.map(fr.x0, 0.0), fr.map(fr.x1, 0.0)], Stroke::new(0.8, Color32::GRAY));
        clip.line_segment([fr.map(0.0, fr.y0), fr.map(0.0, fr.y1)], Stroke::new(0.8, Color32::GRAY));
        let mut ly = fr.plot.top() + 10.0;
        let lx = fr.plot.left() + 10.0;
        let mut leg = vec![("nominal capacity", BLUE), ("chosen load angle", RED)];
        if aci.is_some() {
            leg.insert(1, (if aci.as_ref().map(|a| a.code == 1).unwrap_or(false) { "AASHTO LRFD design (φ × nominal)" } else { "ACI 318 design (φ × nominal)" }, Color32::BLACK));
        }
        for (name, c) in leg {
            painter.line_segment([Pos2::new(lx, ly), Pos2::new(lx + 24.0, ly)], Stroke::new(2.0, c));
            painter.text(Pos2::new(lx + 30.0, ly), Align2::LEFT_CENTER, name, FontId::proportional(12.0), Color32::BLACK);
            ly += 18.0;
        }
        // values read off the curves under the pointer: series 0 is the nominal contour, then the
        // design contour (if any), then the chosen angle's line and peak
        {
            let mut probes = vec![plots::Probe { name: "Nominal capacity", pts: &series[0].pts, color: BLUE }];
            let mut k = 1;
            if aci.is_some() {
                probes.push(plots::Probe { name: if aci.as_ref().map(|a| a.code == 1).unwrap_or(false) { "AASHTO LRFD design" } else { "ACI 318 design" }, pts: &series[1].pts, color: Color32::BLACK });
                k = 2;
            }
            if run.peak_point().is_some() {
                probes.push(plots::Probe { name: "Peak at the chosen angle", pts: &series[k + 1].pts, color: RED });
            }
            let polar = |mx: f64, my: f64| format!("M = {:.1} kNm at α = {:.1}°", mx.hypot(my), my.atan2(mx).to_degrees());
            plots::readout(ui, &painter, &fr, "mxmy", &probes, ("Mx", "kNm"), ("My", "kNm"), Some(&polar));
        }
        // moment-curvature at the chosen angle
        let mk: Vec<[f64; 2]> = run.points.iter().map(|p| [p.kappa, p.m]).collect();
        let mut s2 = vec![Series { width: 2.0, ..Series::line(mk, BLUE) }];
        if let Some(p) = run.peak_point() {
            s2.push(Series { dots: true, width: 0.0, ..Series::line(vec![[p.kappa, p.m]], RED) });
        }
        plots::draw(&painter, r_m.shrink(6.0), &format!("Moment - curvature at α = {:.1}°", run.alpha), "Curvature (rad/km)", "M along α (kNm)", &s2, None, None);
        // the section with its bars and the neutral axis at the peak
        let h = sec.height();
        let bmax = (0..=40).map(|i| sec.width_at(h * i as f64 / 40.0)).fold(1.0, f64::max);
        let area = r_sk.shrink(24.0);
        let sc = ((area.width() as f64) / bmax).min((area.height() as f64 * 0.6) / h);
        let c0 = Pos2::new(area.center().x, area.top() + (h * sc) as f32 + 20.0);
        let map = |x: f64, z: f64| Pos2::new(c0.x + (x * sc) as f32, c0.y - (z * sc) as f32);
        painter.text(Pos2::new(r_sk.center().x, r_sk.top() + 8.0), Align2::CENTER_TOP, "Section at the peak", FontId::proportional(14.0), Color32::BLACK);
        let mut left = vec![];
        let mut right = vec![];
        for i in 0..=60 {
            let z = h * i as f64 / 60.0;
            let w = sec.width_at(z.clamp(h * 0.0001, h * 0.9999));
            left.push(map(-w / 2.0, z));
            right.push(map(w / 2.0, z));
        }
        right.reverse();
        let mut outline = left;
        outline.extend(right);
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.4, Color32::BLACK)));
        for l in &sec.long {
            for x in crate::section_page::bar_xs(&sec, l) {
                painter.circle_filled(map(x, l.z), ((l.draw_dia() / 2.0) * sc).max(2.0) as f32, Color32::from_gray(60));
            }
        }
        if let Some(p) = run.peak_point() {
            // the neutral axis: e0 - kx (z - zc) - ky x = 0; the compression side shaded by a mark
            let k = p.kappa * 1e-6;
            let (kx, ky) = (k * p.beta.to_radians().cos(), k * p.beta.to_radians().sin());
            let zc = (0..400).map(|i| (i as f64 + 0.5) * h / 400.0).map(|z| (z * sec.width_at(z), sec.width_at(z))).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            let zc = if zc.1 > 0.0 { zc.0 / zc.1 } else { h / 2.0 };
            let ext = 2.0 * bmax.max(h);
            // a point on the line and its direction
            let nrm = (ky, kx); // gradient of -strain in (x, z)
            let len2 = nrm.0 * nrm.0 + nrm.1 * nrm.1;
            if len2 > 0.0 {
                let t = p.e0 / len2;
                let (px, pz) = (t * nrm.0, zc + t * nrm.1);
                let dir = (-nrm.1, nrm.0);
                let dl = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
                let (ux, uz) = (dir.0 / dl, dir.1 / dl);
                let clip = painter.with_clip_rect(Rect::from_min_max(map(-bmax / 2.0, h), map(bmax / 2.0, 0.0)).expand(14.0));
                clip.line_segment([map(px - ext * ux, pz - ext * uz), map(px + ext * ux, pz + ext * uz)], Stroke::new(2.0, RED));
                // the arrow of the moment vector (load angle) through the centroid
                let a = run.alpha.to_radians();
                let r = 0.35 * bmax.min(h);
                let (ex, ez) = (r * a.sin(), zc + r * a.cos());
                painter.arrow(map(0.0, zc), map(ex, ez) - map(0.0, zc), Stroke::new(1.6, Color32::from_rgb(0, 120, 0)));
                painter.text(
                    Pos2::new(r_sk.center().x, c0.y + 16.0),
                    Align2::CENTER_TOP,
                    "red: neutral axis at the peak\ngreen: towards the most compressed side",
                    FontId::proportional(12.0),
                    Color32::DARK_GRAY,
                );
            }
        }
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    /// Shear in two directions (shear2d.rs): the inputs, a check of a demand (Vux, Vuy) acting
    /// together, and the Vx-Vy capacity diagram with the demand marked.
    fn shear2d_view(&mut self, ui: &mut egui::Ui) {
        let Some(sec) = self.sec.clone() else { return };
        ui.label(
            RichText::new(
                "Shear in two directions: the shear acts at an angle θ from the vertical, Vy = V cos θ (vertical, with Mx) and \
                 Vx = V sin θ (sideways, with My), with the moment M = (M/V) × V at the same angle. Each direction is a full \
                 sectional shear analysis (MCFT) of the section turned to it.",
            )
            .strong(),
        );
        let idle = self.job.is_none();
        ui.horizontal(|ui| {
            ui.label("Axial load N");
            ui.add(egui::DragValue::new(&mut self.s2_n).speed(10.0).suffix(" kN"));
            ui.label("(tension +)");
            ui.separator();
            ui.label("M/V");
            ui.add(egui::DragValue::new(&mut self.s2_mv).speed(0.05).range(0.0..=100.0).suffix(" m"));
            ui.label("(0 = shear only)");
            ui.separator();
            ui.label("Directions");
            ui.add(egui::Slider::new(&mut self.s2_dirs, 4..=36));
            if ui.add_enabled(idle, egui::Button::new(RichText::new("Calculate").strong())).clicked() {
                self.start(Job::Shear2d);
            }
            if ui.small_button("N from Loads").clicked() {
                self.s2_n = sec.loads.axial;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Check a demand acting together:").strong());
            ui.label("Vux");
            ui.add(egui::DragValue::new(&mut self.s2_vu.0).speed(5.0).suffix(" kN"));
            ui.label("Vuy");
            ui.add(egui::DragValue::new(&mut self.s2_vu.1).speed(5.0).suffix(" kN"));
        });
        let Some(d) = self.s2.clone() else {
            ui.add_space(20.0);
            ui.label("Set the axial load and M/V, then Calculate. The section's left-right symmetry is used: directions 0 to 180 degrees are run, the rest mirrored.");
            return;
        };
        if (d.n - self.s2_n).abs() > 1e-9 || (d.mv - self.s2_mv).abs() > 1e-9 {
            ui.colored_label(Color32::from_rgb(170, 60, 0), "The inputs changed since this result: Calculate again to update it.");
        }
        let (vux, vuy) = self.s2_vu;
        let (th, cap, ratio) = crate::shear2d::check(&d, vux, vuy);
        let vu = vux.hypot(vuy);
        let ok_col = |ok: bool| if ok { Color32::from_rgb(0, 120, 0) } else { Color32::from_rgb(180, 30, 30) };
        ui.label(
            RichText::new(format!(
                "Demand |Vu| = {:.1} kN at θ = {:.1}°: nominal capacity there Vn = {:.1} kN, ratio Vu/Vn = {:.2}{}",
                vu,
                th,
                cap,
                ratio,
                if ratio <= 1.0 { " - inside the nominal capacity" } else { " - outside the nominal capacity" }
            ))
            .strong()
            .color(ok_col(ratio <= 1.0)),
        );
        let aci = sec.aci.clone();
        if let Some(a) = aci.as_ref().filter(|a| a.code == 1) {
            // AASHTO LRFD: the shear check along the demand's direction (no two-direction rule)
            let dr = ratio / a.phi_shear;
            ui.label(
                RichText::new(format!("AASHTO LRFD: Vu/(φv Vn) = {:.2} along the demand's direction (φv = {:.2})", dr, a.phi_shear))
                    .strong()
                    .color(ok_col(dr <= 1.0)),
            );
        }
        if let Some(a) = aci.as_ref().filter(|a| a.code == 0) {
            let dr = ratio / a.phi_shear;
            let (rx, ry, g, lim, pass) = crate::shear2d::aci_two_way(&d, a.phi_shear, vux, vuy);
            ui.label(
                RichText::new(format!(
                    "{}: Vu/(φv Vn) = {:.2} along the demand's direction (φv = {:.2}).   Two-direction rule on φv Vn at 0/180° and 90/270°: \
                     Vux/φVnx = {:.2}, Vuy/φVny = {:.2}; {} = {:.2}, limit {:.1}: {}",
                    a.name(),
                    dr,
                    a.phi_shear,
                    rx,
                    ry,
                    if lim > 1.0 { "sum" } else { "larger ratio" },
                    g,
                    lim,
                    if pass { "passes" } else { "fails" }
                ))
                .strong()
                .color(ok_col(pass && dr <= 1.0)),
            );
        }
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let cont = d.contour();
        let mut series = vec![Series { width: 2.0, ..Series::line(cont.iter().map(|p| [p.0, p.1]).collect(), BLUE) }];
        // reference ellipse through the four principal capacities: (Vx/Vnx)^2 + (Vy/Vny)^2 = 1
        let v0 = d.capacity_at(0.0);
        let v90 = d.capacity_at(90.0);
        let v180 = d.capacity_at(180.0);
        let v270 = d.capacity_at(270.0);
        let ell: Vec<[f64; 2]> = (0..=96)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / 96.0;
                let (sx, cy) = (t.sin(), t.cos());
                let vx = if sx >= 0.0 { v90 } else { v270 };
                let vy = if cy >= 0.0 { v0 } else { v180 };
                [vx * sx, vy * cy]
            })
            .collect();
        series.push(Series { width: 1.0, ..Series::line(ell, Color32::from_gray(150)) });
        let design_black = Color32::BLACK;
        if let Some(a) = &aci {
            let des: Vec<[f64; 2]> = cont.iter().map(|p| [a.phi_shear * p.0, a.phi_shear * p.1]).collect();
            series.push(Series { width: 2.0, ..Series::line(des, design_black) });
            // the ACI two-direction rule's boundary in each quadrant, on phi Vn at 0/90/180/270
            let mut rule = vec![];
            for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)] {
                let vnx = a.phi_shear * if sx > 0.0 { v90 } else { v270 };
                let vny = a.phi_shear * if sy > 0.0 { v0 } else { v180 };
                let q = [(0.0, 1.0), (0.5, 1.0), (1.0, 0.5), (1.0, 0.0)];
                let pts: Vec<[f64; 2]> = if sx * sy > 0.0 { q.iter().map(|p| [sx * p.0 * vnx, sy * p.1 * vny]).collect() } else { q.iter().rev().map(|p| [sx * p.0 * vnx, sy * p.1 * vny]).collect() };
                rule.extend(pts);
            }
            if let Some(f) = rule.first().copied() {
                rule.push(f);
            }
            if a.code == 0 {
                series.push(Series { width: 1.2, ..Series::line(rule, Color32::from_rgb(120, 60, 160)) });
            }
        }
        series.push(Series { width: 1.2, ..Series::line(vec![[0.0, 0.0], [vux, vuy]], RED) });
        series.push(Series { dots: true, width: 0.0, ..Series::line(vec![[vux, vuy]], RED) });
        // the plot takes the full height and as much width as its shape can use (equal scales, so
        // the shape reads true), leaving room on the right for the legend and the table
        let table_w = 450.0_f32.min(rect.width() * 0.45);
        let w = (rect.height() * plots::data_aspect(&series) as f32 + 60.0).clamp(rect.width() * 0.35, (rect.width() - table_w).max(rect.width() * 0.35));
        let r_c = Rect::from_min_size(rect.min, Vec2::new(w, rect.height()));
        let title = format!("Vx-Vy shear capacity at N = {:.0} kN, M/V = {:.2} m", d.n, d.mv);
        let (xr, yr) = plots::equal_ranges(r_c.shrink(6.0), &series, 0.0);
        let fr = plots::draw(&painter, r_c.shrink(6.0), &title, "Vx (kN, sideways)", "Vy (kN, vertical)", &series, Some(yr), Some(xr));
        let clip = painter.with_clip_rect(fr.plot);
        clip.line_segment([fr.map(fr.x0, 0.0), fr.map(fr.x1, 0.0)], Stroke::new(0.8, Color32::GRAY));
        clip.line_segment([fr.map(0.0, fr.y0), fr.map(0.0, fr.y1)], Stroke::new(0.8, Color32::GRAY));
        // each direction's point, coloured by what governed it
        for r in &d.runs {
            let c = match r.mech {
                Some(Mechanism::WebShear) => Color32::from_rgb(230, 120, 0),
                Some(Mechanism::WebCrushing) => Color32::from_rgb(170, 0, 0),
                Some(Mechanism::LongYield) => Color32::from_rgb(0, 150, 0),
                Some(Mechanism::LongHardening) => Color32::from_rgb(0, 110, 110),
                Some(Mechanism::Flexure) => Color32::from_rgb(20, 60, 200),
                _ => Color32::from_rgb(130, 130, 130),
            };
            clip.circle_filled(fr.map(r.vx(), r.vy()), 4.0, c);
        }
        // values read off the curves under the pointer: series 0 nominal, 1 the ellipse, then the
        // design contour and the ACI rule (if shown), then the demand line and its point
        {
            let mut probes = vec![
                plots::Probe { name: "Nominal capacity", pts: &series[0].pts, color: BLUE },
                plots::Probe { name: "Ellipse through the 0/90/180/270° capacities", pts: &series[1].pts, color: Color32::from_gray(150) },
            ];
            let mut k = 2;
            if let Some(a) = &aci {
                probes.push(plots::Probe { name: if a.code == 1 { "AASHTO LRFD design" } else { "ACI 318 design" }, pts: &series[k].pts, color: design_black });
                k += 1;
                if a.code == 0 {
                    probes.push(plots::Probe { name: "ACI 318 two-direction rule", pts: &series[k].pts, color: Color32::from_rgb(120, 60, 160) });
                    k += 1;
                }
            }
            probes.push(plots::Probe { name: "Demand (Vux, Vuy)", pts: &series[k + 1].pts, color: RED });
            let polar = |vx: f64, vy: f64| format!("V = {:.1} kN at θ = {:.1}° (from vertical)", vx.hypot(vy), vx.atan2(vy).to_degrees());
            plots::readout(ui, &painter, &fr, "vxvy", &probes, ("Vx", "kN"), ("Vy", "kN"), Some(&polar));
        }
        // legend and the table of directions, to the right
        let lx = r_c.max.x + 16.0;
        let mut ly = rect.min.y + 16.0;
        let mut leg = vec![("nominal capacity (MCFT, each direction)", BLUE), ("ellipse through the 0/90/180/270° capacities", Color32::from_gray(150)), ("demand (Vux, Vuy)", RED)];
        if aci.is_some() {
            leg.insert(1, (if aci.as_ref().map(|a| a.code == 1).unwrap_or(false) { "AASHTO LRFD design, φv × nominal" } else { "ACI 318 design, φv × nominal" }, design_black));
            if aci.as_ref().map(|a| a.code == 0).unwrap_or(false) {
                leg.insert(2, ("ACI 318 two-direction rule (ratios 0.5 / sum 1.5)", Color32::from_rgb(120, 60, 160)));
            }
        }
        for (name, c) in leg {
            painter.line_segment([Pos2::new(lx, ly), Pos2::new(lx + 26.0, ly)], Stroke::new(2.0, c));
            painter.text(Pos2::new(lx + 32.0, ly), Align2::LEFT_CENTER, name, FontId::proportional(12.5), Color32::BLACK);
            ly += 19.0;
        }
        ly += 10.0;
        painter.text(Pos2::new(lx, ly), Align2::LEFT_TOP, "θ°      V kN    Vx      Vy     M kNm  ties mm²  governed by", FontId::monospace(10.5), Color32::BLACK);
        ly += 16.0;
        let mut rows: Vec<&crate::shear2d::VRun> = d.runs.iter().collect();
        rows.sort_by(|a, b| a.theta.partial_cmp(&b.theta).unwrap());
        for r in rows {
            let mech = r.mech.map(|m| m.label()).unwrap_or("-");
            painter.text(
                Pos2::new(lx, ly),
                Align2::LEFT_TOP,
                format!("{:5.1} {:7.1} {:7.1} {:7.1} {:7.1} {:7.1}  {}", r.theta, r.v, r.vx(), r.vy(), r.m, r.av, mech.split(" (").next().unwrap_or(mech)),
                FontId::monospace(10.5),
                Color32::DARK_GRAY,
            );
            ly += 15.0;
        }
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    /// Load cases (demand.rs): a pasted table of Pu, Mx, My, Vx, Vy, Tu, and each case's D/C
    /// ratios for axial load with bending, shear, torsion and shear with torsion.
    fn load_cases_view(&mut self, ui: &mut egui::Ui) {
        let Some(sec) = self.sec.clone() else { return };
        let code = sec.aci.as_ref().map(|a| a.name()).unwrap_or(if is_factored(&sec.phi) { "factored materials (CSA)" } else { "nominal (no φ)" });
        ui.label(
            RichText::new(format!(
                "Load cases: one a line, Pu Mx My Vx Vy Tu (an optional name first). Paste straight from a spreadsheet: tabs, commas, \
                 semicolons or spaces; header lines are skipped. Pu in kN, compression +; Mx, My, Tu in kNm; Vx, Vy in kN. Design basis: {}.",
                code
            ))
            .strong(),
        );
        ui.label(
            RichText::new(
                "Mx bends about the horizontal axis (+ compresses the top), My about the vertical axis (+ compresses the right side); Vy is the \
                 vertical shear (with Mx), Vx the sideways one (with My). P-M: the biaxial capacity at Pu along the moment's direction; V: the \
                 sectional shear capacity along the shear's direction with its M/V; T and V+T: ACI 318 or AASHTO LRFD torsion (Options > Design Factors).",
            )
            .small()
            .weak(),
        );
        let idle = self.job.is_none();
        let n_cases = crate::demand::parse(&self.lc_text).len();
        ui.horizontal(|ui| {
            if ui.add_enabled(idle && n_cases > 0, egui::Button::new(RichText::new(format!("Calculate {} case{}", n_cases, if n_cases == 1 { "" } else { "s" })).strong())).clicked() {
                self.start(Job::LoadCases);
            }
            if ui.button("Clear table").on_hover_text("Empty the box, then paste your table into it (Ctrl+V)").clicked() {
                self.lc_text.clear();
            }
            if let Some(rows) = &self.lc_rows {
                if ui.button("Copy results").on_hover_text("The results as a tab-separated table, to paste into a spreadsheet").clicked() {
                    ui.ctx().copy_text(crate::demand::to_tsv(rows));
                }
            }
            ui.label(format!("{} case{} read from the table", n_cases, if n_cases == 1 { "" } else { "s" }));
        });
        ui.columns(2, |cols| {
            cols[0].label(RichText::new("Table").strong());
            egui::ScrollArea::vertical().id_salt("lc_in").max_height(cols[0].available_height() * 0.35).show(&mut cols[0], |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.lc_text).font(egui::TextStyle::Monospace).desired_rows(8).desired_width(f32::INFINITY));
            });
            cols[1].label(RichText::new("Read as").strong());
            egui::ScrollArea::vertical().id_salt("lc_read").max_height(cols[1].available_height() * 0.35).show(&mut cols[1], |ui| {
                egui::Grid::new("lc_parsed").striped(true).show(ui, |ui| {
                    for h in ["Case", "Pu kN", "Mx kNm", "My kNm", "Vx kN", "Vy kN", "Tu kNm"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for c in crate::demand::parse(&self.lc_text) {
                        ui.label(&c.name);
                        for v in [c.pu, c.mx, c.my, c.vx, c.vy, c.tu] {
                            ui.label(format!("{:.1}", v));
                        }
                        ui.end_row();
                    }
                });
            });
        });
        ui.separator();
        let Some(rows) = self.lc_rows.clone() else {
            ui.label("Calculate to get the D/C ratios. Each case runs a biaxial bending analysis and a sectional shear analysis, so a long table takes a while.");
            return;
        };
        let worst = rows.iter().filter(|r| !r.dc_max.is_nan()).max_by(|a, b| a.dc_max.partial_cmp(&b.dc_max).unwrap());
        if let Some(w) = worst {
            let ok = w.dc_max <= 1.0;
            ui.label(
                RichText::new(format!(
                    "Largest D/C = {} in {} ({}): {}",
                    if w.dc_max.is_finite() { format!("{:.3}", w.dc_max) } else { "beyond capacity".into() },
                    w.case.name,
                    w.governs,
                    if ok { "every case passes" } else { "at least one case fails" }
                ))
                .strong()
                .color(if ok { Color32::from_rgb(0, 120, 0) } else { Color32::from_rgb(180, 30, 30) }),
            );
        }
        let dc = |ui: &mut egui::Ui, v: f64| {
            if v.is_nan() {
                ui.label("-");
            } else if v.is_infinite() {
                ui.label(RichText::new("beyond").strong().color(Color32::from_rgb(180, 30, 30)));
            } else {
                let c = if v > 1.0 { Color32::from_rgb(180, 30, 30) } else if v > 0.9 { Color32::from_rgb(190, 110, 0) } else { Color32::from_rgb(0, 120, 0) };
                ui.label(RichText::new(format!("{:.3}", v)).strong().color(c));
            }
        };
        egui::ScrollArea::both().id_salt("lc_out").show(ui, |ui| {
            egui::Grid::new("lc_res").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                for h in ["Case", "Pu", "Mx", "My", "Vx", "Vy", "Tu", "φMn kNm", "φVn kN", "φTn kNm", "D/C P-M", "D/C V", "D/C T", "D/C V+T", "D/C max", "governs", "notes"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for r in &rows {
                    let c = &r.case;
                    ui.label(&c.name);
                    for v in [c.pu, c.mx, c.my, c.vx, c.vy, c.tu] {
                        ui.label(format!("{:.1}", v));
                    }
                    ui.label(if r.mn > 0.0 { format!("{:.1}", r.phi_m * r.mn) } else { "-".into() });
                    ui.label(if r.vn > 0.0 { format!("{:.1}", r.vn) } else { "-".into() });
                    ui.label(if r.phi_tn > 0.0 { format!("{:.1}", r.phi_tn) } else { "-".into() });
                    dc(ui, r.dc_pm);
                    dc(ui, r.dc_v);
                    dc(ui, r.dc_t);
                    dc(ui, r.dc_vt);
                    dc(ui, r.dc_max);
                    ui.label(&r.governs);
                    let short: String = r.notes.chars().take(60).collect();
                    let resp = ui.label(if r.notes.chars().count() > 60 { format!("{}…", short) } else { short });
                    if !r.notes.is_empty() {
                        resp.on_hover_text(&r.notes);
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn radial_view(&mut self, ui: &mut egui::Ui) {
        let dm = self.aci();
        let (n_axial, neg) = self
            .sec
            .as_ref()
            .map(|s| (s.loads.axial, s.loads.negative || s.loads.m_const < 0.0))
            .unwrap_or((0.0, false));
        let Some(d) = self.radial.clone() else { return };
        let (title, xl, yl, what) = match d.pair {
            Pair::MV => (
                "M-V Interaction (centred runs, as the original)",
                "Moment (kNm)",
                "Shear (kN)",
                "Moment - shear interaction with the original's run layout: pure positive and negative bending first, then \
                 each run holds the centre moment (their mean) and loads out from it in one direction until it fails.",
            ),
            Pair::MN => (
                "M-N Interaction",
                "Moment (kNm)",
                "Axial load (kN, tension +)",
                "Moment - axial load interaction (no shear): the bending capacity at each axial load (moment-curvature runs at \
                 constant axial load), closed by the pure-compression and pure-tension capacities.",
            ),
            Pair::NV => (
                "N-V Interaction",
                "Axial load (kN, tension +)",
                "Shear (kN)",
                "Axial load - shear interaction at the section's constant moment: each point is a full sectional analysis with \
                 the axial load and the shear rising together along one direction, taken to its peak (as the original does).",
            ),
        };
        ui.horizontal(|ui| {
            ui.label("Points per side:");
            ui.add(egui::Slider::new(&mut self.mv_points, 4..=40));
            let idle = self.job.is_none();
            if ui.add_enabled(idle, egui::Button::new("Recalculate")).clicked() {
                self.start(match d.pair {
                    Pair::MN => Job::MN,
                    Pair::NV => Job::NV,
                    Pair::MV => Job::MVC,
                });
            }
            ui.label(format!("{} points", d.points.len()));
        });
        ui.label(RichText::new("How to read this plot (hover)").small().weak()).on_hover_text(what);
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        // M-V: the same envelope test as the default M-V diagram (points whose run stopped early
        // are marked, not joined), and the line in direction order
        let inside: Vec<bool> = if d.pair == Pair::MV {
            let pts: Vec<(f64, f64)> = d.points.iter().map(|p| (p.x, p.y)).collect();
            let sure: Vec<bool> = d.points.iter().map(|p| p.confirmed).collect();
            analysis::inside_flags(&pts, &sure)
        } else {
            vec![false; d.points.len()]
        };
        let mut live: Vec<(f64, f64)> = d.points.iter().zip(&inside).filter(|(_, i)| !**i).map(|(p, _)| (p.x, p.y)).collect();
        if d.pair == Pair::MV {
            let polar = analysis::mv_polar(&live);
            let mut order: Vec<usize> = (0..live.len()).collect();
            order.sort_by(|a, b| polar[*b].0.partial_cmp(&polar[*a].0).unwrap_or(std::cmp::Ordering::Equal));
            live = order.into_iter().map(|i| live[i]).collect();
        }
        let line: Vec<[f64; 2]> = live.iter().map(|p| [p.0, p.1]).collect();
        let mut series = vec![Series { width: 2.0, ..Series::line(line, BLUE) }];
        let design_red = Color32::BLACK;
        if let Some(dm) = &dm {
            // ACI 318 design curve: M-N scaled radially by phi(et) and cut at phi Pn,max; N-V:
            // phi(et) on N (cut the same way), phi_v on V; M-V: phi(et) on M, phi_v on V
            let pts: Vec<(f64, f64)> = live.clone();
            let des = match d.pair {
                Pair::MN => dm.mn(&pts),
                Pair::NV => dm.nv(&pts, neg),
                Pair::MV => dm.mv(&pts, n_axial),
            };
            series.push(Series { width: 2.0, ..Series::line(des.iter().map(|p| [p.0, p.1]).collect(), design_red) });
        }
        let fr = plots::draw(&painter, rect.shrink(10.0), title, xl, yl, &series, None, None);
        if dm.is_some() {
            let at = Pos2::new(fr.plot.right() - 260.0, fr.plot.top() + 12.0);
            painter.line_segment([at, at + Vec2::new(28.0, 0.0)], Stroke::new(2.0, BLUE));
            painter.text(at + Vec2::new(36.0, 0.0), Align2::LEFT_CENTER, "nominal", FontId::proportional(13.0), Color32::BLACK);
            let at = at + Vec2::new(0.0, 20.0);
            painter.line_segment([at, at + Vec2::new(28.0, 0.0)], Stroke::new(2.0, design_red));
            painter.text(at + Vec2::new(36.0, 0.0), Align2::LEFT_CENTER, format!("{} design (× φ)", dm.as_ref().map(|m| m.a.name()).unwrap_or("ACI 318")), FontId::proportional(13.0), Color32::BLACK);
        }
        let clip = painter.with_clip_rect(fr.plot.expand(4.0));
        for (k, p) in d.points.iter().enumerate() {
            let c = match p.mech {
                Mechanism::Flexure => Color32::from_rgb(20, 60, 200),
                Mechanism::LongYield => Color32::from_rgb(0, 150, 0),
                Mechanism::LongHardening => Color32::from_rgb(0, 110, 110),
                Mechanism::WebShear => Color32::from_rgb(230, 120, 0),
                Mechanism::WebCrushing => Color32::from_rgb(170, 0, 0),
                Mechanism::Unclear => Color32::from_rgb(130, 130, 130),
            };
            let at = fr.map(p.x, p.y);
            if inside[k] {
                // inside the envelope: its run stopped early
                let g = Color32::from_gray(150);
                clip.line_segment([at + Vec2::new(-3.5, -3.5), at + Vec2::new(3.5, 3.5)], Stroke::new(1.3, g));
                clip.line_segment([at + Vec2::new(-3.5, 3.5), at + Vec2::new(3.5, -3.5)], Stroke::new(1.3, g));
            } else if p.confirmed {
                clip.circle_filled(at, 4.0, c);
            } else {
                clip.circle_filled(at, 4.0, Color32::WHITE);
                clip.circle_stroke(at, 4.0, Stroke::new(1.6, c));
            }
        }
        // values read off the curves under the pointer (exact at computed points, interpolated between)
        let design_name = dm.as_ref().map(|m| format!("{} design", m.a.name())).unwrap_or_default();
        let mut probes = vec![plots::Probe { name: "Nominal capacity", pts: &series[0].pts, color: BLUE }];
        if dm.is_some() {
            probes.push(plots::Probe { name: &design_name, pts: &series[1].pts, color: design_red });
        }
        let (ax, ay) = match d.pair {
            Pair::MN => (("M", "kNm"), ("N", "kN")),
            Pair::NV => (("N", "kN"), ("V", "kN")),
            Pair::MV => (("M", "kNm"), ("V", "kN")),
        };
        let id = match d.pair {
            Pair::MN => "radial_mn",
            Pair::NV => "radial_nv",
            Pair::MV => "radial_mv",
        };
        plots::readout(ui, &painter, &fr, id, &probes, ax, ay, None);
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    fn cases_view(&mut self, ui: &mut egui::Ui) {
        let Some(rows) = self.cases.clone() else { return };
        ui.label(RichText::new("This program against the original's own sectional runs on the sample beam").strong());
        ui.label(
            RichText::new(
                "The original (r2k.exe) was run with each loading below (Solve > Sectional Response); its results are the \
                 \"Current Loads\" at the peak, read off its window (pictures in the shots folder). The same loads are run \
                 here with this program's solver.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let pct = |o: f64, x: f64| if o.abs() > 1e-9 { Some(100.0 * (x - o) / o) } else { None };
        let mut diffs = vec![];
        egui::Grid::new("cases").num_columns(7).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
            for h in ["Loading (constant + increment)", "Original M, V", "This program M, V", "Capacity difference", "Time", "How it ended", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for r in &rows {
                let c = &r.case;
                ui.label(c.name);
                if c.lower_bound {
                    ui.label(format!("{:.1} kNm, {:.1} kN (at least)", c.m, c.v)).on_hover_text(
                        "The original's run stopped while the load was still rising (its last state is its largest), so its value is a \
                         lower bound on its capacity, not a failure. Only a result below it counts as a difference.",
                    );
                } else {
                    ui.label(format!("{:.1} kNm, {:.1} kN", c.m, c.v));
                }
                match r.ours {
                    Some((m, v)) => {
                        ui.label(format!("{:.1} kNm, {:.1} kN", m, v));
                        // what the loading raises is what is compared: the shear, or the moment
                        let d = (if c.inc.2.abs() > 1e-12 { pct(c.v, v) } else { pct(c.m, m) }).map(|_| c.scored_pct(m, v));
                        // the colour follows the difference that counts (for a lower bound: a shortfall)
                        let col = match d.map(|x| x.1.abs()) {
                            Some(x) if x <= 5.0 => Color32::from_rgb(0, 130, 0),
                            Some(x) if x <= 10.0 => Color32::from_rgb(170, 110, 0),
                            Some(_) => Color32::from_rgb(190, 30, 30),
                            None => Color32::GRAY,
                        };
                        if let Some(d) = d {
                            diffs.push(d.1.abs());
                        }
                        let text = d
                            .map(|d| if c.lower_bound && d.0 > 0.0 { format!("{:+.1} % (above the lower bound)", d.0) } else { format!("{:+.1} %", d.0) })
                            .unwrap_or("-".into());
                        ui.colored_label(col, text);
                    }
                    None => {
                        ui.label("-");
                        ui.label("-");
                    }
                }
                ui.label(format!("{:.1} s", r.secs));
                ui.label(RichText::new(&r.reason).small());
                ui.label("");
                ui.end_row();
            }
        });
        if !diffs.is_empty() {
            ui.add_space(6.0);
            ui.label(format!(
                "Mean difference {:.1} %, largest {:.1} % (shear compared where the shear rises, moment where only the moment rises; a lower bound counts only when this program is below it).",
                diffs.iter().sum::<f64>() / diffs.len() as f64,
                diffs.iter().cloned().fold(0.0, f64::max)
            ));
        }
    }

    fn member_view(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.member.clone() else { return };
        ui.label(
            RichText::new(format!(
                "Simply supported span {:.0} mm, point load {:.0} mm from the left support. Flexural failure load {:.1} kN (shear along the member is not checked).",
                r.span, r.a, r.p_max
            ))
            .strong(),
        );
        let mut changed = false;
        if let Some(sec) = &mut self.sec {
            ui.horizontal(|ui| {
                let mem = sec.member.get_or_insert(Member { length: r.span, left_sup: 0.0, right_sup: r.span, load_at: r.a });
                ui.label("Span (mm)");
                let mut span = mem.right_sup - mem.left_sup;
                if ui.add(egui::DragValue::new(&mut span).speed(10.0).range(100.0..=1e6)).changed() {
                    mem.right_sup = mem.left_sup + span;
                    changed = true;
                }
                ui.label("Load distance from left support (mm)");
                let mut a = mem.load_at - mem.left_sup;
                if ui.add(egui::DragValue::new(&mut a).speed(10.0).range(1.0..=1e6)).changed() {
                    mem.load_at = mem.left_sup + a;
                    changed = true;
                }
            });
        }
        if changed {
            if let (Some(sec), Some((_, f))) = (&self.sec, &self.flex) {
                self.member = Some(member_response(sec.member.as_ref().unwrap(), f));
            }
        }
        let Some(r) = self.member.clone() else { return };
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        let pts: Vec<[f64; 2]> = r.points.iter().map(|p| [p.1, p.0]).collect();
        plots::draw(&painter, rect.shrink(10.0), "Load - Deflection", "Deflection under the load (mm)", "Load (kN)", &[Series::line(pts, BLUE)], None, None);
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    // ------------------------------------------------------------------ dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(e) = self.error.clone() {
            let mut open = true;
            let mut ok = false;
            egui::Window::new("Section Response").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.label(e);
                ok = ui.button("OK").clicked();
            });
            if !open || ok {
                self.error = None;
            }
        }
        if let Some(mut f) = self.factors.clone() {
            // Options > Design Factors: nominal; CSA A23.3 material factors (the original's
            // Options > General "Material Factors", its CSA and None buttons, 0.01..10 as it clamps
            // them); or ACI 318 strength reduction factors on the nominal response (design.rs)
            let mut open = true;
            let mut apply = false;
            let mut cancel = false;
            egui::Window::new("Design Factors").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut f.code, 0, "Nominal (no factors)");
                    ui.selectable_value(&mut f.code, 1, "CSA A23.3 (material factors)");
                    // ACI 318 and AASHTO LRFD share the strength-reduction set (AciFactors.code)
                    if ui.selectable_label(f.code == 2, "ACI 318 (strength reduction φ)").clicked() && f.code != 2 {
                        f.code = 2;
                        if f.aci.code != 0 {
                            f.aci = AciFactors { spiral: f.aci.spiral, ..AciFactors::default() };
                        }
                    }
                    if ui.selectable_label(f.code == 3, "AASHTO LRFD (resistance φ)").clicked() && f.code != 3 {
                        f.code = 3;
                        if f.aci.code != 1 {
                            f.aci = AciFactors { spiral: f.aci.spiral, ..AciFactors::aashto() };
                        }
                    }
                });
                ui.separator();
                match f.code {
                    0 => {
                        ui.label("The analysis uses the materials as defined: nominal (unfactored) resistances.");
                    }
                    1 => {
                        ui.label("Every stress of a material is multiplied by its factor and the steel is held at its");
                        ui.label("yield stress (no strain hardening), as the original program does; the curves then");
                        ui.label("are factored resistances directly (CSA A23.3: φc 0.65, φs 0.85, φp 0.90).");
                        ui.add_space(4.0);
                        egui::Grid::new("phi").num_columns(2).show(ui, |ui| {
                            for (i, name) in ["Concrete  φc", "Reinforcing steel  φs", "Prestressing steel  φp"].iter().enumerate() {
                                ui.label(*name);
                                ui.add(egui::DragValue::new(&mut f.phi[i]).speed(0.01).range(0.01..=10.0).fixed_decimals(2));
                                ui.end_row();
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("CSA").on_hover_text("CSA A23.3: 0.65, 0.85, 0.90").clicked() {
                                f.phi = PHI_CSA;
                            }
                            if ui.button("None").on_hover_text("Unfactored: 1.00, 1.00, 1.00").clicked() {
                                f.phi = [1.0; 3];
                            }
                        });
                    }
                    _ => {
                        let aashto = f.code == 3;
                        let name = if aashto { "AASHTO LRFD" } else { "ACI 318" };
                        ui.label("The analysis stays nominal (the full sectional response with unfactored materials);");
                        ui.label(format!("each nominal point is then reduced by its {} factor and drawn as a design curve:", name));
                        ui.label("• moment and axial load: φ from the net tensile strain εt of the extreme tension steel");
                        ui.label("   when the extreme compression fibre reaches εcu (plane sections, at that axial load);");
                        ui.label("• shear: φv;   • torsion: φt (load-case checks);   • axial compression capped at φ Pn,max = φ α P0.");
                        ui.add_space(4.0);
                        let a = &mut f.aci;
                        egui::Grid::new("aci").num_columns(2).show(ui, |ui| {
                            ui.label("Transverse reinforcement");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut a.spiral, false, "ties / stirrups");
                                ui.radio_value(&mut a.spiral, true, "spiral");
                            });
                            ui.end_row();
                            let mut row = |ui: &mut egui::Ui, name: &str, v: &mut f64| {
                                ui.label(name);
                                ui.add(egui::DragValue::new(v).speed(0.01).range(0.01..=1.0).fixed_decimals(2));
                                ui.end_row();
                            };
                            row(ui, "Tension-controlled, reinforced  φ", &mut a.phi_tension);
                            if aashto {
                                row(ui, "Tension-controlled, prestressed  φ", &mut a.phi_tension_ps);
                            }
                            row(ui, "Compression-controlled, ties  φ", &mut a.phi_tied);
                            row(ui, "Compression-controlled, spiral  φ", &mut a.phi_spiral);
                            row(ui, "Shear  φv", &mut a.phi_shear);
                            row(ui, "Torsion  φt", &mut a.phi_torsion);
                            row(ui, "Pn,max / P0, ties  α", &mut a.pmax_tied);
                            row(ui, "Pn,max / P0, spiral  α", &mut a.pmax_spiral);
                            ui.label("Concrete strain at nominal strength  εcu (mm/m)");
                            let mut e = a.ecu * 1000.0;
                            if ui.add(egui::DragValue::new(&mut e).speed(0.05).range(1.0..=10.0).fixed_decimals(2)).changed() {
                                a.ecu = e / 1000.0;
                            }
                            ui.end_row();
                        });
                        if aashto {
                            if ui
                                .button("AASHTO LRFD values")
                                .on_hover_text("9th ed. 5.5.4.2: 0.90 reinforced / 1.00 prestressed, 0.75 compression, φv 0.90, φt 0.90, α 0.80 / 0.85, εcu 3.0 mm/m")
                                .clicked()
                            {
                                let sp = f.aci.spiral;
                                f.aci = AciFactors { spiral: sp, ..AciFactors::aashto() };
                            }
                            ui.label(
                                RichText::new("Compression-controlled up to εcl = fy/Es (2 mm/m for strands); tension-controlled from εtl = 5 mm/m (rising to 8 mm/m for 690 MPa bars), linear between. Normal-weight concrete.")
                                    .small()
                                    .weak(),
                            );
                        } else {
                            if ui.button("ACI 318-19 values").on_hover_text("0.90, 0.65, 0.75, φv 0.75, φt 0.75, α 0.80 / 0.85, εcu 3.0 mm/m").clicked() {
                                let sp = f.aci.spiral;
                                f.aci = AciFactors { spiral: sp, ..AciFactors::default() };
                            }
                            ui.label(
                                RichText::new("Tension-controlled from εt = εty + 3 mm/m, compression-controlled up to εty = fy/Es (2 mm/m for strands), linear between.")
                                    .small()
                                    .weak(),
                            );
                        }
                    }
                }                ui.separator();
                ui.horizontal(|ui| {
                    apply = ui.button("OK").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            self.factors = if !open || cancel { None } else { Some(f.clone()) };
            if apply {
                self.factors = None;
                if let Some(sec) = &mut self.sec {
                    // only one approach at a time: ACI keeps the materials nominal
                    let (phi, aci) = match f.code {
                        0 => ([1.0; 3], None),
                        1 => (f.phi, None),
                        _ => ([1.0; 3], Some(f.aci.clone())),
                    };
                    let phi_changed = sec.phi.iter().zip(&phi).any(|(a, b)| (a - b).abs() > 1e-9);
                    let aci_changed = sec.aci != aci;
                    sec.phi = phi;
                    sec.aci = aci;
                    if phi_changed {
                        // the materials changed: the results must be computed again
                        self.changed();
                    } else if aci_changed {
                        // the nominal results stay valid; only the design curves change
                        self.dirty = true;
                        self.aci_models = None;
                    }
                }
            }
        }
        if self.about_open {
            let mut open = true;
            egui::Window::new("About").collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.heading("Section Response");
                ui.label("Reinforced concrete sectional analysis with the Modified Compression Field Theory.");
                ui.label("An independent rebuild; its material rules follow the original program (see MECHANICS.md).");
                ui.label("Units: mm, MPa, kN, kNm.");
            });
            self.about_open = open;
        }
        if self.pick_open {
            let mut open = true;
            let mut chosen = None;
            egui::Window::new("Choose a section").collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label("This file holds several cross-sections:");
                for (i, s) in self.sections.iter().enumerate() {
                    if ui.button(&s.title).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            if let Some(i) = chosen {
                self.sec = Some(self.sections[i].clone());
                self.clear_results();
                open = false;
            }
            self.pick_open = open;
        }
        if self.quick_open {
            self.quick_dialog(ctx);
        }
        if self.sec.is_none() {
            return;
        }
        let mut changed = false;
        if self.mat_open {
            let mut open = true;
            let sec = self.sec.as_mut().unwrap();
            egui::Window::new("Materials").open(&mut open).default_width(560.0).show(ctx, |ui| {
                if let Some(c) = sec.concretes.first_mut() {
                    ui.heading("Concrete");
                    let p = crate::materials::ConcreteProps::new(c);
                    egui::Grid::new("conc").num_columns(2).show(ui, |ui| {
                        ui.label("Name");
                        changed |= ui.text_edit_singleline(&mut c.name).changed();
                        ui.end_row();
                        ui.label("f'c (MPa)");
                        changed |= ui.add(egui::DragValue::new(&mut c.fc).speed(0.5).range(5.0..=200.0)).changed();
                        ui.end_row();
                        ui.label("Cracking strength ft (MPa)");
                        ui.horizontal(|ui| {
                            let mut auto = c.ft.is_none();
                            if ui.checkbox(&mut auto, "automatic").changed() {
                                c.ft = if auto { None } else { Some(p.ft) };
                                changed = true;
                            }
                            if let Some(ft) = &mut c.ft {
                                changed |= ui.add(egui::DragValue::new(ft).speed(0.05).range(0.0..=20.0)).changed();
                            } else {
                                ui.label(format!("{:.2}", p.ft));
                            }
                        });
                        ui.end_row();
                        ui.label("Peak strain e'c (mm/m)");
                        ui.horizontal(|ui| {
                            let mut auto = c.e0.is_none();
                            if ui.checkbox(&mut auto, "automatic").changed() {
                                c.e0 = if auto { None } else { Some(p.e0 * 1000.0) };
                                changed = true;
                            }
                            if let Some(e0) = &mut c.e0 {
                                changed |= ui.add(egui::DragValue::new(e0).speed(0.01).range(0.5..=10.0)).changed();
                            } else {
                                ui.label(format!("{:.3}", p.e0 * 1000.0));
                            }
                        });
                        ui.end_row();
                        ui.label("Max aggregate size (mm)");
                        changed |= ui.add(egui::DragValue::new(&mut c.agg).speed(1.0).range(0.0..=100.0)).changed();
                        ui.end_row();
                        ui.label("Tension stiffening factor");
                        changed |= ui.add(egui::DragValue::new(&mut c.tsfactor).speed(0.05).range(0.0..=1.0)).changed();
                        ui.end_row();
                        ui.label("Tension stiffening");
                        changed |= ui.checkbox(&mut c.tension_stiffening, "on (Bentz 1999)").changed();
                        ui.end_row();
                        ui.label("Design limit ecu (mm/m)").on_hover_text(
                            "The designer's limit on the concrete compressive strain (3.5 by default). Where it is reached is \
                             marked on the M-V and moment-curvature plots; the analysis itself carries on past it.",
                        );
                        changed |= ui.add(egui::DragValue::new(&mut c.ecu).speed(0.05).range(1.0..=10.0)).changed();
                        ui.end_row();
                        ui.label("Base curve");
                        egui::ComboBox::from_id_salt("base")
                            .selected_text(match c.base {
                                BaseCurve::Popovics => "Popovics/Thorenfeldt/Collins",
                                BaseCurve::Parabolic => "Parabolic",
                            })
                            .show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut c.base, BaseCurve::Popovics, "Popovics/Thorenfeldt/Collins").changed();
                                changed |= ui.selectable_value(&mut c.base, BaseCurve::Parabolic, "Parabolic").changed();
                            });
                        ui.end_row();
                        ui.label("Compression softening");
                        egui::ComboBox::from_id_salt("soft")
                            .selected_text(match c.softening {
                                Softening::Default2011 => "Default (program model)",
                                Softening::VecchioCollins1986 => "Vecchio-Collins 1986",
                                Softening::None => "None",
                            })
                            .show_ui(ui, |ui| {
                                changed |= ui.selectable_value(&mut c.softening, Softening::Default2011, "Default (program model)").changed();
                                changed |= ui.selectable_value(&mut c.softening, Softening::VecchioCollins1986, "Vecchio-Collins 1986").changed();
                                changed |= ui.selectable_value(&mut c.softening, Softening::None, "None").changed();
                            });
                        ui.end_row();
                    });
                    ui.separator();
                }
                ui.heading("Reinforcing steel");
                let mut remove = None;
                egui::Grid::new("rebars").striped(true).show(ui, |ui| {
                    for h in ["Name", "fy", "fu", "Es", "esh (mm/m)", "eu (mm/m)", "design limit es (mm/m)", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, r) in sec.rebars.iter_mut().enumerate() {
                        changed |= ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(80.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut r.fy).range(100.0..=2000.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut r.fu).range(100.0..=2500.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut r.es).speed(100.0).range(50000.0..=300000.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut r.esh).speed(0.5).range(0.0..=200.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut r.eu).speed(0.5).range(1.0..=300.0)).changed();
                        // the designer's strain limit: by default the start of strain hardening
                        ui.horizontal(|ui| {
                            let mut own = r.e_lim.is_some();
                            if ui.checkbox(&mut own, "").on_hover_text("Unticked: the start of strain hardening (esh)").changed() {
                                r.e_lim = if own { Some(r.esh) } else { None };
                                changed = true;
                            }
                            match &mut r.e_lim {
                                Some(v) => changed |= ui.add(egui::DragValue::new(v).speed(0.5).range(0.5..=300.0)).changed(),
                                None => {
                                    ui.label(format!("= esh {:.1}", r.esh));
                                }
                            }
                        });
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    if sec.rebars.len() > 1 {
                        sec.rebars.remove(i);
                        changed = true;
                    }
                }
                if ui.button("Add steel type").clicked() {
                    let n = sec.rebars.len() + 1;
                    sec.rebars.push(Rebar::new(&format!("Steel {}", n), 400.0));
                    changed = true;
                }
                ui.separator();
                ui.heading("Prestressing steel");
                egui::Grid::new("ps").striped(true).show(ui, |ui| {
                    for h in ["Name", "fpu", "Ep", "A", "B", "C", "eu (mm/m)"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for p in sec.prestress.iter_mut() {
                        changed |= ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(80.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.fpu).range(500.0..=3000.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.ep).speed(100.0).range(50000.0..=300000.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.a).speed(0.001).range(0.0..=1.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.b).range(1.0..=1000.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.c).range(1.0..=100.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut p.eu).range(1.0..=200.0)).changed();
                        ui.end_row();
                    }
                });
                if ui.button("Add prestressing type").clicked() {
                    let n = sec.prestress.len() + 1;
                    sec.prestress.push(Prestress::new(&format!("P-Steel {}", n), 1860.0));
                    changed = true;
                }
            });
            self.mat_open = open;
        }
        if self.shape_open {
            let mut open = true;
            let sec = self.sec.as_mut().unwrap();
            egui::Window::new("Concrete Section").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Title");
                    changed |= ui.text_edit_singleline(&mut sec.title).changed();
                });
                let kinds = ["Rectangle", "T-beam", "I-beam", "Circle", "Custom outline"];
                let mut k = match sec.shape {
                    Shape::Rect { .. } => 0,
                    Shape::Tee { .. } => 1,
                    Shape::IBeam { .. } => 2,
                    Shape::Circle { .. } => 3,
                    Shape::Custom => 4,
                };
                let h = sec.height().max(100.0);
                let b = sec.max_width().max(100.0);
                egui::ComboBox::from_label("Shape").selected_text(kinds[k]).show_ui(ui, |ui| {
                    for (i, n) in kinds.iter().enumerate() {
                        if ui.selectable_value(&mut k, i, *n).changed() {
                            sec.shape = match i {
                                0 => Shape::Rect { b, h },
                                1 => Shape::Tee { bf: b * 3.0, hf: h * 0.2, bw: b, h },
                                2 => Shape::IBeam { bt: b * 2.0, ht: h * 0.15, bw: b * 0.5, bb: b * 1.5, hb: h * 0.15, h },
                                3 => Shape::Circle { d: h },
                                _ => Shape::Custom,
                            };
                            if !matches!(sec.shape, Shape::Custom) {
                                sec.outline = sec.shape.outline();
                            }
                            changed = true;
                        }
                    }
                });
                let dv = |ui: &mut egui::Ui, l: &str, v: &mut f64| -> bool {
                    ui.label(l);
                    let c = ui.add(egui::DragValue::new(v).speed(5.0).range(10.0..=20000.0)).changed();
                    ui.end_row();
                    c
                };
                let mut sc = false;
                egui::Grid::new("shape").num_columns(2).show(ui, |ui| match &mut sec.shape {
                    Shape::Rect { b, h } => {
                        sc |= dv(ui, "Width b (mm)", b);
                        sc |= dv(ui, "Height h (mm)", h);
                    }
                    Shape::Tee { bf, hf, bw, h } => {
                        sc |= dv(ui, "Flange width (mm)", bf);
                        sc |= dv(ui, "Flange thickness (mm)", hf);
                        sc |= dv(ui, "Web width (mm)", bw);
                        sc |= dv(ui, "Height (mm)", h);
                    }
                    Shape::IBeam { bt, ht, bw, bb, hb, h } => {
                        sc |= dv(ui, "Top flange width (mm)", bt);
                        sc |= dv(ui, "Top flange thickness (mm)", ht);
                        sc |= dv(ui, "Web width (mm)", bw);
                        sc |= dv(ui, "Bottom flange width (mm)", bb);
                        sc |= dv(ui, "Bottom flange thickness (mm)", hb);
                        sc |= dv(ui, "Height (mm)", h);
                    }
                    Shape::Circle { d } => {
                        sc |= dv(ui, "Diameter (mm)", d);
                    }
                    Shape::Custom => {}
                });
                if sc {
                    sec.outline = sec.shape.outline();
                    changed = true;
                }
                if matches!(sec.shape, Shape::Custom) {
                    ui.label("Outline points from bottom to top (height z, width b):");
                    let mut remove = None;
                    egui::Grid::new("outline").striped(true).show(ui, |ui| {
                        for (i, p) in sec.outline.iter_mut().enumerate() {
                            changed |= ui.add(egui::DragValue::new(&mut p.0).speed(5.0).range(0.0..=20000.0)).changed();
                            changed |= ui.add(egui::DragValue::new(&mut p.1).speed(5.0).range(0.0..=20000.0)).changed();
                            if ui.small_button("Remove").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = remove {
                        sec.outline.remove(i);
                        changed = true;
                    }
                    if ui.button("Add point at top").clicked() {
                        let last = sec.outline.last().copied().unwrap_or((0.0, 300.0));
                        sec.outline.push((last.0 + 100.0, last.1));
                        changed = true;
                    }
                }
                ui.label(format!("Height {:.0} mm, gross area {:.0} mm²", sec.height(), sec.gross_area()));
            });
            self.shape_open = open;
        }
        if self.long_open {
            let mut open = true;
            let sec = self.sec.as_mut().unwrap();
            let steels: Vec<String> = sec
                .rebars
                .iter()
                .map(|r| r.name.clone())
                .chain(sec.prestress.iter().map(|p| p.name.clone()))
                .collect();
            let first_steel = sec.rebars.first().map(|r| r.name.clone()).unwrap_or_default();
            let h = sec.height();
            egui::Window::new("Longitudinal Reinforcement").open(&mut open).default_width(700.0).show(ctx, |ui| {
                ui.label("Layers of bars or tendons. Give each layer's distance from the bottom or from the top face, to the bars' centres.");
                let mut remove = None;
                let mut copy = None;
                egui::Grid::new("long").striped(true).show(ui, |ui| {
                    for t in ["Name", "Measured from", "Distance (mm)", "Cover to top/bottom", "Bars", "Bar size", "Area (mm²)", "Steel", "Prestrain (mm/m)", ""] {
                        ui.label(RichText::new(t).strong());
                    }
                    ui.end_row();
                    for (i, l) in sec.long.iter_mut().enumerate() {
                        changed |= ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(70.0)).changed();
                        // the layer's position: from the bottom (z) or from the top (h - z); z is
                        // what the analysis and the file keep
                        egui::ComboBox::from_id_salt(("from", i)).width(70.0).selected_text(if l.from_top { "Top" } else { "Bottom" }).show_ui(ui, |ui| {
                            if ui.selectable_label(!l.from_top, "Bottom").clicked() {
                                l.from_top = false;
                                changed = true;
                            }
                            if ui.selectable_label(l.from_top, "Top").clicked() {
                                l.from_top = true;
                                changed = true;
                            }
                        });
                        let mut dist = if l.from_top { h - l.z } else { l.z };
                        if ui.add(egui::DragValue::new(&mut dist).speed(1.0).range(0.0..=h.max(1.0)).max_decimals(2)).changed() {
                            l.z = if l.from_top { h - dist } else { dist };
                            changed = true;
                        }
                        // clear cover from the nearer face to the bar's surface
                        let cc = l.z.min(h - l.z) - l.draw_dia() / 2.0;
                        ui.label(if cc >= 0.0 { format!("{:.1} mm", cc) } else { "outside!".into() });
                        if ui.add(egui::DragValue::new(&mut l.num).range(1..=100)).changed() {
                            if let Some(a) = bar_area_csa(&l.bartitle) {
                                l.area = a * l.num as f64;
                            }
                            changed = true;
                        }
                        egui::ComboBox::from_id_salt(("bt", i)).width(60.0).selected_text(l.bartitle.clone()).show_ui(ui, |ui| {
                            for t in BAR_TITLES {
                                if ui.selectable_label(l.bartitle == t, t).clicked() {
                                    l.bartitle = t.into();
                                    l.area = bar_area_csa(t).unwrap_or(100.0) * l.num as f64;
                                    changed = true;
                                }
                            }
                        });
                        changed |= ui.add(egui::DragValue::new(&mut l.area).speed(10.0).range(1.0..=1e6)).changed();
                        egui::ComboBox::from_id_salt(("st", i)).width(90.0).selected_text(l.steel.clone()).show_ui(ui, |ui| {
                            for s in &steels {
                                if ui.selectable_label(&l.steel == s, s.as_str()).clicked() {
                                    l.steel = s.clone();
                                    changed = true;
                                }
                            }
                        });
                        changed |= ui.add(egui::DragValue::new(&mut l.dep).speed(0.1).range(-50.0..=50.0)).changed();
                        ui.horizontal(|ui| {
                            if ui.small_button("Copy").on_hover_text("Copy this layer").clicked() {
                                copy = Some(i);
                            }
                            if ui.small_button("Remove").on_hover_text("Remove this layer").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    sec.long.remove(i);
                    changed = true;
                }
                if let Some(i) = copy {
                    let mut l = sec.long[i].clone();
                    l.name = format!("{} copy", l.name);
                    l.ring = 0;
                    l.ring_id = 0;
                    sec.long.insert(i + 1, l);
                    changed = true;
                }
                ui.horizontal(|ui| {
                    let n = sec.long.len() + 1;
                    let mut add = |from_top: bool, dist: f64| {
                        sec.long.push(LongLayer {
                            name: format!("layer {}", n),
                            z: if from_top { h - dist } else { dist },
                            num: 2,
                            area: 400.0,
                            steel: first_steel.clone(),
                            bartitle: "15M".into(),
                            dep: 0.0,
                            ring: 0,
                            ring_id: 0,
                            from_top,
                        });
                    };
                    if ui.button("Add layer from the bottom").clicked() {
                        add(false, 60.0_f64.min(h * 0.5));
                        changed = true;
                    }
                    if ui.button("Add layer from the top").clicked() {
                        add(true, 60.0_f64.min(h * 0.5));
                        changed = true;
                    }
                });
            });
            self.long_open = open;
        }
        if self.trans_open {
            let mut open = true;
            let sec = self.sec.as_mut().unwrap();
            let steels: Vec<String> = sec.rebars.iter().map(|r| r.name.clone()).collect();
            let h = sec.height();
            egui::Window::new("Transverse Reinforcement").open(&mut open).show(ctx, |ui| {
                let mut has = sec.stirrups.is_some();
                if ui.checkbox(&mut has, "Section has stirrups").changed() {
                    sec.stirrups = if has {
                        Some(Stirrups {
                            name: "stirrups".into(),
                            area: 200.0,
                            steel: steels.first().cloned().unwrap_or_default(),
                            spacing: 200.0,
                            // 40 mm clear cover to a 10M stirrup, as Quick Define places it
                            z_top: h - 40.0 - 11.3 / 2.0,
                            z_bot: 40.0 + 11.3 / 2.0,
                            db: 11.3,
                            bartitle: "10M".into(),
                            pattern: 3,
                        })
                    } else {
                        None
                    };
                    changed = true;
                }
                let bars: Vec<(String, f64, f64)> = sec.long.iter().map(|l| (if l.bartitle.is_empty() { l.name.clone() } else { l.bartitle.clone() }, l.z, l.draw_dia())).collect();
                if let Some(st) = &mut sec.stirrups {
                    egui::Grid::new("tr").num_columns(2).show(ui, |ui| {
                        // bar size: sets the diameter and the area of all legs together, keeping
                        // the clear covers (the stirrup's centreline moves with the new diameter)
                        ui.label("Bar size");
                        let cur = if bar_db_csa(&st.bartitle).is_some() { st.bartitle.to_uppercase() } else { "Other".to_string() };
                        let mut pick: Option<&str> = None;
                        egui::ComboBox::from_id_salt("tsize").selected_text(cur.clone()).show_ui(ui, |ui| {
                            for t in BAR_TITLES {
                                if ui.selectable_label(cur == t, t).clicked() {
                                    pick = Some(t);
                                }
                            }
                        });
                        if let Some(t) = pick {
                            let (ct, cb) = (h - st.z_top - st.db / 2.0, st.z_bot - st.db / 2.0);
                            st.db = bar_db_csa(t).unwrap_or(st.db);
                            st.area = bar_area_csa(t).unwrap_or(st.area / stirrup_legs(st.pattern)) * stirrup_legs(st.pattern);
                            st.bartitle = t.to_string();
                            st.z_top = h - ct - st.db / 2.0;
                            st.z_bot = cb + st.db / 2.0;
                            changed = true;
                        }
                        ui.end_row();
                        // clear covers to the outside of the stirrup or hoop: the stirrup's
                        // centreline sits half a bar diameter further in (cover 40 with a 10M:
                        // 45.65 from the bottom, as the original's Quick Define writes it)
                        let mut ct = h - st.z_top - st.db / 2.0;
                        let mut cb = st.z_bot - st.db / 2.0;
                        ui.label("Clear cover to stirrup, top (mm)");
                        if ui.add(egui::DragValue::new(&mut ct).speed(1.0).range(0.0..=h).max_decimals(1)).changed() {
                            st.z_top = h - ct - st.db / 2.0;
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Clear cover to stirrup, bottom and sides (mm)");
                        if ui.add(egui::DragValue::new(&mut cb).speed(1.0).range(0.0..=h).max_decimals(1)).changed() {
                            st.z_bot = cb + st.db / 2.0;
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Area of one stirrup, all legs (mm²)");
                        changed |= ui.add(egui::DragValue::new(&mut st.area).speed(5.0).range(1.0..=10000.0)).changed();
                        ui.end_row();
                        ui.label("Spacing (mm)");
                        changed |= ui.add(egui::DragValue::new(&mut st.spacing).speed(5.0).range(10.0..=5000.0)).changed();
                        ui.end_row();
                        ui.label("Top of stirrup, height (mm)");
                        changed |= ui.add(egui::DragValue::new(&mut st.z_top).speed(1.0).range(0.0..=20000.0)).changed();
                        ui.end_row();
                        ui.label("Bottom of stirrup, height (mm)");
                        changed |= ui.add(egui::DragValue::new(&mut st.z_bot).speed(1.0).range(0.0..=20000.0)).changed();
                        ui.end_row();
                        ui.label("Bar diameter (mm)");
                        let (ct, cb) = (h - st.z_top - st.db / 2.0, st.z_bot - st.db / 2.0);
                        if ui.add(egui::DragValue::new(&mut st.db).speed(0.5).range(1.0..=60.0)).changed() {
                            st.z_top = h - ct - st.db / 2.0;
                            st.z_bot = cb + st.db / 2.0;
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("Steel");
                        egui::ComboBox::from_id_salt("tst").selected_text(st.steel.clone()).show_ui(ui, |ui| {
                            for s in &steels {
                                if ui.selectable_label(&st.steel == s, s.as_str()).clicked() {
                                    st.steel = s.clone();
                                    changed = true;
                                }
                            }
                        });
                        ui.end_row();
                    });
                    // check the detailing: the stirrup inside the concrete, every bar row inside
                    // the stirrup (bar face no closer than the stirrup's inside face)
                    let mut notes: Vec<String> = vec![];
                    let (ct, cb) = (h - st.z_top - st.db / 2.0, st.z_bot - st.db / 2.0);
                    if ct < 0.0 || cb < 0.0 {
                        notes.push("The stirrup sticks out of the concrete: the cover must be positive.".into());
                    }
                    if st.z_top <= st.z_bot {
                        notes.push("The covers leave no room for the stirrup.".into());
                    }
                    let (inner_top, inner_bot) = (st.z_top - st.db / 2.0, st.z_bot + st.db / 2.0);
                    for (name, z, d) in &bars {
                        if z + d / 2.0 > inner_top + 0.5 {
                            notes.push(format!("Bar {} at {:.1} mm is above the stirrup's inside face ({:.1} mm): it should sit at {:.1} mm or lower.", name, z, inner_top, inner_top - d / 2.0));
                        } else if z - d / 2.0 < inner_bot - 0.5 {
                            notes.push(format!("Bar {} at {:.1} mm is below the stirrup's inside face ({:.1} mm): it should sit at {:.1} mm or higher.", name, z, inner_bot, inner_bot + d / 2.0));
                        }
                    }
                    ui.separator();
                    ui.label(format!("Stirrup centreline from {:.2} to {:.2} mm; inside faces {:.2} to {:.2} mm.", st.z_bot, st.z_top, inner_bot, inner_top));
                    if notes.is_empty() {
                        ui.colored_label(Color32::from_rgb(0, 120, 0), "Cover and bars check out: every bar is inside the stirrup.");
                    } else {
                        for n in notes {
                            ui.colored_label(Color32::from_rgb(170, 60, 0), n);
                        }
                    }
                }
            });
            self.trans_open = open;
        }
        if self.loads_open {
            let mut open = true;
            let sec = self.sec.as_mut().unwrap();
            egui::Window::new("Define Loading").open(&mut open).show(ctx, |ui| {
                // laid out like the original's Define Loading dialog: constant + increment
                let l = &mut sec.loads;
                let mut ch = false;
                egui::Grid::new("loads").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new("Constant").strong());
                    ui.label(RichText::new("+ Increment").strong());
                    ui.label("");
                    ui.end_row();
                    ui.label("Axial Load");
                    ch |= ui.add(egui::DragValue::new(&mut l.axial).speed(5.0)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut l.n_inc).speed(0.1)).changed();
                    ui.label("kN");
                    ui.end_row();
                    ui.label("Moment");
                    ch |= ui.add(egui::DragValue::new(&mut l.m_const).speed(5.0)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut l.m_inc).speed(0.05)).changed();
                    ui.label("kNm");
                    ui.end_row();
                    ui.label("Shear");
                    ch |= ui.add(egui::DragValue::new(&mut l.v_const).speed(5.0)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut l.v_inc).speed(0.05)).changed();
                    ui.label("kN");
                    ui.end_row();
                });
                if ch {
                    l.sync();
                    changed = true;
                }
                ui.label("For a \"One Load\" analysis, only use the left side.");
                ui.label(
                    RichText::new(
                        "Loads = constant + load factor x increment. The constant loads are applied first and held, then the \
                         load factor is raised until the section fails (Solve > Shear). Axial tension is positive; a positive \
                         moment puts the bottom in tension. Examples: M/V = 1.5 m is moment increment 1.5 with shear increment 1; \
                         the largest shear at a fixed 100 kNm is constant moment 100 with shear increment 1.",
                    )
                    .weak(),
                );
                if l.m_const.abs() > 1e-9 && l.m_inc.abs() > 1e-12 && (l.m_const < 0.0) != (l.m_inc < 0.0) {
                    ui.colored_label(
                        Color32::from_rgb(180, 110, 0),
                        "The constant moment and the moment increment act in opposite directions: the moment passes through zero.",
                    );
                }
            });
            self.loads_open = open;
        }
        if changed {
            self.changed();
        }
    }

    fn quick_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut finish = false;
        let qf = &mut self.quick;
        let title = format!("Quick Define - Step {} of 4", qf.step + 1);
        let num = |ui: &mut egui::Ui, v: &mut f64, unit: &str, enabled: bool| {
            ui.horizontal(|ui| {
                ui.add_enabled(enabled, egui::DragValue::new(v).speed(1.0).range(0.0..=1.0e6).min_decimals(0));
                if !unit.is_empty() {
                    ui.label(unit);
                }
            });
        };
        egui::Window::new(title)
            .id(egui::Id::new("quick_define"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.set_min_width(440.0);
                ui.set_min_height(250.0);
                let q = &mut qf.q;
                match qf.step {
                    0 => {
                        ui.group(|ui| {
                            ui.set_min_width(420.0);
                            ui.label("General Information");
                            egui::Grid::new("qd1a").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                                ui.label("Title");
                                ui.add(egui::TextEdit::singleline(&mut q.title).desired_width(290.0));
                                ui.end_row();
                                ui.label("Analysis By:");
                                ui.add(egui::TextEdit::singleline(&mut q.done_by).desired_width(72.0));
                                ui.end_row();
                            });
                        });
                        ui.add_space(6.0);
                        ui.group(|ui| {
                            ui.set_min_width(420.0);
                            ui.label("Material Properties");
                            egui::Grid::new("qd1b").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                                ui.label("Concrete Cylinder Strength");
                                num(ui, &mut q.fc, "MPa", true);
                                ui.end_row();
                                ui.label("Long. Steel Yield Strength");
                                num(ui, &mut q.fy, "MPa", true);
                                ui.end_row();
                                ui.label("Transverse Steel Yield");
                                num(ui, &mut q.fyt, "MPa", true);
                                ui.end_row();
                                ui.label("Prestressed Steel Type");
                                egui::ComboBox::from_id_salt("qdps").width(222.0).selected_text(STRAND_TYPES[q.strand_type]).show_ui(ui, |ui| {
                                    for (i, t) in STRAND_TYPES.iter().enumerate() {
                                        ui.selectable_value(&mut q.strand_type, i, *t);
                                    }
                                });
                                ui.end_row();
                            });
                        });
                    }
                    1 => {
                        let names = ["Rectangle Section", "Circular Section", "T-Beam Section", "I-Beam Section", "Column Section"];
                        let hints: [&[&str]; 5] = [
                            &["Width", "Height"],
                            &["Diameter"],
                            &["Top flange width", "Height", "Web width", "Top flange thickness"],
                            &["Top flange width", "Height", "Web width", "Top flange thickness", "Bottom flange thickness", "Bottom flange width"],
                            &["Width", "Depth (in the direction of bending)"],
                        ];
                        let mut status = names[q.shape].to_string();
                        ui.horizontal(|ui| {
                            ui.group(|ui| {
                                ui.set_min_size(Vec2::new(230.0, 200.0));
                                ui.vertical(|ui| {
                                    ui.label("Title");
                                    for (i, n) in names.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(Vec2::new(30.0, 30.0), egui::Sense::hover());
                                            shape_icon(ui.painter(), r, i);
                                            if ui.selectable_label(q.shape == i, *n).clicked() {
                                                // a column starts square, 400 x 400
                                                if i == 4 && q.shape != 4 {
                                                    q.dims[0] = 400.0;
                                                    q.dims[1] = 400.0;
                                                }
                                                q.shape = i;
                                            }
                                        });
                                    }
                                });
                            });
                            ui.vertical(|ui| {
                                let labels = QuickDefine::dim_labels(q.shape);
                                egui::Grid::new("qd2").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                                    for k in 0..6 {
                                        let on = k < labels.len();
                                        ui.label(if on { labels[k] } else { "" });
                                        let r = ui.add_enabled(on, egui::DragValue::new(&mut q.dims[k]).speed(5.0).range(0.0..=100000.0));
                                        if on && (r.has_focus() || r.hovered()) {
                                            status = hints[q.shape][k].to_string();
                                        }
                                        ui.label(if on { "mm" } else { "" });
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                        ui.separator();
                        ui.label(status);
                    }
                    2 => {
                        let circle = q.shape == 1;
                        let groups = if circle {
                            ["Outer Non-Prestressed Reinforcement Ring", "Inner Non-Prestressed Reinforcement Ring"]
                        } else if q.shape == 4 {
                            ["Top Face Reinforcement (corner bars included)", "Bottom Face Reinforcement (corner bars included)"]
                        } else {
                            ["Top Non-Prestressed Reinforcement", "Bottom Non-Prestressed Reinforcement"]
                        };
                        for (gi, g) in groups.iter().enumerate() {
                            let (n, bar, by_area, area) = if gi == 0 {
                                (&mut q.top_n, &mut q.top_bar, &mut q.top_by_area, &mut q.top_area)
                            } else {
                                (&mut q.bot_n, &mut q.bot_bar, &mut q.bot_by_area, &mut q.bot_area)
                            };
                            ui.group(|ui| {
                                ui.set_min_width(420.0);
                                ui.label(*g);
                                egui::Grid::new(("qd3", gi)).num_columns(3).spacing([16.0, 6.0]).show(ui, |ui| {
                                    ui.label("Number of Bars");
                                    ui.add(egui::DragValue::new(n).range(0..=500));
                                    ui.label("(e.g. 4)");
                                    ui.end_row();
                                    ui.checkbox(by_area, "Select bar by area");
                                    ui.end_row();
                                    if *by_area {
                                        ui.label("Bar Area");
                                        ui.add(egui::DragValue::new(area).speed(5.0).range(0.0..=100000.0));
                                        ui.label("mm²");
                                    } else {
                                        ui.label("Bar Designation");
                                        ui.add(egui::TextEdit::singleline(bar).desired_width(66.0));
                                        ui.label("(eg: 25M)");
                                    }
                                    ui.end_row();
                                });
                            });
                            ui.add_space(6.0);
                        }
                        if !circle {
                            // side bars (this program's addition): one on each side face per
                            // layer, spread evenly between the top and bottom rows
                            ui.group(|ui| {
                                ui.set_min_width(420.0);
                                ui.label(if q.shape == 4 { "Side Reinforcement (each side face, between the corner bars)" } else { "Side (Skin) Reinforcement (each side face)" });
                                egui::Grid::new("qd3s").num_columns(3).spacing([16.0, 6.0]).show(ui, |ui| {
                                    ui.label("Bars per Side Face");
                                    ui.add(egui::DragValue::new(&mut q.side_n).range(0..=100));
                                    ui.label(format!("({} bars in all)", 2 * q.side_n));
                                    ui.end_row();
                                    ui.checkbox(&mut q.side_by_area, "Select bar by area");
                                    ui.end_row();
                                    if q.side_by_area {
                                        ui.label("Bar Area");
                                        ui.add(egui::DragValue::new(&mut q.side_area).speed(5.0).range(0.0..=100000.0));
                                        ui.label("mm²");
                                    } else {
                                        ui.label("Bar Designation");
                                        ui.add(egui::TextEdit::singleline(&mut q.side_bar).desired_width(66.0));
                                        ui.label("(eg: 20M)");
                                    }
                                    ui.end_row();
                                });
                            });
                        }
                    }
                    _ => {
                        ui.group(|ui| {
                            ui.set_min_width(420.0);
                            ui.label("Transverse Steel");
                            egui::Grid::new("qd4a").num_columns(3).spacing([16.0, 6.0]).show(ui, |ui| {
                                ui.label("Stirrup Type");
                                egui::ComboBox::from_id_salt("qdst").width(168.0).selected_text(STIRRUP_TYPES[q.stirrup_type]).show_ui(ui, |ui| {
                                    for (i, t) in STIRRUP_TYPES.iter().enumerate() {
                                        ui.selectable_value(&mut q.stirrup_type, i, *t);
                                    }
                                });
                                ui.end_row();
                                let on = q.stirrup_type > 0;
                                ui.add_enabled(on, egui::Checkbox::new(&mut q.st_by_area, "Select bar by area"));
                                ui.end_row();
                                if q.st_by_area {
                                    ui.label("Bar Area");
                                    ui.add_enabled(on, egui::DragValue::new(&mut q.st_area).speed(5.0).range(0.0..=100000.0));
                                    ui.label("mm²");
                                } else {
                                    ui.label("Bar Designation");
                                    ui.add_enabled(on, egui::TextEdit::singleline(&mut q.st_bar).desired_width(84.0));
                                    ui.label("(eg: 10M)");
                                }
                                ui.end_row();
                                ui.label("Spacing");
                                num(ui, &mut q.st_space, "mm", on);
                                ui.end_row();
                                ui.label("Clear Cover");
                                num(ui, &mut q.cover, "mm", true);
                                ui.end_row();
                            });
                        });
                        ui.add_space(6.0);
                        let ps = q.strand_type > 0;
                        ui.group(|ui| {
                            ui.set_min_width(420.0);
                            ui.label("Bottom Tendons");
                            egui::Grid::new("qd4b").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                                ui.label(if q.strand_type >= 3 { "Number of 15 mm strands" } else { "Number of 13 mm strands" });
                                ui.add_enabled(ps, egui::DragValue::new(&mut q.strands).range(0..=500));
                                ui.end_row();
                                ui.label("Delta-Epsilon P");
                                ui.horizontal(|ui| {
                                    ui.add_enabled(ps, egui::DragValue::new(&mut q.dep).speed(0.05).range(0.0..=20.0).fixed_decimals(3));
                                    ui.label("mm/m");
                                });
                                ui.end_row();
                            });
                        });
                    }
                }
                if let Some(e) = &qf.error {
                    ui.colored_label(Color32::from_rgb(180, 30, 30), e);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            qf.step = 99;
                        }
                        if qf.step == 3 {
                            if ui.button("Finish").clicked() {
                                finish = true;
                            }
                        } else if ui.button("Next >").clicked() {
                            qf.error = None;
                            if qf.step == 1 && qf.q.shape != qf.last_shape {
                                // as the original: a circle gets an outer ring of 16 bars, no inner
                                // ring and a hoop; the others a top bar, three bottom bars and a
                                // closed stirrup
                                let q = &mut qf.q;
                                if q.shape == 1 {
                                    q.top_n = 16;
                                    q.top_bar = "25M".into();
                                    q.bot_n = 0;
                                    q.bot_bar.clear();
                                    q.side_n = 0;
                                    q.stirrup_type = 4;
                                } else if q.shape == 4 {
                                    // a column: 3 bars on each of the four faces (8 x 25M), tied
                                    q.top_n = 3;
                                    q.top_bar = "25M".into();
                                    q.bot_n = 3;
                                    q.bot_bar = "25M".into();
                                    q.side_n = 1;
                                    q.side_bar = "25M".into();
                                    q.stirrup_type = 3;
                                    q.st_space = 300.0;
                                } else if qf.last_shape == 1 || qf.last_shape == 4 {
                                    q.top_n = 1;
                                    q.top_bar = "20M".into();
                                    q.bot_n = 3;
                                    q.bot_bar = "25M".into();
                                    q.side_n = 0;
                                    q.stirrup_type = 3;
                                    q.st_space = 250.0;
                                }
                                qf.last_shape = q.shape;
                            }
                            if qf.step == 1 && qf.q.shape == 1 && qf.q.dims[0] <= 0.0 {
                                qf.error = Some("Give the diameter.".into());
                            } else {
                                qf.step += 1;
                            }
                        }
                        if ui.add_enabled(qf.step > 0, egui::Button::new("< Back")).clicked() {
                            qf.step -= 1;
                            qf.error = None;
                        }
                    });
                });
            });
        if finish {
            match Section::from_quick(&self.quick.q) {
                Ok(sec) => {
                    self.sections = vec![sec.clone()];
                    self.sec = Some(sec);
                    self.path = None;
                    self.changed();
                    open = false;
                }
                Err(e) => self.quick.error = Some(e),
            }
        }
        if self.quick.step == 99 {
            open = false;
        }
        self.quick_open = open;
    }
}

/// The little shape pictures of the original's Quick Define list.
fn shape_icon(p: &egui::Painter, r: Rect, kind: usize) {
    let fill = Color32::from_gray(200);
    let line = Stroke::new(1.0, Color32::from_gray(60));
    let c = r.center();
    match kind {
        1 => {
            p.circle(c, r.width() * 0.45, fill, line);
        }
        2 | 3 => {
            let w = r.width();
            let h = r.height();
            let x0 = r.left();
            let y0 = r.top();
            let mut pts = vec![
                Pos2::new(x0, y0),
                Pos2::new(x0 + w, y0),
                Pos2::new(x0 + w, y0 + 0.25 * h),
                Pos2::new(x0 + 0.62 * w, y0 + 0.25 * h),
            ];
            if kind == 2 {
                pts.extend([Pos2::new(x0 + 0.62 * w, y0 + h), Pos2::new(x0 + 0.38 * w, y0 + h)]);
            } else {
                pts.extend([
                    Pos2::new(x0 + 0.62 * w, y0 + 0.75 * h),
                    Pos2::new(x0 + w, y0 + 0.75 * h),
                    Pos2::new(x0 + w, y0 + h),
                    Pos2::new(x0, y0 + h),
                    Pos2::new(x0, y0 + 0.75 * h),
                    Pos2::new(x0 + 0.38 * w, y0 + 0.75 * h),
                ]);
            }
            pts.extend([Pos2::new(x0 + 0.38 * w, y0 + 0.25 * h), Pos2::new(x0, y0 + 0.25 * h)]);
            p.add(egui::Shape::convex_polygon(vec![], fill, Stroke::NONE));
            p.add(egui::Shape::closed_line(pts.clone(), line));
            // fill the pieces (the outline is not convex)
            p.rect_filled(Rect::from_min_max(Pos2::new(x0 + 1.0, y0 + 1.0), Pos2::new(x0 + w - 1.0, y0 + 0.25 * h)), 0.0, fill);
            p.rect_filled(Rect::from_min_max(Pos2::new(x0 + 0.38 * w + 1.0, y0 + 0.25 * h), Pos2::new(x0 + 0.62 * w - 1.0, y0 + if kind == 2 { h - 1.0 } else { 0.75 * h })), 0.0, fill);
            if kind == 3 {
                p.rect_filled(Rect::from_min_max(Pos2::new(x0 + 1.0, y0 + 0.75 * h), Pos2::new(x0 + w - 1.0, y0 + h - 1.0)), 0.0, fill);
            }
            p.add(egui::Shape::closed_line(pts, line));
        }
        4 => {
            // a column: a square with bars round all four faces
            let s = r.shrink(3.0);
            p.rect(s, 0.0, fill, line, StrokeKind::Inside);
            let i = s.shrink(4.5);
            for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (0.0, 0.5), (1.0, 0.5), (0.0, 1.0), (0.5, 1.0), (1.0, 1.0)] {
                p.circle_filled(Pos2::new(i.left() + fx * i.width(), i.top() + fy * i.height()), 1.8, Color32::from_gray(40));
            }
        }
        _ => {
            p.rect(r.shrink(2.0), 0.0, fill, line, StrokeKind::Inside);
        }
    }
}


fn samples() -> Vec<(&'static str, fn() -> Section)> {
    vec![
        ("Rectangular beam with stirrups", || {
            Section::quick(
                "Beam with stirrups",
                40.0,
                19.0,
                400.0,
                Shape::Rect { b: 300.0, h: 600.0 },
                (3, "25M", 60.0),
                Some((2, "15M", 50.0)),
                Some(("10M", 250.0, 40.0)),
            )
        }),
        ("Beam without stirrups", || {
            Section::quick("Beam without stirrups", 35.0, 20.0, 450.0, Shape::Rect { b: 300.0, h: 700.0 }, (4, "25M", 60.0), None, None)
        }),
        ("T-beam", || {
            Section::quick(
                "T-beam",
                35.0,
                19.0,
                400.0,
                Shape::Tee { bf: 1000.0, hf: 150.0, bw: 300.0, h: 700.0 },
                (4, "30M", 70.0),
                Some((4, "15M", 50.0)),
                Some(("10M", 300.0, 40.0)),
            )
        }),
        ("Circular column", || {
            let mut s = Section::quick(
                "Circular column",
                40.0,
                19.0,
                400.0,
                Shape::Circle { d: 600.0 },
                (2, "25M", 80.0),
                Some((2, "25M", 80.0)),
                Some(("10M", 300.0, 50.0)),
            );
            s.long.push(LongLayer {
                name: "middle".into(),
                z: 300.0,
                num: 2,
                area: 1000.0,
                steel: "Long steel".into(),
                bartitle: "25M".into(),
                dep: 0.0, ring: 0, ring_id: 0, from_top: false,
            });
            s.loads.axial = -1500.0;
            s
        }),
    ]
}

/// Draw the cross-section: concrete, stirrup and bars, with a few dimensions.
pub fn draw_section(p: &egui::Painter, area: Rect, sec: &Section) {
    let h = sec.height();
    let b = sec.max_width();
    if h <= 0.0 || b <= 0.0 {
        return;
    }
    let area = area.shrink2(Vec2::new(32.0, 18.0));
    let s = (area.width() as f64 / b).min(area.height() as f64 / h);
    let cx = area.center().x as f64;
    let base = area.center().y as f64 + h * s / 2.0;
    let map = |y: f64, z: f64| Pos2::new((cx + y * s) as f32, (base - z * s) as f32);
    let fill = Color32::from_rgb(205, 205, 205);
    let n = 160;
    for i in 0..n {
        let z0 = h * i as f64 / n as f64;
        let z1 = h * (i + 1) as f64 / n as f64;
        let w = sec.width_at(0.5 * (z0 + z1));
        if w > 0.0 {
            let r = Rect::from_min_max(map(-w / 2.0, z1), map(w / 2.0, z0)).expand2(Vec2::new(0.0, 0.6));
            p.rect_filled(r, 0.0, fill);
        }
    }
    let mut ring: Vec<Pos2> = sec.outline.iter().map(|(z, w)| map(w / 2.0, *z)).collect();
    ring.extend(sec.outline.iter().rev().map(|(z, w)| map(-w / 2.0, *z)));
    p.add(egui::Shape::closed_line(ring, Stroke::new(1.5, Color32::BLACK)));
    // stirrup and bars as the original draws them
    let _ = crate::section_page::draw_steel(p, sec, &map, s);
    let f = FontId::proportional(11.0);
    let c = Color32::from_rgb(60, 60, 60);
    let top = map(0.0, h);
    p.text(Pos2::new(top.x, top.y - 3.0), Align2::CENTER_BOTTOM, format!("{:.0}", sec.width_at(h * 0.999)), f.clone(), c);
    let rt = map(b / 2.0, h);
    let rb = map(b / 2.0, 0.0);
    p.line_segment([Pos2::new(rt.x + 10.0, rt.y), Pos2::new(rb.x + 10.0, rb.y)], Stroke::new(1.0, c));
    p.text(Pos2::new(rt.x + 13.0, (rt.y + rb.y) / 2.0), Align2::LEFT_CENTER, format!("{:.0}", h), f.clone(), c);
    let bot = map(0.0, 0.0);
    p.text(Pos2::new(bot.x, bot.y + 3.0), Align2::CENTER_TOP, format!("{:.0}", sec.width_at(h * 0.001)), f, c);
}

/// A plain placeholder picture for the title page: sky and a fluted column drawn with shapes.
pub fn draw_column_art(p: &egui::Painter, r: Rect) {
    let steps = 40;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let c = Color32::from_rgb((150.0 + 60.0 * t) as u8, (185.0 + 40.0 * t) as u8, (225.0 + 20.0 * t) as u8);
        let y0 = r.top() + r.height() * t;
        p.rect_filled(
            Rect::from_min_max(Pos2::new(r.left(), y0), Pos2::new(r.right(), y0 + r.height() / steps as f32 + 1.0)),
            0.0,
            c,
        );
    }
    let stone = Color32::from_rgb(196, 178, 146);
    let shade = Color32::from_rgb(150, 130, 100);
    let band = Rect::from_min_max(
        Pos2::new(r.left(), r.top() + r.height() * 0.08),
        Pos2::new(r.right(), r.top() + r.height() * 0.22),
    );
    p.rect_filled(band, 0.0, stone);
    for k in 0..12 {
        let x = band.left() + band.width() * (k as f32 + 0.3) / 12.0;
        p.rect_filled(
            Rect::from_min_size(Pos2::new(x, band.top() + 4.0), Vec2::new(band.width() / 30.0, band.height() * 0.45)),
            0.0,
            shade,
        );
    }
    let cap = Rect::from_min_max(
        Pos2::new(r.left() + r.width() * 0.2, band.bottom()),
        Pos2::new(r.right() - r.width() * 0.2, band.bottom() + r.height() * 0.08),
    );
    p.rect_filled(cap, 4.0, stone);
    let shaft = Rect::from_min_max(Pos2::new(r.left() + r.width() * 0.3, cap.bottom()), Pos2::new(r.right() - r.width() * 0.3, r.bottom()));
    p.rect_filled(shaft, 0.0, stone);
    let flutes = 7;
    for k in 0..flutes {
        let x = shaft.left() + shaft.width() * (k as f32 + 0.5) / flutes as f32;
        p.line_segment([Pos2::new(x, shaft.top() + 6.0), Pos2::new(x, shaft.bottom())], Stroke::new(2.0, shade));
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 120, 120)), StrokeKind::Inside);
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        self.poll();
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let t = self.title();
        if ctx.data(|d| d.get_temp::<String>(egui::Id::new("title"))) != Some(t.clone()) {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("title"), t.clone()));
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.open_path(p);
        }
        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui, ctx);
        });
        egui::Panel::top("tools").show(ui, |ui| {
            self.toolbar(ui);
        });
        egui::Panel::bottom("status").exact_size(24.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.job.is_some() {
                    ui.add(egui::ProgressBar::new(self.progress.0).desired_width(260.0).text(self.progress.1.clone()));
                } else {
                    ui.label(&self.status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("SECTION").italics().underline().size(16.0).color(Color32::from_rgb(20, 40, 200)));
                    ui.separator();
                    if let Some(p) = &self.path {
                        ui.label(p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
                    }
                });
            });
        });
        let w = ctx.content_rect().width();
        // the cross-section page uses the whole window, as in the original
        if self.view != View::Section {
            egui::Panel::left("section")
                .resizable(true)
                .default_size((w * 0.245).max(260.0))
                .frame(egui::Frame::new().fill(Color32::from_rgb(240, 240, 240)))
                .show(ui, |ui| {
                    self.left_panel(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(6.0)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(100, 100, 100))),
            )
            .show(ui, |ui| {
                // a factored run is marked on every page, as the original prints "Factored Values"
                // (0x463ad0, 0x485fc0) and "FACTORED" in the Cross Section title (0x4026e0)
                if self.view != View::Welcome {
                    if let Some(sec) = self.sec.as_ref().filter(|s| is_factored(&s.phi)) {
                        ui.label(
                            RichText::new(format!(
                                "Factored Values: φc = {:.2}, φs = {:.2}, φp = {:.2} (design resistances; Options > Material Factors)",
                                sec.phi[0], sec.phi[1], sec.phi[2]
                            ))
                            .strong()
                            .color(Color32::from_rgb(160, 30, 30)),
                        );
                    }
                    if self.sec.as_ref().map(|s| s.aci.is_some()).unwrap_or(false)
                        && matches!(self.view, View::MPhi | View::Interaction | View::Radial)
                    {
                        if let Some(dm) = self.aci() {
                            let n = self.sec.as_ref().map(|s| s.loads.axial).unwrap_or(0.0);
                            ui.label(
                                RichText::new(format!("{}   (black curve: design = φ × nominal; Options > Design Factors)", crate::design::describe(&dm, n)))
                                    .strong()
                                    .color(Color32::from_rgb(160, 30, 30)),
                            );
                        }
                    }
                }
                match self.view {
                View::Welcome => self.welcome(ui),
                View::Section => {
                    if let Some(sec) = self.sec.clone() {
                        crate::section_page::section_page(ui, &sec, self.sections.len().max(1));
                    } else {
                        self.welcome(ui)
                    }
                }
                View::Nine => self.nine(ui),
                View::MPhi => self.mphi_view(ui),
                View::Shear => self.shear_view(ui),
                View::Interaction => self.inter_view(ui),
                View::Member => self.member_view(ui),
                View::Cases => self.cases_view(ui),
                View::Radial => self.radial_view(ui),
                View::Biaxial => self.biaxial_view(ui),
                View::Shear2d => self.shear2d_view(ui),
                View::LoadCases => self.load_cases_view(ui),
                }
            });
        self.dialogs(ctx);

        if let Some(path) = self.shot.clone() {
            self.frames += 1;
            ctx.request_repaint();
            // wait for a calculation started from the command line before taking the picture
            if self.frames >= 120 && !self.shot_requested && (self.job.is_none() || self.started.elapsed().as_secs() > 180) {
                self.shot_requested = true;
                let c2 = ctx.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&path);
                    }
                    c2.send_viewport_cmd(egui::ViewportCommand::Close);
                    c2.request_repaint();
                });
            }
            if self.started.elapsed().as_secs() > 240 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
