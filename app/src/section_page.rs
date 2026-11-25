//! The "Cross Section" page: properties table, dimensioned drawing, materials and title block,
//! laid out like the original program's cross-section view.

use crate::materials::{ConcreteProps, SteelProps};
use crate::model::*;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

pub struct SectionProps {
    pub n: f64,
    pub area: f64,
    pub inertia: f64,
    pub yt: f64,
    pub yb: f64,
    pub t_area: f64,
    pub t_inertia: f64,
    pub t_yt: f64,
    pub t_yb: f64,
}

/// Gross and transformed (bars counted as (n-1) times their area) section properties.
pub fn properties(sec: &Section) -> SectionProps {
    let h = sec.height();
    let conc = sec.concretes.first().map(ConcreteProps::new);
    let ec = conc.as_ref().map(|c| c.ec).unwrap_or(25000.0).max(1.0);
    let es = sec.rebars.first().map(|r| r.es).unwrap_or(200000.0);
    let n_strip = 2000;
    let dz = h / n_strip as f64;
    let (mut a, mut az, mut azz) = (0.0, 0.0, 0.0);
    for i in 0..n_strip {
        let z = (i as f64 + 0.5) * dz;
        let da = sec.width_at(z) * dz;
        a += da;
        az += da * z;
        azz += da * (z * z + dz * dz / 12.0);
    }
    let zc = if a > 0.0 { az / a } else { 0.0 };
    let inertia = azz - a * zc * zc;
    let (mut ta, mut taz, mut tazz) = (a, az, azz);
    for l in &sec.long {
        let e = sec
            .find_rebar(&l.steel)
            .map(|r| r.es)
            .or_else(|| sec.find_prestress(&l.steel).map(|p| p.ep))
            .unwrap_or(es);
        let extra = (e / ec - 1.0) * l.area;
        ta += extra;
        taz += extra * l.z;
        tazz += extra * l.z * l.z;
    }
    let tzc = if ta > 0.0 { taz / ta } else { 0.0 };
    SectionProps {
        n: es / ec,
        area: a,
        inertia,
        yt: h - zc,
        yb: zc,
        t_area: ta,
        t_inertia: tazz - ta * tzc * tzc,
        t_yt: h - tzc,
        t_yb: tzc,
    }
}

fn text(p: &egui::Painter, pos: Pos2, align: Align2, t: &str, size: f32, c: Color32) {
    p.text(pos, align, t, FontId::proportional(size), c);
}

fn bold(p: &egui::Painter, pos: Pos2, align: Align2, t: &str, size: f32) {
    let off = (size / 28.0).max(0.7);
    text(p, pos, align, t, size, Color32::BLACK);
    text(p, pos + Vec2::new(off, 0.0), align, t, size, Color32::BLACK);
}

fn underlined(p: &egui::Painter, pos: Pos2, t: &str, size: f32) {
    let g = p.layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::BLACK);
    let w = g.size().x;
    let hgt = g.size().y;
    p.galley(pos, g, Color32::BLACK);
    p.line_segment(
        [Pos2::new(pos.x, pos.y + hgt), Pos2::new(pos.x + w, pos.y + hgt)],
        Stroke::new(1.0, Color32::BLACK),
    );
}

