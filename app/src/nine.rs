//! The nine sectional-response plots, drawn the way the original program draws them: big titles,
//! plain line profiles over the depth with "top" and "bot" beside the zero axis, the smallest
//! and largest values written on a line across the middle, red curves for the limits, and a
//! cross-section showing the compression zone and each bar's state. Plus the two "control"
//! plots (load against deformation) with the current stage marked by dashed lines.

use crate::analysis::{LayerState, SModel, SectionState};
use crate::materials::SteelProps;
use crate::model::Section;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};

pub const LINE_BLUE: Color32 = Color32::from_rgb(20, 40, 230);
pub const LIMIT_RED: Color32 = Color32::from_rgb(235, 20, 20);
pub const GREEN: Color32 = Color32::from_rgb(0, 170, 0);

/// A curve over the depth: (value, height above the bottom in mm).
pub struct Curve {
    pub pts: Vec<(f64, f64)>,
    pub color: Color32,
}

fn title_size(cell: Rect) -> f32 {
    (cell.height() * 0.085).clamp(13.0, 30.0)
}

fn title(p: &egui::Painter, cell: Rect, t: &str) -> f32 {
    let s = title_size(cell);
    p.text(Pos2::new(cell.center().x, cell.top() + 2.0), Align2::CENTER_TOP, t, FontId::proportional(s), Color32::BLACK);
    // a bold look, as the original's titles: draw it again a hair to the right
    p.text(Pos2::new(cell.center().x + 0.6, cell.top() + 2.0), Align2::CENTER_TOP, t, FontId::proportional(s), Color32::BLACK);
    s
}

/// Three significant figures, as the original writes its axis values (-1.44, 8.15, -40.0).
pub fn sig3(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let mag = v.abs().log10().floor() as i32;
    let dec = (2 - mag).clamp(0, 4) as usize;
    format!("{:.*}", dec, v)
}

/// A depth profile in the original's style.
pub fn profile(p: &egui::Painter, cell: Rect, name: &str, curves: &[Curve], h: f64) {
    let ts = title(p, cell, name);
    let inner = Rect::from_min_max(Pos2::new(cell.left() + 10.0, cell.top() + ts * 1.45), Pos2::new(cell.right() - 10.0, cell.bottom() - 6.0));
    let vals = curves.iter().flat_map(|c| c.pts.iter().map(|q| q.0)).filter(|v| v.is_finite());
    let (mut lo, mut hi) = vals.fold((0.0f64, 0.0f64), |(a, b), v| (a.min(v), b.max(v)));
    if hi - lo < 1e-9 {
        hi = lo + 1.0;
    }
    let (dlo, dhi) = (lo, hi);
    let pad = 0.02 * (hi - lo);
    lo -= pad;
    hi += pad;
    let mx = |v: f64| inner.left() + ((v - lo) / (hi - lo)) as f32 * inner.width();
    let my = |z: f64| inner.bottom() - (z / h.max(1e-9)) as f32 * inner.height();
    let black = Stroke::new(1.0, Color32::BLACK);
    let x0 = mx(0.0);
    // zero axis over the depth, and the line across the middle with the extreme values
    p.line_segment([Pos2::new(x0, inner.top()), Pos2::new(x0, inner.bottom())], black);
    let ym = inner.center().y;
    p.line_segment([Pos2::new(inner.left(), ym), Pos2::new(inner.right(), ym)], black);
    let vs = ts * 0.92;
    // a side with only a trace of the other sign gets no value (no "-0.0000")
    let tiny = 0.01 * dlo.abs().max(dhi.abs());
    if dlo < -tiny {
        p.text(Pos2::new(inner.left(), ym - 2.0), Align2::LEFT_BOTTOM, sig3(dlo), FontId::proportional(vs), Color32::BLACK);
    }
    if dhi > tiny {
        p.text(Pos2::new(inner.right(), ym - 2.0), Align2::RIGHT_BOTTOM, sig3(dhi), FontId::proportional(vs), Color32::BLACK);
    }
    // "top" and "bot" beside the axis, on the side with room
    let right_side = x0 < inner.center().x + inner.width() * 0.3;
    let (ax, al_t, al_b) = if right_side {
        (x0 + 3.0, Align2::LEFT_TOP, Align2::LEFT_BOTTOM)
    } else {
        (x0 - 3.0, Align2::RIGHT_TOP, Align2::RIGHT_BOTTOM)
    };
    p.text(Pos2::new(ax, inner.top()), al_t, "top", FontId::proportional(vs), Color32::BLACK);
    p.text(Pos2::new(ax, inner.bottom()), al_b, "bot", FontId::proportional(vs), Color32::BLACK);
    let clip = p.with_clip_rect(inner.expand(2.0));
    for c in curves {
        let pts: Vec<Pos2> = c.pts.iter().filter(|q| q.0.is_finite()).map(|q| Pos2::new(mx(q.0), my(q.1))).collect();
        if pts.len() >= 2 {
            clip.add(egui::Shape::line(pts, Stroke::new(1.3, c.color)));
        }
    }
}

