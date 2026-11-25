//! Small hand-drawn plots (no plotting library).

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind};

pub const AXIS: Color32 = Color32::from_rgb(60, 60, 60);
pub const GRID: Color32 = Color32::from_rgb(225, 225, 225);
pub const BLUE: Color32 = Color32::from_rgb(20, 60, 200);
pub const RED: Color32 = Color32::from_rgb(200, 30, 30);
pub const GREEN: Color32 = Color32::from_rgb(20, 140, 60);
pub const GREY: Color32 = Color32::from_rgb(140, 140, 140);

pub struct Series {
    pub pts: Vec<[f64; 2]>,
    pub color: Color32,
    pub width: f32,
    pub filled_to_zero: bool,
    pub dots: bool,
}

impl Series {
    pub fn line(pts: Vec<[f64; 2]>, color: Color32) -> Self {
        Series { pts, color, width: 1.6, filled_to_zero: false, dots: false }
    }
}

pub fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return vec![lo];
    }
    let raw = (hi - lo) / n.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let start = (lo / step).ceil() * step;
    let mut v = vec![];
    let mut x = start;
    while x <= hi + step * 1e-6 {
        v.push(if x.abs() < step * 1e-9 { 0.0 } else { x });
        x += step;
    }
    v
}

pub fn fmt_tick(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if a >= 1000.0 {
        format!("{:.0}", v)
    } else if a >= 10.0 {
        format!("{:.0}", v)
    } else if a >= 1.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

pub struct Frame {
    pub plot: Rect,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

impl Frame {
    pub fn map(&self, x: f64, y: f64) -> Pos2 {
        let fx = if self.x1 > self.x0 { (x - self.x0) / (self.x1 - self.x0) } else { 0.5 };
        let fy = if self.y1 > self.y0 { (y - self.y0) / (self.y1 - self.y0) } else { 0.5 };
        Pos2::new(
            self.plot.left() + fx as f32 * self.plot.width(),
            self.plot.bottom() - fy as f32 * self.plot.height(),
        )
    }
}

fn range(series: &[Series], axis: usize, include_zero: bool) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in series {
        for p in &s.pts {
            if p[axis].is_finite() {
                lo = lo.min(p[axis]);
                hi = hi.max(p[axis]);
            }
        }
    }
    if include_zero {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        let d = if hi.abs() > 0.0 { hi.abs() * 0.5 } else { 1.0 };
        return (lo - d, hi + d);
    }
    let pad = 0.05 * (hi - lo);
    (if lo < 0.0 || !include_zero { lo - pad } else { lo }, hi + pad)
}

/// The area inside the axes of a plot drawn in `rect` (room left for the title, tick labels and
/// axis names).
fn plot_area(rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(rect.left() + 46.0, rect.top() + 22.0), Pos2::new(rect.right() - 10.0, rect.bottom() - 30.0))
}

/// Axis ranges with the same scale on both axes (a kNm across is as long as a kNm up), fitted to
/// the data and to the shape of `rect` (the rect that will be passed to `draw`). A capacity
/// diagram keeps its true shape and still fills whatever space the window gives it.
/// `reserve_top` keeps that many pixels clear above the data (for a legend in the corner).
pub fn equal_ranges(rect: Rect, series: &[Series], reserve_top: f32) -> ((f64, f64), (f64, f64)) {
    let (x0, x1) = range(series, 0, true);
    let (y0, y1) = range(series, 1, true);
    let plot = plot_area(rect);
    let top = reserve_top.min(plot.height() * 0.4) as f64;
    let (w, h) = (plot.width().max(1.0) as f64, (plot.height() as f64 - top).max(1.0));
    // units per pixel: the larger of the two, so all the data fits
    let s = ((x1 - x0) / w).max((y1 - y0) / h);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    ((cx - s * w / 2.0, cx + s * w / 2.0), (cy - s * h / 2.0, cy + s * (h / 2.0 + top)))
}