pub fn section_page(ui: &mut egui::Ui, sec: &Section, n_elements: usize) {
    let r = ui.available_rect_before_wrap();
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, Color32::WHITE);
    let frame = r.shrink(8.0);
    let line = Stroke::new(1.0, Color32::BLACK);
    p.rect_stroke(frame, 0.0, line, StrokeKind::Inside);
    let s = (frame.height() / 1000.0).clamp(0.55, 1.6);
    let props = properties(sec);
    let black = Color32::BLACK;

    // ---------------------------------------------------------------- left column
    let left = Rect::from_min_size(frame.min, Vec2::new(frame.width() * 0.28, frame.height()));
    let table = Rect::from_min_size(left.min, Vec2::new(left.width(), frame.height() * 0.48));
    p.rect_stroke(table, 0.0, line, StrokeKind::Inside);
    let pad = 14.0 * s;
    underlined(&p, table.min + Vec2::new(pad, 6.0 * s), "Geometric Properties", 21.0 * s);
    let c0 = table.left() + table.width() * 0.29;
    let c1 = table.left() + table.width() * 0.45;
    let c2 = table.left() + table.width() * 0.80;
    let head_y = table.top() + 40.0 * s;
    text(&p, Pos2::new(c1, head_y), Align2::CENTER_TOP, "Gross Conc.", 17.0 * s, black);
    text(&p, Pos2::new(c2, head_y), Align2::CENTER_TOP, &format!("Trans (n={:.2})", props.n), 17.0 * s, black);
    let rule_y = head_y + 24.0 * s;
    p.line_segment([Pos2::new(table.left(), rule_y), Pos2::new(table.right(), rule_y)], line);
    let rows = [
        ("Area (mm²) x 10³", props.area / 1e3, props.t_area / 1e3, 1),
        ("Inertia (mm⁴) x 10⁶", props.inertia / 1e6, props.t_inertia / 1e6, 1),
        ("yt (mm)", props.yt, props.t_yt, 0),
        ("yb (mm)", props.yb, props.t_yb, 0),
        ("St (mm³) x 10³", props.inertia / props.yt.max(1e-9) / 1e3, props.t_inertia / props.t_yt.max(1e-9) / 1e3, 1),
        ("Sb (mm³) x 10³", props.inertia / props.yb.max(1e-9) / 1e3, props.t_inertia / props.t_yb.max(1e-9) / 1e3, 1),
    ];
    let row_h = 37.0 * s;
    let mut y = rule_y + 10.0 * s;
    for (label, g, t, dec) in rows {
        text(&p, Pos2::new(table.left() + 6.0 * s, y), Align2::LEFT_TOP, label, 16.5 * s, black);
        let f = |v: f64| if dec == 0 { format!("{:.0}", v) } else { format!("{:.1}", v) };
        text(&p, Pos2::new(c1, y), Align2::CENTER_TOP, &f(g), 16.5 * s, black);
        text(&p, Pos2::new(c2, y), Align2::CENTER_TOP, &f(t), 16.5 * s, black);
        y += row_h;
    }
    p.line_segment([Pos2::new(c0, rule_y), Pos2::new(c0, y)], line);
    p.line_segment([Pos2::new(table.left(), y), Pos2::new(table.right(), y)], line);
    // member
    underlined(&p, Pos2::new(table.left() + pad, y + 6.0 * s), "Full Member Properties", 21.0 * s);
    let mut my = y + 44.0 * s;
    let mline = |t: String, my: &mut f32| {
        text(&p, Pos2::new(table.left() + pad * 1.6, *my), Align2::LEFT_TOP, &t, 16.5 * s, black);
        *my += 33.0 * s;
    };
    match &sec.member {
        Some(m) => {
            mline(format!("Length: {:.1} mm", m.length), &mut my);
            mline(format!("Supports at {:.1} mm and {:.1} mm", m.left_sup, m.right_sup), &mut my);
            mline(format!("Point load at {:.1} mm", m.load_at), &mut my);
        }
        None => {
            mline("Length: not defined (simple span assumed".into(), &mut my);
            mline(format!("  of {:.0} mm for load-deflection)", 10.0 * sec.height()), &mut my);
        }
    }
    // crack spacing and loading
    let mut ty = table.bottom() + 22.0 * s;
    text(&p, Pos2::new(left.left() + pad, ty), Align2::LEFT_TOP, "Crack Spacing", 16.5 * s, black);
    ty += 38.0 * s;
    text(&p, Pos2::new(left.left() + pad * 3.0, ty), Align2::LEFT_TOP, "gap between bar layers (100 mm to 0.9 h)", 16.0 * s, black);
    ty += 44.0 * s;
    text(&p, Pos2::new(left.left() + pad, ty), Align2::LEFT_TOP, "Loading (N,M,V + dN,dM,dV)", 16.5 * s, black);
    ty += 38.0 * s;
    text(
        &p,
        Pos2::new(left.left() + pad * 3.0, ty),
        Align2::LEFT_TOP,
        &format!(
            "{:.1} , {:.1} , {:.1}  +  {:.1} , {:.1} , {:.1}",
            sec.loads.axial, sec.loads.m_const, sec.loads.v_const, sec.loads.n_inc, sec.loads.m_inc, sec.loads.v_inc
        ),
        16.5 * s,
        black,
    );

    // materials
    let mat_top = frame.bottom() - frame.height() * 0.24;
    let mw = frame.width() * 0.165;
    let conc_r = Rect::from_min_size(Pos2::new(left.left() + pad, mat_top), Vec2::new(mw, frame.bottom() - mat_top - 16.0 * s));
    let bar_r = Rect::from_min_size(Pos2::new(conc_r.right() + 30.0 * s, mat_top), conc_r.size());
    if let Some(c) = sec.concretes.first() {
        let cp = ConcreteProps::new(c);
        bold(&p, Pos2::new(conc_r.center().x, conc_r.top()), Align2::CENTER_TOP, "Concrete", 30.0 * s);
        let plot = Rect::from_min_max(conc_r.min + Vec2::new(0.0, 42.0 * s), conc_r.max);
        p.line_segment([plot.left_top(), plot.left_bottom()], line);
        p.line_segment([plot.left_bottom(), plot.right_bottom()], line);
        let emax = 2.6 * cp.e0;
        let pts: Vec<Pos2> = (0..=60)
            .map(|i| {
                let e = emax * i as f64 / 60.0;
                let f = -cp.compression(-e, 0.0) / cp.fc;
                Pos2::new(plot.left() + plot.width() * (e / emax) as f32, plot.bottom() - plot.height() * 0.9 * f as f32)
            })
            .collect();
        p.add(egui::Shape::line(pts, line));
        text(&p, plot.left_top() + Vec2::new(4.0, 0.0), Align2::LEFT_TOP, &format!("f'c = {:.1} MPa", c.fc), 18.0 * s, black);
        let rx = plot.right() - 2.0;
        text(&p, Pos2::new(rx, plot.top() + plot.height() * 0.36), Align2::RIGHT_TOP, &format!("ag = {:.0} mm", c.agg), 18.0 * s, black);
        let auto = if c.ft.is_none() { " (auto)" } else { "" };
        text(&p, Pos2::new(rx, plot.top() + plot.height() * 0.56), Align2::RIGHT_TOP, &format!("ft = {:.2} MPa{}", cp.ft, auto), 18.0 * s, black);
        text(&p, Pos2::new(rx, plot.bottom() - 4.0), Align2::RIGHT_BOTTOM, &format!("e'c = {:.2} mm/m", cp.e0 * 1000.0), 18.0 * s, black);
    }
    if let Some(rb) = sec.rebars.first() {
        let st = SteelProps::from_rebar(rb);
        bold(&p, Pos2::new(bar_r.center().x, bar_r.top()), Align2::CENTER_TOP, "Rebar", 30.0 * s);
        let plot = Rect::from_min_max(bar_r.min + Vec2::new(0.0, 42.0 * s), bar_r.max);
        p.line_segment([plot.left_top(), plot.left_bottom()], line);
        p.line_segment([plot.left_bottom(), plot.right_bottom()], line);
        let emax = st.eu() * 0.999;
        let fmax = rb.fu.max(rb.fy);
        let pts: Vec<Pos2> = (0..=120)
            .map(|i| {
                let e = emax * i as f64 / 120.0;
                let f = st.stress(e) / fmax;
                Pos2::new(plot.left() + plot.width() * (e / emax) as f32, plot.bottom() - plot.height() * 0.85 * f as f32)
            })
            .collect();
        p.add(egui::Shape::line(pts, line));
        text(&p, plot.left_top() + Vec2::new(4.0, 0.0), Align2::LEFT_TOP, &format!("fu = {:.0} MPa", rb.fu), 18.0 * s, black);
        text(&p, Pos2::new(plot.left() + plot.width() * 0.42, plot.top() + plot.height() * 0.38), Align2::LEFT_TOP, &format!("fy = {:.0}", rb.fy), 18.0 * s, black);
        text(&p, Pos2::new(plot.right() - 2.0, plot.bottom() - 4.0), Align2::RIGHT_BOTTOM, &format!("es = {:.1} mm/m", rb.eu), 18.0 * s, black);
    }

    // ---------------------------------------------------------------- right column
    let right_w = frame.width() * 0.15;
    let cat = Rect::from_min_size(Pos2::new(frame.right() - right_w, frame.top()), Vec2::new(right_w, frame.height() * 0.26));
    p.rect_stroke(cat, 0.0, line, StrokeKind::Inside);
    let cat_head = cat.top() + cat.height() * 0.32;
    p.line_segment([Pos2::new(cat.left(), cat_head), Pos2::new(cat.right(), cat_head)], line);
    underlined(&p, Pos2::new(cat.center().x - 55.0 * s, cat.top() + 18.0 * s), "Element Catalog", 17.0 * s);
    text(&p, Pos2::new(cat.center().x, cat.top() + 48.0 * s), Align2::CENTER_TOP, &format!("Number of Elements: {}", n_elements), 13.0 * s, black);
    let thumb = Rect::from_min_max(Pos2::new(cat.left() + cat.width() * 0.3, cat_head + 18.0 * s), Pos2::new(cat.right() - cat.width() * 0.3, cat.bottom() - 6.0 * s));
    p.rect_stroke(thumb, 0.0, line, StrokeKind::Inside);
    draw_detailed(&p, thumb.shrink(4.0), sec, false, s * 0.4);
    {
        let ay = cat_head + 11.0 * s;
        let ax = cat.left() + 6.0 * s;
        p.add(egui::Shape::convex_polygon(
            vec![Pos2::new(ax, ay), Pos2::new(ax + 14.0 * s, ay - 4.0 * s), Pos2::new(ax + 14.0 * s, ay + 4.0 * s)],
            black,
            Stroke::NONE,
        ));
        p.line_segment([Pos2::new(ax + 14.0 * s, ay), Pos2::new(ax + 24.0 * s, ay)], line);
        text(&p, Pos2::new(ax + 27.0 * s, ay), Align2::LEFT_CENTER, &sec.title, 12.5 * s, black);
    }
    p.line_segment([Pos2::new(cat.left(), cat.bottom()), Pos2::new(cat.left(), frame.bottom())], line);

    // title block
    let tb_top = frame.bottom() - frame.height() * 0.145;
    let tb = Rect::from_min_max(Pos2::new(frame.left() + frame.width() * 0.55, tb_top), Pos2::new(cat.left(), frame.bottom()));
    p.rect_stroke(tb, 0.0, line, StrokeKind::Inside);
    let pic = Rect::from_min_size(tb.min, Vec2::new(tb.width() * 0.27, tb.height()));
    crate::ui::draw_column_art(&p, pic.shrink(3.0));
    p.line_segment([pic.right_top(), pic.right_bottom()], line);
    let name_y = tb.top() + tb.height() * 0.5;
    p.line_segment([Pos2::new(pic.right(), name_y), Pos2::new(tb.right(), name_y)], line);
    let avail = (tb.right() - pic.right() - 16.0).max(20.0);
    let probe = p.layout_no_wrap(sec.title.clone(), FontId::proportional(30.0 * s), Color32::BLACK).size().x.max(1.0);
    let tsize = (30.0 * s * (avail / probe).min(1.0)).max(9.0);
    bold(&p, Pos2::new((pic.right() + tb.right()) / 2.0, tb.top() + tb.height() * 0.25), Align2::CENTER_CENTER, &sec.title, tsize);
    if !sec.done_by.is_empty() || !sec.date.is_empty() {
        text(&p, Pos2::new((pic.right() + tb.right()) / 2.0, tb.top() + tb.height() * 0.75), Align2::CENTER_CENTER, &format!("{}   {}", sec.done_by, sec.date), 17.0 * s, black);
    }
    text(&p, Pos2::new(cat.left() - 20.0 * s, tb.top() - 78.0 * s), Align2::RIGHT_BOTTOM, "All dimensions in millimetres", 25.0 * s, black);
    if let Some(st) = &sec.stirrups {
        let cover = (st.z_bot.min(sec.height() - st.z_top) - st.db / 2.0).max(0.0);
        text(&p, Pos2::new(cat.left() - 20.0 * s, tb.top() - 34.0 * s), Align2::RIGHT_BOTTOM, &format!("Clear cover to reinforcement =    {:.0} mm", cover), 25.0 * s, black);
    }

    // ---------------------------------------------------------------- main drawing
    let draw = Rect::from_min_max(
        Pos2::new(left.right() + 60.0 * s, frame.top() + frame.height() * 0.12),
        Pos2::new(cat.left() - frame.width() * 0.17, frame.top() + frame.height() * 0.68),
    );
    draw_detailed(&p, draw, sec, true, s);
    ui.allocate_rect(r, egui::Sense::hover());
}