/// How a bar is doing: elastic (white), yielded in tension (red) or compression (blue),
/// strain-hardening (dark red / dark blue).
fn bar_colour(steel: &SteelProps, e: f64) -> Color32 {
    let ey = steel.fy() / steel.modulus();
    let esh = match steel {
        SteelProps::Rebar { esh, .. } => *esh,
        SteelProps::Prestress { eu, .. } => *eu,
    };
    if e.abs() < 0.98 * ey {
        Color32::WHITE
    } else if e > 0.0 {
        if e >= esh { Color32::from_rgb(140, 0, 0) } else { Color32::from_rgb(230, 0, 0) }
    } else if -e >= esh {
        Color32::from_rgb(0, 0, 140)
    } else {
        Color32::from_rgb(40, 80, 240)
    }
}

/// The cross-section at this stage: concrete in compression drawn darker, the stirrup with its
/// hooks, and each bar coloured by its state (as the original shows yielded bars in red).
pub fn section_state(p: &egui::Painter, cell: Rect, sec: &Section, st: &SectionState, m: &SModel) {
    let ts = title(p, cell, "Cross Section");
    let area = Rect::from_min_max(Pos2::new(cell.left() + 8.0, cell.top() + ts * 1.5), Pos2::new(cell.right() - 8.0, cell.bottom() - 6.0));
    let h = sec.height();
    let b = sec.max_width();
    if h <= 0.0 || b <= 0.0 {
        return;
    }
    let s = (area.width() as f64 / b).min(area.height() as f64 / h);
    let cx = area.center().x as f64;
    let base = area.center().y as f64 + h * s / 2.0;
    let map = |y: f64, z: f64| Pos2::new((cx + y * s) as f32, (base - z * s) as f32);
    // the strain at a height (from the layer nearest to it)
    let ex_at = |z: f64| -> f64 {
        st.layers
            .iter()
            .min_by(|a, c| (m.display_z(a.z) - z).abs().partial_cmp(&(m.display_z(c.z) - z).abs()).unwrap())
            .map(|l| l.ex)
            .unwrap_or(0.0)
    };
    let light = Color32::from_rgb(200, 200, 200);
    let dark = Color32::from_rgb(120, 120, 120);
    let n = 200;
    for i in 0..n {
        let z0 = h * i as f64 / n as f64;
        let z1 = h * (i + 1) as f64 / n as f64;
        let zm = 0.5 * (z0 + z1);
        let w = sec.width_at(zm);
        if w > 0.0 {
            let col = if ex_at(zm) < 0.0 { dark } else { light };
            let r = Rect::from_min_max(map(-w / 2.0, z1), map(w / 2.0, z0)).expand2(Vec2::new(0.0, 0.6));
            p.rect_filled(r, 0.0, col);
        }
    }
    let mut ring: Vec<Pos2> = sec.outline.iter().map(|(z, w)| map(w / 2.0, *z)).collect();
    ring.extend(sec.outline.iter().rev().map(|(z, w)| map(-w / 2.0, *z)));
    p.add(egui::Shape::closed_line(ring, Stroke::new(1.2, Color32::from_gray(60))));
    // stirrup and bars detailed and placed exactly as on the cross-section page (both faces of
    // the stirrup, bends of the real bend diameter, 135-degree hooks with their tails, bars
    // inside the stirrup's corners), then every bar filled with its state colour
    let _ = crate::section_page::draw_steel(p, sec, &map, s);
    for (k, l) in sec.long.iter().enumerate() {
        let e = st.bars.get(k).map(|b| b.1).unwrap_or(0.0);
        let col = m.bars.get(k).map(|mb| bar_colour(&mb.steel, e)).unwrap_or(Color32::WHITE);
        let r = ((l.draw_dia().max(1.0) / 2.0) * s).max(2.5) as f32;
        for (y, z) in crate::section_page::bar_draw_points(sec, l) {
            let c = map(y, z);
            p.circle_filled(c, r, col);
            p.circle_stroke(c, r, Stroke::new(1.0, Color32::BLACK));
        }
    }
}