/// Width over height of the data in `series` (origin included, as the plot shows it).
pub fn data_aspect(series: &[Series]) -> f64 {
    let (x0, x1) = range(series, 0, true);
    let (y0, y1) = range(series, 1, true);
    ((x1 - x0) / (y1 - y0).max(1e-9)).clamp(0.05, 20.0)
}

/// A plot with axes. `yrange` fixes the vertical range (for profile plots over the height).
pub fn draw(
    painter: &egui::Painter,
    rect: Rect,
    title: &str,
    xlabel: &str,
    ylabel: &str,
    series: &[Series],
    yrange: Option<(f64, f64)>,
    xrange: Option<(f64, f64)>,
) -> Frame {
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let font = FontId::proportional(11.0);
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 2.0),
        Align2::CENTER_TOP,
        title,
        FontId::proportional(13.0),
        Color32::BLACK,
    );
    let plot = plot_area(rect);
    let (x0, x1) = xrange.unwrap_or_else(|| range(series, 0, true));
    let (y0, y1) = yrange.unwrap_or_else(|| range(series, 1, true));
    let fr = Frame { plot, x0, x1, y0, y1 };
    if plot.width() < 20.0 || plot.height() < 20.0 {
        return fr;
    }
    // more ticks on a bigger plot, so a plot scaled up to the window still reads finely
    let (nx, ny) = (((plot.width() / 110.0) as usize).clamp(4, 12), ((plot.height() / 55.0) as usize).clamp(4, 10));
    for t in nice_ticks(x0, x1, nx) {
        let p = fr.map(t, y0);
        painter.line_segment([Pos2::new(p.x, plot.top()), Pos2::new(p.x, plot.bottom())], Stroke::new(1.0, GRID));
        painter.text(Pos2::new(p.x, plot.bottom() + 2.0), Align2::CENTER_TOP, fmt_tick(t), font.clone(), AXIS);
    }
    for t in nice_ticks(y0, y1, ny) {
        let p = fr.map(x0, t);
        painter.line_segment([Pos2::new(plot.left(), p.y), Pos2::new(plot.right(), p.y)], Stroke::new(1.0, GRID));
        painter.text(Pos2::new(plot.left() - 3.0, p.y), Align2::RIGHT_CENTER, fmt_tick(t), font.clone(), AXIS);
    }
    if x0 < 0.0 && x1 > 0.0 {
        let p = fr.map(0.0, y0);
        painter.line_segment([Pos2::new(p.x, plot.top()), Pos2::new(p.x, plot.bottom())], Stroke::new(1.0, GREY));
    }
    if y0 < 0.0 && y1 > 0.0 {
        let p = fr.map(x0, 0.0);
        painter.line_segment([Pos2::new(plot.left(), p.y), Pos2::new(plot.right(), p.y)], Stroke::new(1.0, GREY));
    }
    painter.rect_stroke(plot, 0.0, Stroke::new(1.0, AXIS), StrokeKind::Inside);
    painter.text(
        Pos2::new(plot.center().x, rect.bottom() - 2.0),
        Align2::CENTER_BOTTOM,
        xlabel,
        font.clone(),
        AXIS,
    );
    if !ylabel.is_empty() {
        painter.text(Pos2::new(rect.left() + 2.0, plot.top() - 12.0), Align2::LEFT_TOP, ylabel, font, AXIS);
    }
    let clip = painter.with_clip_rect(plot.expand(1.0));
    for s in series {
        if s.pts.len() >= 2 {
            if s.filled_to_zero {
                for w in s.pts.windows(2) {
                    let a = fr.map(w[0][0], w[0][1]);
                    let b = fr.map(w[1][0], w[1][1]);
                    let z = fr.map(0.0, 0.0).x;
                    let poly = vec![a, b, Pos2::new(z, b.y), Pos2::new(z, a.y)];
                    clip.add(egui::Shape::convex_polygon(
                        poly,
                        s.color.gamma_multiply(0.25),
                        Stroke::NONE,
                    ));
                }
            }
            // a non-finite point is a gap: the line is drawn in pieces between gaps
            for piece in s.pts.split(|p| !(p[0].is_finite() && p[1].is_finite())) {
                if piece.len() >= 2 {
                    let pts: Vec<Pos2> = piece.iter().map(|p| fr.map(p[0], p[1])).collect();
                    clip.add(egui::Shape::line(pts, Stroke::new(s.width, s.color)));
                }
            }
        }
        if s.dots || s.pts.len() == 1 {
            for p in s.pts.iter().filter(|p| p[0].is_finite() && p[1].is_finite()) {
                clip.circle_filled(fr.map(p[0], p[1]), 3.0, s.color);
            }
        }
    }
    fr
}

