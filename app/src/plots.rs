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
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 46.0, rect.top() + 22.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 30.0),
    );
    let (x0, x1) = xrange.unwrap_or_else(|| range(series, 0, true));
    let (y0, y1) = yrange.unwrap_or_else(|| range(series, 1, true));
    let fr = Frame { plot, x0, x1, y0, y1 };
    if plot.width() < 20.0 || plot.height() < 20.0 {
        return fr;
    }
    for t in nice_ticks(x0, x1, 5) {
        let p = fr.map(t, y0);
        painter.line_segment([Pos2::new(p.x, plot.top()), Pos2::new(p.x, plot.bottom())], Stroke::new(1.0, GRID));
        painter.text(Pos2::new(p.x, plot.bottom() + 2.0), Align2::CENTER_TOP, fmt_tick(t), font.clone(), AXIS);
    }
    for t in nice_ticks(y0, y1, 5) {
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