/// Draw the stirrup and the bars as a reinforcement detail: the original's layout (r2k 0x4a32f0
/// and its helpers), drawn the way a detailing drawing shows bent bars.
/// * A bent bar is its centre line (straight runs and bends) drawn as its two faces, half a bar
///   either side, so every bend's faces are concentric arcs running on tangentially into the
///   straight parts, and both ends are squared off.
/// * Bends have the bend diameter D = max(4 db, (57.2957 db[m] + 2.5) db) inside (0x4a6290), or
///   the corner bar's diameter where that is larger, so the corner bar fits the bend.
/// * Hooks (0x4a6c90) bend 135 degrees around the corner bar and run on in a straight tail
///   max(6 db, 60 mm) long (0x52f080, 0x52e598) into the core.
/// * Closed stirrup: rounded rectangle with both ends hooked around the top-left corner bar,
///   tails at -45 degrees (0x4a69a0); open stirrup: a U whose top ends hook inwards around the
///   corner bars; single leg: one leg just left of the middle bar with a hook at each end;
///   T-headed: one leg with plates 6 db wide at both ends (0x4a7310); hoop: a circle on the
///   stirrup's height (0x4a2540); interlocking hoops: two such circles, one at the top and one at
///   the bottom.
/// * The stirrup's outer faces sit at the clear cover from the sides.
/// * Bars (0x4a2b00): circles of the bar's diameter, white (grey for strands), where
///   `bar_draw_points` seats them: on the stirrup's legs and in its bends.
/// Returns the point to hang the stirrup label on (screen) and the stirrup.
pub fn draw_steel<'a>(p: &egui::Painter, sec: &'a Section, map: &dyn Fn(f64, f64) -> Pos2, sc: f64) -> Option<(Pos2, &'a Stirrups)> {
    let thin = Stroke::new(1.0, Color32::BLACK);
    let line = |a: (f64, f64), b: (f64, f64)| p.line_segment([map(a.0, a.1), map(b.0, b.1)], thin);
    let circle = |c: (f64, f64), r: f64| {
        let n = 120;
        let pts: Vec<Pos2> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                map(c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect();
        p.add(egui::Shape::closed_line(pts, thin));
    };
    let d45 = std::f64::consts::FRAC_1_SQRT_2;
    let mut anchor = None;
    let st = sec.stirrups.as_ref();
    if let Some(st) = st {
        let db = st.db.max(1.0);
        let w = db;
        let tail = (6.0 * db).max(60.0);
        let zt = st.z_top.max(st.z_bot);
        let zb = st.z_top.min(st.z_bot);
        let mid_y = 0.5 * (zt + zb);
        match st.pattern {
            4 | 6 => {
                let wmid = sec.width_at(mid_y);
                let cover = (zb - db / 2.0).max(0.0);
                let r = if st.pattern == 4 { (zt - zb) / 2.0 } else { ((zt - zb) / 2.0).min(wmid / 2.0 - cover - db / 2.0) };
                let centres = if st.pattern == 4 { vec![mid_y] } else { vec![zt - r, zb + r] };
                for (k, zc) in centres.iter().enumerate() {
                    circle((0.0, *zc), r + db / 2.0);
                    circle((0.0, *zc), r - db / 2.0);
                    if k == 0 {
                        anchor = Some(map(r + db / 2.0, *zc));
                    }
                }
            }
            1 | 5 => {
                // the leg just left of the middle bar of the rows with one
                let off = sec.long.iter().filter(|l| l.num % 2 == 1).map(|l| l.draw_dia() / 2.0 + db / 2.0).fold(0.0, f64::max);
                let xl = -off;
                if st.pattern == 5 {
                    line((xl - w / 2.0, zt), (xl - w / 2.0, zb));
                    line((xl + w / 2.0, zt), (xl + w / 2.0, zb));
                    for zc in [zt, zb] {
                        let (x0, x1) = (xl - 3.0 * w, xl + 3.0 * w);
                        let (z0, z1) = (zc - w / 2.0, zc + w / 2.0);
                        line((x0, z0), (x1, z0));
                        line((x1, z0), (x1, z1));
                        line((x1, z1), (x0, z1));
                        line((x0, z1), (x0, z0));
                    }
                } else {
                    let rc = bend_inner_radius(db) + w / 2.0;
                    let ct = (xl + rc, zt - rc);
                    let cb = (xl + rc, zb + rc);
                    let t0 = (ct.0 + rc * d45, ct.1 + rc * d45);
                    let b0 = (cb.0 + rc * d45, cb.1 - rc * d45);
                    let path = [
                        Seg::Line((t0.0 + tail * d45, t0.1 - tail * d45), t0),
                        Seg::Arc(ct, rc, 45.0, 180.0),
                        Seg::Line((xl, ct.1), (xl, cb.1)),
                        Seg::Arc(cb, rc, 180.0, 315.0),
                        Seg::Line(b0, (b0.0 + tail * d45, b0.1 + tail * d45)),
                    ];
                    bent_bar(p, map, &path, w, thin);
                }
                anchor = Some(map(xl + w / 2.0, mid_y));
            }
            _ => {
                // rectangular stirrup (open or closed), traced from the tail of the hook at the
                // top-left corner, down the left leg, along the bottom and up the right leg
                if let Some(t) = rect_tie(sec) {
                    let x = t.xi + w / 2.0; // centre line, half width
                    let (rb, rt) = (t.ri_b + w / 2.0, t.ri_t + w / 2.0); // centre-line bend radii
                    let (bl, br) = ((-x + rb, zb + rb), (x - rb, zb + rb));
                    let (tl, tr) = ((-x + rt, zt - rt), (x - rt, zt - rt));
                    let h0 = (tl.0 + rt * d45, tl.1 + rt * d45);
                    let mut path = vec![
                        Seg::Line((h0.0 + tail * d45, h0.1 - tail * d45), h0),
                        Seg::Arc(tl, rt, 45.0, 180.0),
                        Seg::Line((-x, tl.1), (-x, bl.1)),
                        Seg::Arc(bl, rb, 180.0, 270.0),
                        Seg::Line((bl.0, zb), (br.0, zb)),
                        Seg::Arc(br, rb, 270.0, 360.0),
                        Seg::Line((x, br.1), (x, tr.1)),
                    ];
                    if st.pattern == 2 {
                        // open: the right leg hooks inwards around its corner bar too
                        let e = (tr.0 - rt * d45, tr.1 + rt * d45);
                        path.push(Seg::Arc(tr, rt, 0.0, 135.0));
                        path.push(Seg::Line(e, (e.0 - tail * d45, e.1 - tail * d45)));
                    } else {
                        // closed: across the top and round the top-left corner bar a second time
                        let e = (tl.0 - rt * d45, tl.1 - rt * d45);
                        path.push(Seg::Arc(tr, rt, 0.0, 90.0));
                        path.push(Seg::Line((tr.0, zt), (tl.0, zt)));
                        path.push(Seg::Arc(tl, rt, 90.0, 225.0));
                        path.push(Seg::Line(e, (e.0 + tail * d45, e.1 - tail * d45)));
                    }
                    bent_bar(p, map, &path, w, thin);
                    anchor = Some(map(t.xi + w, mid_y));
                }
            }
        }
    }
    // bars
    for l in &sec.long {
        let db = l.draw_dia().max(1.0);
        let r = ((db / 2.0) * sc).max(2.5) as f32;
        let ps = sec.prestress.iter().any(|ps| ps.name == l.steel);
        for (x, z) in bar_draw_points(sec, l) {
            let c = map(x, z);
            p.circle_filled(c, r, if ps { Color32::from_gray(190) } else { Color32::WHITE });
            p.circle_stroke(c, r, thin);
        }
    }
    st.and_then(|s| anchor.map(|a| (a, s)))
}

/// Inside radius of a stirrup bend: half the bend diameter max(4 db, (57.2957 db[m] + 2.5) db)
/// (r2k 0x4a6290).
fn bend_inner_radius(db: f64) -> f64 {
    (4.0 * db).max((57.2957 * db / 1000.0 + 2.5) * db) / 2.0
}

/// A piece of a bent bar's centre line: a straight run, or a bend (centre, radius, start and end
/// angle in degrees, counter-clockwise positive; an end angle below the start runs clockwise).
#[derive(Clone, Copy)]
enum Seg {
    Line((f64, f64), (f64, f64)),
    Arc((f64, f64), f64, f64, f64),
}

/// A bent bar of diameter w along its centre line: both faces as continuous lines, half a bar
/// either side of the centre line (so each bend's faces are concentric arcs running on into the
/// straight parts), and both ends squared off. The path must run on tangentially.
fn bent_bar(p: &egui::Painter, map: &dyn Fn(f64, f64) -> Pos2, path: &[Seg], w: f64, stroke: Stroke) {
    // side +1: the face on the left of the direction of travel
    let face = |side: f64| -> Vec<Pos2> {
        let mut pts: Vec<Pos2> = vec![];
        let mut push = |q: Pos2| {
            if pts.last().map(|l: &Pos2| l.distance(q) > 0.05).unwrap_or(true) {
                pts.push(q);
            }
        };
        for s in path {
            match *s {
                Seg::Line(a, b) => {
                    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-9);
                    let (nx, nz) = (-dz / len * side * w / 2.0, dx / len * side * w / 2.0);
                    push(map(a.0 + nx, a.1 + nz));
                    push(map(b.0 + nx, b.1 + nz));
                }
                Seg::Arc(c, r, a1, a2) => {
                    // counter-clockwise the left face is the inside of the bend
                    let rr = if a2 > a1 { r - side * w / 2.0 } else { r + side * w / 2.0 };
                    let n = ((a2 - a1).abs() / 3.0).ceil().max(2.0) as usize;
                    for i in 0..=n {
                        let a = (a1 + (a2 - a1) * i as f64 / n as f64).to_radians();
                        push(map(c.0 + rr * a.cos(), c.1 + rr * a.sin()));
                    }
                }
            }
        }
        pts
    };
    let (l, r) = (face(1.0), face(-1.0));
    if let (Some(l0), Some(r0), Some(l1), Some(r1)) = (l.first(), r.first(), l.last(), r.last()) {
        p.line_segment([*l0, *r0], stroke);
        p.line_segment([*l1, *r1], stroke);
    }
    p.add(egui::Shape::line(l, stroke));
    p.add(egui::Shape::line(r, stroke));
}

