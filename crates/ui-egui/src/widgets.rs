//! Small shared widgets drawn with the theme tokens.

use egui::{Align2, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};

/// Names a hand-drawn clickable widget for screen readers (and UI tests).
pub fn a11y(resp: &Response, label: impl Into<String>) {
    let label = label.into();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
}

/// A square, borderless icon button with a hover background and a tooltip.
pub fn icon_button(ui: &mut Ui, g: Glyph, tip: &str, t: &Tokens) -> Response {
    icon_button_sized(ui, g, tip, 26.0, false, t)
}

pub fn icon_button_sized(ui: &mut Ui, g: Glyph, tip: &str, size: f32, active: bool, t: &Tokens) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let bg = if active {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, bg);
    let col = if active || resp.hovered() { t.text } else { t.text_dim };
    icons::glyph(ui.painter(), rect.center(), size * 0.55, g, col);
    a11y(&resp, tip);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// A filled call-to-action button.
pub fn primary_button(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::bold(13.0), t.accent_text);
    let size = vec2(galley.size().x + 24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        t.accent.gamma_multiply(0.85)
    } else if resp.hovered() {
        t.accent.gamma_multiply(0.93)
    } else {
        t.accent
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.accent_text);
    a11y(&resp, text);
    resp
}

/// An outlined secondary button.
pub fn secondary_button(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::body(13.0), t.text);
    let size = vec2(galley.size().x + 24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect(rect, 7.0, if resp.hovered() { t.hover } else { t.surface }, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.text);
    a11y(&resp, text);
    resp
}

/// A danger button (delete).
pub fn danger_button(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::bold(13.0), Color32::WHITE);
    let size = vec2(galley.size().x + 24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_filled(rect, 7.0, if resp.hovered() { t.danger.gamma_multiply(0.9) } else { t.danger });
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    a11y(&resp, text);
    resp
}

/// A rounded label with a coloured dot.
pub fn label_chip(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let font = theme::body(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, t.text_dim);
    let size = vec2(galley.size().x + 22.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(rect, 10.0, t.surface, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    ui.painter().circle_filled(pos2(rect.left() + 9.0, rect.center().y), 3.5, t.tag(text));
    ui.painter().galley(pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0), galley, t.text_dim);
    resp
}

/// The space a label chip takes, for laying out right-aligned rows.
pub fn label_chip_width(ui: &Ui, text: &str) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), theme::body(11.5), Color32::WHITE).size().x + 22.0
}

/// A property "pill" button used in the detail pane and dialogs: icon, text and a hover frame.
pub fn pill(ui: &mut Ui, paint_icon: impl FnOnce(&egui::Painter, egui::Pos2), text: &str, dim: bool, t: &Tokens) -> Response {
    pill_with(ui, paint_icon, text, dim, false, t)
}

/// [`pill`] with a border, for forms where the pill has no row label next to it.
pub fn pill_outlined(ui: &mut Ui, paint_icon: impl FnOnce(&egui::Painter, egui::Pos2), text: &str, dim: bool, t: &Tokens) -> Response {
    pill_with(ui, paint_icon, text, dim, true, t)
}

fn pill_with(ui: &mut Ui, paint_icon: impl FnOnce(&egui::Painter, egui::Pos2), text: &str, dim: bool, outline: bool, t: &Tokens) -> Response {
    let font = theme::body(13.0);
    let col = if dim { t.text_faint } else { t.text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, col);
    let size = vec2(galley.size().x + 36.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if outline {
        ui.painter().rect(rect, 7.0, if resp.hovered() { t.hover } else { t.surface }, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    paint_icon(ui.painter(), pos2(rect.left() + 14.0, rect.center().y));
    ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0), galley, col);
    a11y(&resp, text);
    resp
}

/// One row in a dropdown menu: icon, label, and a check mark when chosen.
pub fn menu_row(ui: &mut Ui, paint_icon: impl FnOnce(&egui::Painter, egui::Pos2), text: &str, checked: bool, t: &Tokens) -> Response {
    let w = ui.available_width().max(180.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    }
    paint_icon(ui.painter(), pos2(rect.left() + 14.0, rect.center().y));
    ui.painter().text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, text, theme::body(13.0), t.text);
    if checked {
        icons::glyph(ui.painter(), pos2(rect.right() - 14.0, rect.center().y), 12.0, Glyph::Check, t.accent);
    }
    a11y(&resp, text);
    resp
}