/// A named curve whose values can be read off the plot (see `readout`).
pub struct Probe<'a> {
    pub name: &'a str,
    pub pts: &'a [[f64; 2]],
    pub color: Color32,
}

/// What a readout found on a curve: the value, the curve, and whether it is a computed point
/// (exact) or a linear interpolation between two neighbouring computed points.
#[derive(Clone)]
struct Hit {
    x: f64,
    y: f64,
    name: String,
    color: Color32,
    exact: bool,
}

/// How close (screen pixels) the pointer must be to a line to read it, and to a computed point
/// to snap to it. Generous enough for a finger on a touch screen.
const PICK_PX: f32 = 16.0;
const SNAP_PX: f32 = 7.0;

fn nearest(fr: &Frame, curves: &[Probe], at: Pos2) -> Option<Hit> {
    let mut best: Option<(f32, Hit)> = None;
    let ok = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
    for c in curves {
        let mut consider = |d: f32, hit: Hit| {
            if d <= PICK_PX && best.as_ref().map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, hit));
            }
        };
        if c.pts.len() == 1 && ok(&c.pts[0]) {
            let p = c.pts[0];
            consider(fr.map(p[0], p[1]).distance(at), Hit { x: p[0], y: p[1], name: c.name.into(), color: c.color, exact: true });
        }
        for w in c.pts.windows(2) {
            if !ok(&w[0]) || !ok(&w[1]) {
                continue;
            }
            let (a, b) = (fr.map(w[0][0], w[0][1]), fr.map(w[1][0], w[1][1]));
            let ab = b - a;
            let len = ab.length();
            let mut t = if len > 1e-6 { ((at - a).dot(ab) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
            // snap to a computed point when the pointer is on it
            if t * len <= SNAP_PX {
                t = 0.0;
            } else if (1.0 - t) * len <= SNAP_PX {
                t = 1.0;
            }
            let d = (a + ab * t).distance(at);
            // the axes are linear, so the fraction along the segment on screen is the same as in values
            let tt = t as f64;
            let (x, y) = (w[0][0] + tt * (w[1][0] - w[0][0]), w[0][1] + tt * (w[1][1] - w[0][1]));
            consider(d, Hit { x, y, name: c.name.into(), color: c.color, exact: t == 0.0 || t == 1.0 });
        }
    }
    best.map(|b| b.1)
}