/// A rectangular stirrup (open or closed) as the detail draws it: half width of its inner faces,
/// heights of the inner faces of its bottom and top legs, and the inside radii of its bottom and
/// top bends (the bend diameter's, or the corner bar's where larger).
struct RectTie {
    xi: f64,
    zin_b: f64,
    zin_t: f64,
    ri_b: f64,
    ri_t: f64,
}

#[derive(PartialEq)]
enum Leg {
    Bottom,
    Top,
}

impl RectTie {
    /// The leg a row of bars rests on: one closer to it than its bar radius (or into it).
    fn leg_of(&self, l: &LongLayer) -> Option<Leg> {
        let rb = l.draw_dia().max(1.0) / 2.0;
        let mid = 0.5 * (self.zin_b + self.zin_t);
        if l.z < mid && (l.z - rb) - self.zin_b < rb {
            Some(Leg::Bottom)
        } else if l.z > mid && self.zin_t - (l.z + rb) < rb {
            Some(Leg::Top)
        } else {
            None
        }
    }

    /// The farthest a bar of radius rb can sit from the centre line at height z, inside the
    /// stirrup's inner faces and bends.
    fn x_limit(&self, z: f64, rb: f64) -> f64 {
        let (rcb, rct) = ((self.ri_b - rb).max(0.0), (self.ri_t - rb).max(0.0));
        let (zcb, zct) = (self.zin_b + self.ri_b, self.zin_t - self.ri_t);
        if z < zcb {
            let dz = (zcb - z).min(rcb);
            self.xi - self.ri_b + (rcb * rcb - dz * dz).max(0.0).sqrt()
        } else if z > zct {
            let dz = (z - zct).min(rct);
            self.xi - self.ri_t + (rct * rct - dz * dz).max(0.0).sqrt()
        } else {
            self.xi - rb
        }
    }
}