/// Crack pattern: cracks run parallel to the compression direction in each cracked layer, so
/// each crack is traced up from the bottom (or the top, for negative moment) layer by layer;
/// crack widths (mm) are written along the middle crack, as the original shows them.
pub fn crack_diagram(p: &egui::Painter, cell: Rect, st: &SectionState, m: &SModel, h: f64) {
    let ts = title(p, cell, "Crack Diagram");
    let area = Rect::from_min_max(Pos2::new(cell.left() + 8.0, cell.top() + ts * 1.45), Pos2::new(cell.right() - 8.0, cell.bottom() - 4.0));
    p.rect_filled(area, 0.0, Color32::from_rgb(190, 190, 190));
    p.rect_stroke(area, 0.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
    let mut layers: Vec<(f64, &LayerState)> = st.layers.iter().map(|l| (m.display_z(l.z), l)).collect();
    layers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    if layers.is_empty() {
        return;
    }
    let ppm = area.height() as f64 / h.max(1e-9); // pixels per mm, both ways
    let span = area.width() as f64 / ppm; // mm of member shown
    let dz = h / layers.len() as f64;
    // cracks start where the most strained (tension) face is
    let from_bottom = layers.first().map(|l| l.1.ex).unwrap_or(0.0) >= layers.last().map(|l| l.1.ex).unwrap_or(0.0);
    let order: Vec<&(f64, &LayerState)> = if from_bottom { layers.iter().collect() } else { layers.iter().rev().collect() };
    let spacing = (0.45 * h).max(span / 6.0);
    let clip = p.with_clip_rect(area);
    let ncr = (span / spacing).ceil() as i32 + 2;
    // widths are written along the crack that starts nearest a third of the way across, so
    // the whole crack (leaning right as it rises) stays inside the picture
    let mid = ((span / 3.0) / spacing).round() as i32;
    for c in -1..ncr {
        let mut x = c as f64 * spacing;
        let mut pts: Vec<Pos2> = vec![];
        let mut labels: Vec<(Pos2, f64)> = vec![];
        for (k, (z, l)) in order.iter().enumerate() {
            if !l.cracked {
                if !pts.is_empty() {
                    break;
                }
                continue;
            }
            // crack parallel to the principal compression: rise dz, run dz / tan(theta)
            let th = l.theta.abs().clamp(0.15, std::f64::consts::FRAC_PI_2);
            let run = if th > 1.55 { 0.0 } else { dz / th.tan() };
            let zz = if from_bottom { *z - dz / 2.0 } else { *z + dz / 2.0 };
            if pts.is_empty() {
                pts.push(Pos2::new(area.left() + (x * ppm) as f32, area.bottom() - (zz * ppm) as f32));
            }
            x += run;
            let z_end = if from_bottom { *z + dz / 2.0 } else { *z - dz / 2.0 };
            let q = Pos2::new(area.left() + (x * ppm) as f32, area.bottom() - (z_end * ppm) as f32);
            pts.push(q);
            if c == mid && k % (order.len() / 6).max(1) == 0 && l.w > 0.0 {
                labels.push((q, l.w));
            }
        }
        if pts.len() >= 2 {
            clip.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_gray(25))));
        }
        let fs = ts * 0.92;
        for (q, w) in labels.iter().take(6) {
            clip.text(Pos2::new(q.x - 6.0, q.y), Align2::RIGHT_CENTER, format!("{:.2}", w), FontId::proportional(fs), Color32::BLACK);
        }
    }
}

/// A control plot: load against deformation over the whole run, the current stage marked by
/// dashed green lines, the largest load written at the top left and the largest deformation at
/// the bottom right.
pub fn control(p: &egui::Painter, cell: Rect, name: &str, pts: &[(f64, f64)], cur: Option<(f64, f64)>) {
    p.rect_filled(cell, 0.0, Color32::from_rgb(240, 240, 240));
    p.rect_stroke(cell, 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
    let ts = title(p, cell, name);
    let inner = Rect::from_min_max(Pos2::new(cell.left() + 22.0, cell.top() + ts * 1.7), Pos2::new(cell.right() - 16.0, cell.bottom() - 18.0));
    let xmax = pts.iter().map(|q| q.0).fold(0.0, f64::max).max(1e-9);
    let ymax = pts.iter().map(|q| q.1).fold(0.0, f64::max).max(1e-9);
    let xmin = pts.iter().map(|q| q.0).fold(0.0, f64::min);
    let mx = |v: f64| inner.left() + ((v - xmin) / (xmax - xmin).max(1e-9)) as f32 * inner.width();
    let my = |v: f64| inner.bottom() - (v / ymax) as f32 * inner.height();
    let black = Stroke::new(1.0, Color32::BLACK);
    p.line_segment([Pos2::new(inner.left(), inner.top()), Pos2::new(inner.left(), inner.bottom())], black);
    p.line_segment([Pos2::new(inner.left(), inner.bottom()), Pos2::new(inner.right(), inner.bottom())], black);
    let vs = ts * 0.85;
    p.text(Pos2::new(inner.left() + 2.0, inner.top() + 2.0), Align2::LEFT_TOP, sig3(ymax), FontId::proportional(vs), Color32::BLACK);
    p.text(Pos2::new(inner.right(), inner.bottom() - 2.0), Align2::RIGHT_BOTTOM, sig3(xmax), FontId::proportional(vs), Color32::BLACK);
    let line: Vec<Pos2> = pts.iter().map(|q| Pos2::new(mx(q.0), my(q.1))).collect();
    if line.len() >= 2 {
        p.add(egui::Shape::line(line, Stroke::new(1.2, LINE_BLUE)));
    }
    if let Some((x, y)) = cur {
        let dash = Stroke::new(1.4, GREEN);
        let (cx, cy) = (mx(x), my(y));
        p.extend(egui::Shape::dashed_line(&[Pos2::new(cell.left() + 2.0, cy), Pos2::new(cell.right() - 2.0, cy)], dash, 9.0, 6.0));
        p.extend(egui::Shape::dashed_line(&[Pos2::new(cx, inner.top() - 6.0), Pos2::new(cx, cell.bottom() - 2.0)], dash, 9.0, 6.0));
    }
}