/// Reads values off the curves: hovering (or touching) near a line shows the value on it,
/// exact on a computed point and linearly interpolated between two of them; a click or tap pins
/// the readout so it stays (a click away from the lines removes it). `id` keeps each plot's pin
/// apart; `extra` adds a line such as the resultant and its angle.
pub fn readout(
    ui: &egui::Ui,
    painter: &egui::Painter,
    fr: &Frame,
    id: &str,
    curves: &[Probe],
    x: (&str, &str),
    y: (&str, &str),
    extra: Option<&dyn Fn(f64, f64) -> String>,
) {
    let key = egui::Id::new(("plot_readout", id));
    let over = ui.rect_contains_pointer(fr.plot);
    let pos = if over { ui.input(|i| i.pointer.latest_pos()) } else { None };
    let hover = pos.and_then(|p| nearest(fr, curves, p));
    if over && ui.input(|i| i.pointer.primary_clicked()) {
        ui.ctx().data_mut(|d| match &hover {
            Some(h) => {
                d.insert_temp(key, h.clone());
            }
            None => d.remove::<Hit>(key),
        });
    }
    let pinned: Option<Hit> = ui.ctx().data(|d| d.get_temp(key));
    let clip = painter.with_clip_rect(fr.plot.expand(2.0));
    let draw = |h: &Hit, pinned: bool| {
        let at = fr.map(h.x, h.y);
        if !fr.plot.expand(2.0).contains(at) {
            return;
        }
        clip.circle_filled(at, 5.0, Color32::WHITE);
        clip.circle_stroke(at, 5.0, Stroke::new(2.0, h.color));
        if pinned {
            clip.circle_filled(at, 2.0, h.color);
        }
        let mut lines = vec![
            h.name.clone(),
            format!("{} = {:.1} {}", x.0, h.x, x.1),
            format!("{} = {:.1} {}", y.0, h.y, y.1),
        ];
        if let Some(f) = extra {
            lines.push(f(h.x, h.y));
        }
        lines.push(if h.exact { "computed point".into() } else { "interpolated between computed points".into() });
        let text = lines.join("\n");
        let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), Color32::BLACK);
        let size = galley.size() + egui::Vec2::splat(12.0);
        // beside the point, kept inside the plot
        let mut min = at + egui::Vec2::new(12.0, -size.y - 12.0);
        if min.x + size.x > fr.plot.right() {
            min.x = at.x - 12.0 - size.x;
        }
        if min.y < fr.plot.top() {
            min.y = at.y + 12.0;
        }
        let r = Rect::from_min_size(min, size);
        painter.rect(r, 3.0, Color32::from_white_alpha(240), Stroke::new(1.0, h.color), StrokeKind::Inside);
        painter.galley(r.min + egui::Vec2::splat(6.0), galley, Color32::BLACK);
    };
    if let Some(p) = &pinned {
        draw(p, true);
    }
    if let Some(h) = &hover {
        let same = pinned.as_ref().map(|p| p.x == h.x && p.y == h.y).unwrap_or(false);
        if !same {
            draw(h, false);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a 100 x 100 pixel plot from (0, 0) to (10, 10): 10 pixels per unit on both axes
    fn frame() -> Frame {
        Frame { plot: Rect::from_min_size(Pos2::new(0.0, 0.0), egui::Vec2::splat(100.0)), x0: 0.0, x1: 10.0, y0: 0.0, y1: 10.0 }
    }

    #[test]
    fn readout_snaps_interpolates_and_ignores_far_pointers() {
        let fr = frame();
        let pts = [[0.0, 0.0], [10.0, 10.0], [10.0, f64::NAN], [0.0, 10.0]];
        let c = [Probe { name: "line", pts: &pts, color: BLUE }];
        // on the line halfway: interpolated (5, 5)
        let h = nearest(&fr, &c, fr.map(5.0, 5.0)).unwrap();
        assert!(!h.exact && (h.x - 5.0).abs() < 1e-6 && (h.y - 5.0).abs() < 1e-6);
        // a little off the line at 3.0: the foot of the perpendicular, interpolated
        let h = nearest(&fr, &c, fr.map(3.0, 3.0) + egui::Vec2::new(5.0, 5.0)).unwrap();
        assert!(!h.exact && (h.x - h.y).abs() < 1e-6 && (h.x - 3.0).abs() < 1e-6);
        // within a few pixels of a computed point: snaps to it exactly
        let h = nearest(&fr, &c, fr.map(9.6, 9.6)).unwrap();
        assert!(h.exact && h.x == 10.0 && h.y == 10.0);
        // far from every line: nothing
        assert!(nearest(&fr, &c, fr.map(8.0, 2.0)).is_none());
        // the gap (NaN) is not a line: no hit between (10, 10) and (0, 10) along the top
        assert!(nearest(&fr, &c, fr.map(5.0, 10.0)).is_none());
    }
}