/// Whether `bar_xs` spreads a row between a rectangular stirrup's inner faces (inner half width
/// xi), rather than across the section.
fn in_tie(sec: &Section, l: &LongLayer, xi: f64) -> bool {
    let h = sec.height();
    let db = l.draw_dia().max(1.0);
    let n = l.num.max(1) as f64;
    let wl = sec.width_at((l.z - db / 2.0).max(0.0)).min(sec.width_at((l.z + db / 2.0).min(h))).max(1.0);
    l.ring == 0 && l.num >= 2 && 2.0 * xi > n * db && 2.0 * xi / wl > 0.3
}

fn rect_tie(sec: &Section) -> Option<RectTie> {
    let st = sec.stirrups.as_ref()?;
    let xi = rect_inner_half(sec)?;
    let db = st.db.max(1.0);
    let (zt, zb) = (st.z_top.max(st.z_bot), st.z_top.min(st.z_bot));
    let ri = bend_inner_radius(db);
    let mut t = RectTie { xi, zin_b: zb + db / 2.0, zin_t: zt - db / 2.0, ri_b: ri, ri_t: ri };
    let (mut big_b, mut big_t) = (0.0f64, 0.0f64);
    for l in sec.long.iter().filter(|l| in_tie(sec, l, xi)) {
        let rb = l.draw_dia().max(1.0) / 2.0;
        match t.leg_of(l) {
            Some(Leg::Bottom) => big_b = big_b.max(rb),
            Some(Leg::Top) => big_t = big_t.max(rb),
            None => {}
        }
    }
    t.ri_b = t.ri_b.max(big_b);
    t.ri_t = t.ri_t.max(big_t);
    Some(t)
}