/// A two-or-more option toggle with one selected.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, options: &[(T, Glyph, &str)], current: T, t: &Tokens) -> Option<T> {
    let mut chosen = None;
    let seg_w = 30.0;
    let (rect, _) = ui.allocate_exact_size(vec2(seg_w * options.len() as f32 + 4.0, 28.0), Sense::hover());
    ui.painter().rect(rect, 7.0, t.sidebar, Stroke::new(1.0, t.border), StrokeKind::Inside);
    for (i, (value, g, tip)) in options.iter().enumerate() {
        let r = egui::Rect::from_min_size(pos2(rect.left() + 2.0 + i as f32 * seg_w, rect.top() + 2.0), vec2(seg_w, 24.0));
        let resp = ui.interact(r, ui.id().with(("seg", i)), Sense::click()).on_hover_text(*tip);
        a11y(&resp, *tip);
        let on = *value == current;
        if on {
            ui.painter().rect(r, 5.0, t.surface, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 5.0, t.hover);
        }
        icons::glyph(ui.painter(), r.center(), 14.0, *g, if on { t.text } else { t.text_dim });
        if resp.clicked() && !on {
            chosen = Some(*value);
        }
    }
    chosen
}

/// A keyboard shortcut hint like `⌘N`.
pub fn kbd(ui: &mut Ui, text: &str, t: &Tokens) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::body(11.0), t.text_faint);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 10.0, 18.0), Sense::hover());
    ui.painter().rect(rect, 4.0, Color32::TRANSPARENT, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.text_faint);
}

/// A thin progress bar.
pub fn progress(ui: &mut Ui, width: f32, done: usize, total: usize, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, t.border);
    if total > 0 && done > 0 {
        let f = (done as f32 / total as f32).clamp(0.0, 1.0);
        let r = egui::Rect::from_min_size(rect.min, vec2(rect.width() * f, rect.height()));
        ui.painter().rect_filled(r, 3.0, t.positive);
    }
}

/// A small, round progress indicator for sidebars.
pub fn progress_ring(p: &egui::Painter, c: egui::Pos2, r: f32, done: usize, total: usize, t: &Tokens) {
    p.circle_stroke(c, r, Stroke::new(2.0, t.border_strong));
    if total == 0 || done == 0 {
        return;
    }
    let f = (done as f32 / total as f32).clamp(0.0, 1.0);
    let start = -std::f32::consts::FRAC_PI_2;
    let n = (f * 32.0).ceil().max(2.0) as usize;
    let pts: Vec<egui::Pos2> =
        (0..=n).map(|i| start + f * std::f32::consts::TAU * i as f32 / n as f32).map(|a| egui::pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, t.positive)));
}

/// The frame for popup menus.
pub fn popup_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border_strong))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(4))
        .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow })
}

/// Text cut to fit `max` points wide, with an ellipsis.
pub fn elide(ui: &Ui, text: &str, font: egui::FontId, max: f32) -> String {
    let fits = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x <= max;
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars.iter().take(mid).collect::<String>() + "…";
        if fits(&s) { lo = mid } else { hi = mid - 1 }
    }
    chars.iter().take(lo).collect::<String>().trim_end().to_string() + "…"
}

/// An amount right-aligned in `rect`'s row at `x_right`: green for money in when `colour` is set, the text colour
/// otherwise, dimmed when zero.
pub fn money(p: &egui::Painter, x_right: f32, y: f32, m: pecunia_engine::model::Money, colour: bool, size: f32, t: &Tokens) -> f32 {
    let col = if m.is_zero() {
        t.text_faint
    } else if colour && m.is_positive() {
        t.positive
    } else {
        t.text
    };
    let g = p.layout_no_wrap(m.format(), theme::mono_digits(size), col);
    let w = g.size().x;
    p.galley(pos2(x_right - w, y - g.size().y / 2.0), g, col);
    w
}

/// A thin progress bar from 0 to 1, in `col`.
pub fn bar(ui: &mut Ui, width: f32, fraction: f32, col: Color32, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, t.border);
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    if f > 0.0 {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, vec2((rect.width() * f).max(3.0), rect.height())), 3.0, col);
    }
}

/// A section heading in small capitals, like the sidebar's.
pub fn section(ui: &mut Ui, title: &str, t: &Tokens) {
    ui.label(egui::RichText::new(title.to_uppercase()).font(theme::bold(11.0)).color(t.text_faint));
}

/// A card: a surface with a border, for tiles on the Overview and the reports.
pub fn card(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.surface).stroke(Stroke::new(1.0, t.border)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(16))
}

/// A small "DEMO" badge, for a made-up book.
pub fn demo_badge(ui: &mut Ui, size: f32, t: &Tokens) -> Response {
    let g = ui.painter().layout_no_wrap("DEMO".into(), theme::bold(size * 0.85), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + size, size * 1.6), Sense::hover());
    ui.painter().rect_filled(rect, size * 0.4, t.warning);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
    a11y(&resp, "Demo book");
    resp.on_hover_text("A made-up household to try Pecunia")
}