/// Where the drawing puts a row's bars (mm from the centre line, mm up): as `bar_xs` and the
/// row's height, except that inside a rectangular stirrup a row on one of its legs rests on that
/// leg with its end bars seated in the bends (touching the bend on its 45-degree line), and other
/// rows' end bars stay clear of the bends. Drawing only: the analyses keep `bar_xs` and the row's
/// own height.
pub fn bar_draw_points(sec: &Section, l: &LongLayer) -> Vec<(f64, f64)> {
    let xs = bar_xs(sec, l);
    let plain = || xs.iter().map(|x| (*x, l.z)).collect::<Vec<_>>();
    let Some(t) = rect_tie(sec) else { return plain() };
    if !in_tie(sec, l, t.xi) || xs.len() < 2 {
        return plain();
    }
    let d45 = std::f64::consts::FRAC_1_SQRT_2;
    let rb = l.draw_dia().max(1.0) / 2.0;
    // the right-hand end bar, and the height of the bars between
    let (end, z_row) = match t.leg_of(l) {
        Some(Leg::Bottom) => {
            let rc = (t.ri_b - rb).max(0.0);
            ((t.xi - t.ri_b + rc * d45, t.zin_b + t.ri_b - rc * d45), t.zin_b + rb)
        }
        Some(Leg::Top) => {
            let rc = (t.ri_t - rb).max(0.0);
            ((t.xi - t.ri_t + rc * d45, t.zin_t - t.ri_t + rc * d45), t.zin_t - rb)
        }
        None => ((t.x_limit(l.z, rb), l.z), l.z),
    };
    let n = xs.len();
    (0..n)
        .map(|i| {
            if i == 0 {
                end
            } else if i == n - 1 {
                (-end.0, end.1)
            } else {
                (end.0 - 2.0 * end.0 * i as f64 / (n - 1) as f64, z_row)
            }
        })
        .collect()
}

/// The inner faces' half width of a rectangular stirrup (open or closed), as draw_steel places
/// it; None for single legs, hoops and no stirrup.
fn rect_inner_half(sec: &Section) -> Option<f64> {
    let st = sec.stirrups.as_ref()?;
    if matches!(st.pattern, 1 | 4 | 5 | 6) {
        return None;
    }
    let db = st.db.max(1.0);
    let zt = st.z_top.max(st.z_bot);
    let zb = st.z_top.min(st.z_bot);
    let mid_y = 0.5 * (zt + zb);
    let wmin = [zb + 1.0, mid_y, zt - 1.0].iter().map(|z| sec.width_at(*z)).fold(f64::INFINITY, f64::min);
    let mut c = (zb - db / 2.0).max(0.0);
    if wmin < 2.0 * c + 2.0 * db {
        c = (wmin - 2.0 * db) / 3.0;
    }
    Some(wmin / 2.0 - c - db)
}

/// The horizontal positions (mm from the section's centre line) of the bars of one layer, as
/// the drawing places them (r2k 0x4a2b00): a ring pair on its circle; one bar on the centre
/// line; otherwise edge to edge between a rectangular stirrup's inner faces when they leave
/// room, else splitting the width in n + 1 equal parts. Used for biaxial bending too.
pub fn bar_xs(sec: &Section, l: &LongLayer) -> Vec<f64> {
    let h = sec.height();
    let db = l.draw_dia().max(1.0);
    let n = l.num.max(1) as usize;
    let wl = sec.width_at((l.z - db / 2.0).max(0.0)).min(sec.width_at((l.z + db / 2.0).min(h))).max(1.0);
    if l.ring > 0 && n <= 2 {
        return match ring_geom(sec, l.ring_id) {
            Some((zc, r)) if n == 2 => {
                let x = (r * r - (l.z - zc).powi(2)).max(0.0).sqrt();
                vec![-x, x]
            }
            _ => vec![0.0],
        };
    }
    if n == 1 {
        return vec![0.0];
    }
    match rect_inner_half(sec) {
        Some(xi) if 2.0 * xi > n as f64 * db && 2.0 * xi / wl > 0.3 => {
            let span = 2.0 * xi;
            (0..n).map(|i| span / 2.0 - db / 2.0 - i as f64 * (span - db) / (n - 1) as f64).collect()
        }
        _ => (1..=n).map(|i| -(wl / 2.0 - i as f64 * wl / (n + 1) as f64)).collect(),
    }
}

/// Centre height and radius of a ring of bars (layers with the same ring id): the ring's top bar
/// sits at angle 0 and the next layer at 2 pi / n, as the original's Quick Define lays them.
pub fn ring_geom(sec: &Section, id: u32) -> Option<(f64, f64)> {
    let mut zs: Vec<(f64, u32)> = sec.long.iter().filter(|l| l.ring > 0 && l.ring_id == id).map(|l| (l.z, l.ring)).collect();
    if zs.is_empty() {
        return None;
    }
    zs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let n = zs[0].1.max(2) as f64;
    if zs.len() >= 2 {
        let r = (zs[0].0 - zs[1].0) / (1.0 - (2.0 * std::f64::consts::PI / n).cos());
        Some((zs[0].0 - r, r))
    } else {
        let zc = sec.height() / 2.0;
        Some((zc, (zs[0].0 - zc).abs()))
    }
}

/// Where a label leader meets the stirrup at height z (its outer face, on the right).
fn stirrup_attach(sec: &Section, st: &Stirrups, z: f64) -> f64 {
    let (zt, zb) = (st.z_top.max(st.z_bot), st.z_top.min(st.z_bot));
    let db = st.db;
    let mid = 0.5 * (zt + zb);
    match st.pattern {
        4 | 6 => {
            let cover = (zb - db / 2.0).max(0.0);
            let r = if st.pattern == 4 { (zt - zb) / 2.0 } else { ((zt - zb) / 2.0).min(sec.width_at(mid) / 2.0 - cover - db / 2.0) };
            let zc = if st.pattern == 4 { mid } else if z > mid { zt - r } else { zb + r };
            let ro = r + db / 2.0;
            (ro * ro - (z - zc).powi(2)).max(0.0).sqrt()
        }
        1 | 5 => {
            let off = sec.long.iter().filter(|l| l.num % 2 == 1).map(|l| l.draw_dia() / 2.0 + db / 2.0).fold(0.0, f64::max);
            -off + db / 2.0
        }
        _ => {
            let wmin = [zb + 1.0, mid, zt - 1.0].iter().map(|z| sec.width_at(*z)).fold(f64::INFINITY, f64::min);
            let mut c = (zb - db / 2.0).max(0.0);
            if wmin < 2.0 * c + 2.0 * db {
                c = (wmin - 2.0 * db) / 3.0;
            }
            wmin / 2.0 - c
        }
    }
}

/// Section drawing with a light fill, the stirrup and the bars as the original draws them; with
/// `labels`, dimension lines and bar / stirrup labels as on the original's cross-section page.
pub fn draw_detailed(p: &egui::Painter, area: Rect, sec: &Section, labels: bool, s: f32) {
    let h = sec.height();
    let b = sec.max_width();
    if h <= 0.0 || b <= 0.0 {
        return;
    }
    let sc = (area.width() as f64 / b).min(area.height() as f64 / h);
    let cx = area.center().x as f64;
    let base = area.center().y as f64 + h * sc / 2.0;
    let map = |y: f64, z: f64| Pos2::new((cx + y * sc) as f32, (base - z * sc) as f32);
    let fill = Color32::from_rgb(242, 242, 242);
    let n = 200;
    for i in 0..n {
        let z0 = h * i as f64 / n as f64;
        let z1 = h * (i + 1) as f64 / n as f64;
        let w = sec.width_at(0.5 * (z0 + z1));
        if w > 0.0 {
            p.rect_filled(Rect::from_min_max(map(-w / 2.0, z1), map(w / 2.0, z0)).expand2(Vec2::new(0.0, 0.6)), 0.0, fill);
        }
    }
    let mut ring: Vec<Pos2> = sec.outline.iter().map(|(z, w)| map(w / 2.0, *z)).collect();
    ring.extend(sec.outline.iter().rev().map(|(z, w)| map(-w / 2.0, *z)));
    p.add(egui::Shape::closed_line(ring, Stroke::new(1.3, Color32::BLACK)));
    let thin = Stroke::new(1.0, Color32::BLACK);
    // stirrup and bars as the original draws them
    let stir = draw_steel(p, sec, &map, sc).map(|(_, st)| st);
    if !labels {
        return;
    }
    let f = 25.0 * s;
    let right_edge = map(b / 2.0, 0.0).x;
    // bar labels: one a row; a ring of bars gets one label ("16 - 20M") whose leader reaches the
    // ring's bar just above mid-height, as on the original's cross-section page
    let mut rows: Vec<(f64, String, Option<f64>)> = vec![];
    let mut rings_done: Vec<u32> = vec![];
    for l in &sec.long {
        let name = if l.bartitle.is_empty() { format!("{:.0} mm²", l.area) } else { l.bartitle.clone() };
        if l.ring > 0 {
            if rings_done.contains(&l.ring_id) {
                continue;
            }
            rings_done.push(l.ring_id);
            let mut group: Vec<&LongLayer> = sec.long.iter().filter(|m| m.ring > 0 && m.ring_id == l.ring_id).collect();
            group.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap());
            let k = ((l.ring as f64 / 4.0).ceil() as usize).saturating_sub(1).min(group.len() - 1);
            let g = group[k];
            let x = ring_geom(sec, l.ring_id).map(|(zc, r)| (r * r - (g.z - zc).powi(2)).max(0.0).sqrt()).unwrap_or(0.0) + g.draw_dia() / 2.0;
            rows.push((g.z, format!("{} - {}", l.ring, name), Some(x)));
        } else {
            rows.push((l.z, format!("{} - {}", l.num.max(1), name), None));
        }
    }
    let lx1 = right_edge + 62.0 * s;
    // labels in a column; crowded ones are pushed apart and their leaders slant from a tick just
    // off the section's edge at the bar's height to the label, as the original does; strands get
    // a second line with their locked-in strain
    let line_h = f * 1.2;
    let mut placed: Vec<(f64, f32, f32, String, Option<String>, Option<f64>)> = rows
        .iter()
        .map(|(z, t, reach)| {
            let l = sec.long.iter().find(|l| (l.z - *z).abs() < 1e-9);
            let extra = l.filter(|l| l.dep.abs() > 1e-9).map(|l| format!("Δεp = {:.2} mm/m", l.dep));
            (*z, map(0.0, *z).y, 0.0f32, t.clone(), extra, *reach)
        })
        .collect();
    placed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let mut next_free = f32::MIN;
    for r in placed.iter_mut() {
        let hgt = if r.4.is_some() { 2.0 * line_h } else { line_h };
        let top = (r.1 - line_h / 2.0).max(next_free);
        r.2 = top + line_h / 2.0;
        next_free = top + hgt;
    }
    for (z, y, ly, t, extra, reach) in &placed {
        let tick = map(sec.width_at(*z) / 2.0, 0.0).x + 12.0 * s;
        match reach {
            Some(x) => {
                p.line_segment([Pos2::new(map(*x, 0.0).x, *y), Pos2::new(lx1, *ly)], thin);
            }
            None => {
                p.line_segment([Pos2::new(tick, *y - 7.0 * s), Pos2::new(tick, *y + 7.0 * s)], thin);
                p.line_segment([Pos2::new(tick, *y), Pos2::new(lx1 - 6.0 * s, *ly)], thin);
                p.line_segment([Pos2::new(lx1 - 6.0 * s, *ly), Pos2::new(lx1, *ly)], thin);
            }
        }
        text(p, Pos2::new(lx1 + 4.0 * s, *ly), Align2::LEFT_CENTER, t, f, Color32::BLACK);
        if let Some(e) = extra {
            p.line_segment([Pos2::new(lx1, *ly - line_h / 2.0), Pos2::new(lx1, *ly + 1.5 * line_h)], thin);
            text(p, Pos2::new(lx1 + 4.0 * s, *ly + line_h), Align2::LEFT_CENTER, e, f, Color32::BLACK);
        }
    }
    let rows: Vec<(f64, String, Option<f64>)> = placed.iter().map(|r| (r.0, r.3.clone(), r.5)).collect();
    if let Some(st) = stir {
        let name = if st.bartitle.is_empty() { format!("{:.0} mm²", st.area) } else { st.bartitle.clone() };
        let label = format!("{} @  {:.0} mm", name, st.spacing);
        let (zt, zb) = (st.z_top.max(st.z_bot), st.z_top.min(st.z_bot));
        // the stirrup label goes in the middle of the largest free height between the bar
        // labels inside the stirrup (0x4e6d70); interlocking hoops: the top hoop's, and a second
        // label at the bottom hoop's middle
        let hoop_r = {
            let cover = (zb - st.db / 2.0).max(0.0);
            ((zt - zb) / 2.0).min(sec.width_at(0.5 * (zt + zb)) / 2.0 - cover - st.db / 2.0)
        };
        let (lo, hi) = if st.pattern == 6 { (zt - 2.0 * hoop_r, zt) } else { (zb, zt) };
        let mut cuts: Vec<f64> = rows.iter().map(|r| r.0).filter(|z| *z > lo && *z < hi).collect();
        cuts.push(lo);
        cuts.push(hi);
        cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut best = (-1.0, 0.5 * (lo + hi));
        for w in cuts.windows(2) {
            if w[1] - w[0] > best.0 {
                best = (w[1] - w[0], 0.5 * (w[0] + w[1]));
            }
        }
        let zl = best.1;
        let (sx, sy) = (map(stirrup_attach(sec, st, zl), 0.0).x, map(0.0, zl).y);
        p.line_segment([Pos2::new(sx, sy), Pos2::new(lx1, sy)], thin);
        if st.pattern == 5 {
            // as the original (0x4e6d70): "T-Headed" above the size and spacing, with a bar
            p.line_segment([Pos2::new(lx1, sy - 30.0 * s), Pos2::new(lx1, sy + 30.0 * s)], thin);
            text(p, Pos2::new(lx1 + 4.0 * s, sy - 2.0 * s), Align2::LEFT_BOTTOM, "T-Headed", f, Color32::BLACK);
            text(p, Pos2::new(lx1 + 4.0 * s, sy + 2.0 * s), Align2::LEFT_TOP, &label, f, Color32::BLACK);
        } else {
            text(p, Pos2::new(lx1 + 4.0 * s, sy), Align2::LEFT_CENTER, &label, f, Color32::BLACK);
        }
        if st.pattern == 6 {
            let z2 = zb + hoop_r;
            let at = map(stirrup_attach(sec, st, z2), z2);
            p.line_segment([at, Pos2::new(lx1, at.y)], thin);
            text(p, Pos2::new(lx1 + 4.0 * s, at.y), Align2::LEFT_CENTER, &label, f, Color32::BLACK);
        }
    }
    // height dimension on the left, its extension lines reaching the section's top and bottom
    let lx = map(-b / 2.0, 0.0).x - 55.0 * s;
    let (yt, yb) = (map(0.0, h).y, map(0.0, 0.0).y);
    p.line_segment([Pos2::new(lx, yt), Pos2::new(lx, yb)], thin);
    for (y, z) in [(yt, h), (yb, 0.0)] {
        let edge = map(-sec.width_at(if z > 0.0 { h * 0.9999 } else { h * 0.0001 }) / 2.0, 0.0).x;
        p.line_segment([Pos2::new(lx - 4.0, y), Pos2::new(edge - 6.0, y)], thin);
        p.circle_filled(Pos2::new(lx, y), 3.0, Color32::BLACK);
    }
    let g = p.layout_no_wrap(format!("{:.0}", h), FontId::proportional(f), Color32::BLACK);
    let gw = g.size().x;
    let gh = g.size().y;
    p.add(egui::epaint::TextShape::new(Pos2::new(lx - gh - 4.0, (yt + yb) / 2.0 + gw / 2.0), g, Color32::BLACK).with_angle(-std::f32::consts::FRAC_PI_2));
    // width dimension below (none for a circle, whose bottom is a point, as the original)
    let wb = sec.width_at(h * 0.0001);
    if wb > 1.0 {
        let dy = yb + 50.0 * s;
        let (x1, x2) = (map(-wb / 2.0, 0.0).x, map(wb / 2.0, 0.0).x);
        p.line_segment([Pos2::new(x1, dy), Pos2::new(x2, dy)], thin);
        for x in [x1, x2] {
            p.line_segment([Pos2::new(x, yb + 6.0), Pos2::new(x, dy + 4.0)], thin);
            p.circle_filled(Pos2::new(x, dy), 3.0, Color32::BLACK);
        }
        text(p, Pos2::new((x1 + x2) / 2.0, dy - 4.0), Align2::CENTER_BOTTOM, &format!("{:.0}", wb), f, Color32::BLACK);
        // a wider (or narrower) top gets its own dimension line above, as the original's T-beams
        let wt = sec.width_at(h * 0.9999);
        if (wt - wb).abs() > 1.0 && wt > 1.0 {
            let ty = yt - 50.0 * s;
            let (t1, t2) = (map(-wt / 2.0, h).x, map(wt / 2.0, h).x);
            p.line_segment([Pos2::new(t1, ty), Pos2::new(t2, ty)], thin);
            for x in [t1, t2] {
                p.line_segment([Pos2::new(x, yt - 6.0), Pos2::new(x, ty - 4.0)], thin);
                p.circle_filled(Pos2::new(x, ty), 3.0, Color32::BLACK);
            }
            text(p, Pos2::new((t1 + t2) / 2.0, ty - 4.0), Align2::CENTER_BOTTOM, &format!("{:.0}", wt), f, Color32::BLACK);
        }
    }
}
