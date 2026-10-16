//! Every icon is drawn in code from simple shapes: our own artwork, no files, sharp at any scale.

use std::f32::consts::{PI, TAU};

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use pecunia_engine::model::{AccountKind, Group, ReviewKind, TxnStatus};

use crate::theme::Tokens;

fn ring(c: Pos2, r: f32, from: f32, to: f32, n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| from + (to - from) * i as f32 / n as f32).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

fn check(p: &Painter, c: Pos2, s: f32, stroke: Stroke) {
    p.add(Shape::line(vec![pos2(c.x - 0.42 * s, c.y + 0.02 * s), pos2(c.x - 0.12 * s, c.y + 0.32 * s), pos2(c.x + 0.45 * s, c.y - 0.3 * s)], stroke));
}

fn cross(p: &Painter, c: Pos2, s: f32, stroke: Stroke) {
    p.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], stroke);
    p.line_segment([pos2(c.x - s, c.y + s), pos2(c.x + s, c.y - s)], stroke);
}

/// The mark of an account: a rounded tile in its group's colour with a white sign for its kind (bank, piggy bank,
/// wallet, house, card, loan, pot, category). `size` is the full width.
pub fn account(p: &Painter, c: Pos2, size: f32, kind: AccountKind, group: Group, t: &Tokens) {
    let col = t.group(group);
    let h = size / 2.0;
    let white = Stroke::new((size / 10.0).max(1.1), Color32::WHITE);
    match kind {
        AccountKind::Pot => {
            p.circle_stroke(c, h - 1.0, Stroke::new((size / 8.0).max(1.4), col));
            p.circle_filled(c, h * 0.38, col);
            return;
        }
        AccountKind::Category | AccountKind::CategoryGroup => {
            p.circle_filled(c, h * if kind == AccountKind::Category { 0.62 } else { 0.8 }, col);
            return;
        }
        AccountKind::System => {
            p.circle_stroke(c, h * 0.7, Stroke::new(1.4, col));
            return;
        }
        _ => {}
    }
    p.rect_filled(Rect::from_center_size(c, vec2(size, size)), size * 0.24, col);
    let s = size * 0.3;
    match kind {
        AccountKind::Current => {
            // A bank: a roof and three pillars.
            p.add(Shape::line(vec![pos2(c.x - s, c.y - s * 0.35), pos2(c.x, c.y - s * 1.0), pos2(c.x + s, c.y - s * 0.35)], white));
            for dx in [-0.65f32, 0.0, 0.65] {
                p.line_segment([pos2(c.x + dx * s, c.y - s * 0.15), pos2(c.x + dx * s, c.y + s * 0.6)], white);
            }
            p.line_segment([pos2(c.x - s, c.y + s * 0.9), pos2(c.x + s, c.y + s * 0.9)], white);
        }
        AccountKind::Savings => {
            // A coin with a slot above it.
            p.circle_stroke(pos2(c.x, c.y + s * 0.25), s * 0.7, white);
            p.line_segment([pos2(c.x - s * 0.45, c.y - s * 0.9), pos2(c.x + s * 0.45, c.y - s * 0.9)], white);
        }
        AccountKind::Cash => {
            let r = Rect::from_center_size(c, vec2(s * 2.1, s * 1.3));
            p.rect_stroke(r, 1.5, white, egui::StrokeKind::Middle);
            p.circle_stroke(c, s * 0.35, white);
        }
        AccountKind::Property => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s, c.y),
                    pos2(c.x, c.y - s),
                    pos2(c.x + s, c.y),
                    pos2(c.x + s * 0.75, c.y),
                    pos2(c.x + s * 0.75, c.y + s * 0.9),
                    pos2(c.x - s * 0.75, c.y + s * 0.9),
                    pos2(c.x - s * 0.75, c.y),
                ],
                white,
            ));
        }
        AccountKind::CreditCard => {
            let r = Rect::from_center_size(c, vec2(s * 2.1, s * 1.45));
            p.rect_stroke(r, 2.0, white, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left(), c.y - s * 0.25), pos2(r.right(), c.y - s * 0.25)], white);
        }
        AccountKind::Loan => {
            // A receipt with a minus.
            p.line_segment([pos2(c.x - s * 0.7, c.y), pos2(c.x + s * 0.7, c.y)], Stroke::new(white.width * 1.3, Color32::WHITE));
            p.add(Shape::line(ring(c, s * 1.0, -PI * 0.85, -PI * 0.15, 10), white));
        }
        _ => {}
    }
}

/// The state of a transaction: a dashed ring with a dot (waiting), a ring with a check (sorted by itself), a filled
/// check (checked by a person).
pub fn status(p: &Painter, c: Pos2, size: f32, s: TxnStatus, t: &Tokens) {
    let col = t.status(s);
    let r = size / 2.0 - 0.75;
    let w = (size / 9.0).max(1.4);
    match s {
        TxnStatus::ToReview => {
            for i in 0..10 {
                let a = i as f32 * TAU / 10.0;
                p.add(Shape::line(ring(c, r, a, a + TAU / 22.0, 3), Stroke::new(w, col)));
            }
            p.circle_filled(c, r * 0.38, col);
        }
        TxnStatus::Auto => {
            p.circle_stroke(c, r, Stroke::new(w, col));
            check(p, c, size * 0.5, Stroke::new(w, col));
        }
        TxnStatus::Reviewed => {
            p.circle_filled(c, r + w / 2.0, col);
            check(p, c, size * 0.55, Stroke::new(w * 1.1, Color32::WHITE));
        }
    }
}

/// The mark of a Review tab.
pub fn review(p: &Painter, c: Pos2, size: f32, k: ReviewKind, t: &Tokens) {
    let col = match k {
        ReviewKind::Duplicate => t.danger,
        ReviewKind::Transfer => t.group(Group::Have),
        ReviewKind::Suggested => t.accent,
        ReviewKind::Unknown => t.warning,
    };
    match k {
        ReviewKind::Duplicate => {
            let r = size * 0.32;
            p.rect_stroke(
                Rect::from_center_size(pos2(c.x - r * 0.35, c.y - r * 0.35), vec2(r * 2.0, r * 2.0)),
                2.0,
                Stroke::new(1.4, col),
                egui::StrokeKind::Middle,
            );
            p.rect_filled(Rect::from_center_size(pos2(c.x + r * 0.35, c.y + r * 0.35), vec2(r * 2.0, r * 2.0)), 2.0, col);
        }
        ReviewKind::Transfer => glyph(p, c, size, Glyph::Swap, col),
        ReviewKind::Suggested => {
            p.circle_filled(c, size / 2.0 - 0.5, col);
            glyph(p, c, size * 0.62, Glyph::Bolt, Color32::WHITE);
        }
        ReviewKind::Unknown => {
            p.circle_filled(c, size / 2.0 - 0.5, col);
            p.text(c, egui::Align2::CENTER_CENTER, "?", crate::theme::bold(size * 0.72), Color32::WHITE);
        }
    }
}

/// Small line glyphs for buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Plus,
    Search,
    Close,
    ChevronDown,
    ChevronRight,
    List,
    Board,
    Trash,
    Filter,
    Sort,
    More,
    Comment,
    Layers,
    Folder,
    Inbox,
    Tag,
    Person,
    Check,
    Sidebar,
    /// Two arrows apart: open wide.
    Expand,
    Link,
    /// A calendar page.
    Sprint,
    Doc,
    /// Four tiles: the Overview.
    Grid,
    /// Bars of a chart: Reports.
    Chart,
    /// A jar: Pots.
    Jar,
    /// An arrow into a tray: import a file.
    Import,
    /// An arrow to the right.
    Arrow,
    /// One line forking into two: split.
    Split,
    /// A lightning bolt: payee rules.
    Bolt,
    /// Two arrows in opposite directions: a transfer.
    Swap,
}

pub fn glyph(p: &Painter, c: Pos2, size: f32, g: Glyph, col: Color32) {
    let s = size / 2.0;
    let st = Stroke::new((size / 11.0).max(1.25), col);
    match g {
        Glyph::Plus => {
            p.line_segment([pos2(c.x - s * 0.8, c.y), pos2(c.x + s * 0.8, c.y)], st);
            p.line_segment([pos2(c.x, c.y - s * 0.8), pos2(c.x, c.y + s * 0.8)], st);
        }
        Glyph::Search => {
            let cc = pos2(c.x - s * 0.15, c.y - s * 0.15);
            p.circle_stroke(cc, s * 0.58, st);
            p.line_segment([pos2(cc.x + s * 0.42, cc.y + s * 0.42), pos2(c.x + s * 0.85, c.y + s * 0.85)], st);
        }
        Glyph::Close => cross(p, c, s * 0.6, st),
        Glyph::Check => check(p, c, size * 0.8, st),
        Glyph::ChevronDown => {
            p.add(Shape::line(vec![pos2(c.x - s * 0.55, c.y - s * 0.25), pos2(c.x, c.y + s * 0.3), pos2(c.x + s * 0.55, c.y - s * 0.25)], st));
        }
        Glyph::ChevronRight => {
            p.add(Shape::line(vec![pos2(c.x - s * 0.25, c.y - s * 0.55), pos2(c.x + s * 0.3, c.y), pos2(c.x - s * 0.25, c.y + s * 0.55)], st));
        }
        Glyph::List => {
            for dy in [-0.6, 0.0, 0.6] {
                p.circle_filled(pos2(c.x - s * 0.75, c.y + dy * s), st.width * 0.75, col);
                p.line_segment([pos2(c.x - s * 0.4, c.y + dy * s), pos2(c.x + s * 0.85, c.y + dy * s)], st);
            }
        }
        Glyph::Board => {
            for (i, h) in [(-1.0f32, 1.6f32), (0.0, 1.1), (1.0, 1.4)] {
                let x = c.x + i * s * 0.62;
                let r = Rect::from_min_max(pos2(x - s * 0.24, c.y - s * 0.8), pos2(x + s * 0.24, c.y - s * 0.8 + h * s));
                p.rect_stroke(r, 1.5, st, egui::StrokeKind::Middle);
            }
        }
        Glyph::Trash => {
            p.line_segment([pos2(c.x - s * 0.8, c.y - s * 0.55), pos2(c.x + s * 0.8, c.y - s * 0.55)], st);
            p.line_segment([pos2(c.x - s * 0.25, c.y - s * 0.8), pos2(c.x + s * 0.25, c.y - s * 0.8)], st);
            p.add(Shape::line(
                vec![
                    pos2(c.x - s * 0.6, c.y - s * 0.55),
                    pos2(c.x - s * 0.45, c.y + s * 0.85),
                    pos2(c.x + s * 0.45, c.y + s * 0.85),
                    pos2(c.x + s * 0.6, c.y - s * 0.55),
                ],
                st,
            ));
        }
        Glyph::Filter => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.85, c.y - s * 0.7),
                    pos2(c.x + s * 0.85, c.y - s * 0.7),
                    pos2(c.x + s * 0.15, c.y + s * 0.05),
                    pos2(c.x + s * 0.15, c.y + s * 0.8),
                    pos2(c.x - s * 0.15, c.y + s * 0.6),
                    pos2(c.x - s * 0.15, c.y + s * 0.05),
                ],
                st,
            ));
        }
        Glyph::Sort => {
            for (i, w) in [(-0.6f32, 0.85f32), (0.0, 0.55), (0.6, 0.25)] {
                p.line_segment([pos2(c.x - s * 0.85, c.y + i * s), pos2(c.x - s * 0.85 + w * 2.0 * s, c.y + i * s)], st);
            }
        }
        Glyph::More => {
            for i in -1..=1 {
                p.circle_filled(pos2(c.x + i as f32 * s * 0.6, c.y), st.width * 0.9, col);
            }
        }
        Glyph::Comment => {
            let r = Rect::from_min_max(pos2(c.x - s * 0.85, c.y - s * 0.7), pos2(c.x + s * 0.85, c.y + s * 0.45));
            p.rect_stroke(r, s * 0.3, st, egui::StrokeKind::Middle);
            p.add(Shape::line(
                vec![pos2(c.x - s * 0.35, c.y + s * 0.45), pos2(c.x - s * 0.55, c.y + s * 0.85), pos2(c.x + s * 0.05, c.y + s * 0.45)],
                st,
            ));
        }
        Glyph::Layers => {
            let d = |dy: f32| {
                vec![pos2(c.x - s * 0.85, c.y + dy), pos2(c.x, c.y + dy - s * 0.42), pos2(c.x + s * 0.85, c.y + dy), pos2(c.x, c.y + dy + s * 0.42)]
            };
            p.add(Shape::closed_line(d(-s * 0.3), st));
            p.add(Shape::line(vec![pos2(c.x - s * 0.85, c.y + s * 0.3), pos2(c.x, c.y + s * 0.72), pos2(c.x + s * 0.85, c.y + s * 0.3)], st));
        }
        Glyph::Folder => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.85, c.y - s * 0.6),
                    pos2(c.x - s * 0.2, c.y - s * 0.6),
                    pos2(c.x, c.y - s * 0.35),
                    pos2(c.x + s * 0.85, c.y - s * 0.35),
                    pos2(c.x + s * 0.85, c.y + s * 0.7),
                    pos2(c.x - s * 0.85, c.y + s * 0.7),
                ],
                st,
            ));
        }
        Glyph::Inbox => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.85, c.y - s * 0.65),
                    pos2(c.x + s * 0.85, c.y - s * 0.65),
                    pos2(c.x + s * 0.85, c.y + s * 0.7),
                    pos2(c.x - s * 0.85, c.y + s * 0.7),
                ],
                st,
            ));
            p.add(Shape::line(
                vec![
                    pos2(c.x - s * 0.85, c.y + s * 0.1),
                    pos2(c.x - s * 0.3, c.y + s * 0.1),
                    pos2(c.x - s * 0.15, c.y + s * 0.35),
                    pos2(c.x + s * 0.15, c.y + s * 0.35),
                    pos2(c.x + s * 0.3, c.y + s * 0.1),
                    pos2(c.x + s * 0.85, c.y + s * 0.1),
                ],
                st,
            ));
        }
        Glyph::Tag => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.85, c.y - s * 0.85),
                    pos2(c.x + s * 0.05, c.y - s * 0.85),
                    pos2(c.x + s * 0.85, c.y - s * 0.05),
                    pos2(c.x - s * 0.05, c.y + s * 0.85),
                    pos2(c.x - s * 0.85, c.y + s * 0.05),
                ],
                st,
            ));
            p.circle_filled(pos2(c.x - s * 0.4, c.y - s * 0.4), st.width, col);
        }
        Glyph::Person => {
            p.circle_stroke(pos2(c.x, c.y - s * 0.35), s * 0.38, st);
            p.add(Shape::line(ring(pos2(c.x, c.y + s * 0.95), s * 0.75, PI * 1.08, PI * 1.92, 12), st));
        }
        Glyph::Sidebar => {
            let r = Rect::from_center_size(c, vec2(size * 0.9, size * 0.75));
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left() + r.width() * 0.36, r.top()), pos2(r.left() + r.width() * 0.36, r.bottom())], st);
        }
        Glyph::Expand => {
            let (a, b) = (pos2(c.x - s * 0.75, c.y + s * 0.75), pos2(c.x + s * 0.75, c.y - s * 0.75));
            p.line_segment([a, b], st);
            p.add(Shape::line(vec![pos2(b.x - s * 0.55, b.y), b, pos2(b.x, b.y + s * 0.55)], st));
            p.add(Shape::line(vec![pos2(a.x + s * 0.55, a.y), a, pos2(a.x, a.y - s * 0.55)], st));
        }
        Glyph::Link => {
            // Two chain links.
            for dx in [-0.32f32, 0.32] {
                let r = Rect::from_center_size(pos2(c.x + dx * s, c.y), vec2(s * 1.05, s * 0.7));
                p.rect_stroke(r, s * 0.35, st, egui::StrokeKind::Middle);
            }
        }
        Glyph::Sprint => {
            let r = Rect::from_center_size(pos2(c.x, c.y + s * 0.08), vec2(size * 0.85, size * 0.75));
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left(), r.top() + r.height() * 0.32), pos2(r.right(), r.top() + r.height() * 0.32)], st);
            for dx in [-0.4f32, 0.4] {
                p.line_segment([pos2(c.x + dx * s, r.top() - s * 0.2), pos2(c.x + dx * s, r.top() + s * 0.15)], st);
            }
        }
        Glyph::Doc => {
            let r = Rect::from_center_size(c, vec2(size * 0.7, size * 0.9));
            p.rect_stroke(r, 1.5, st, egui::StrokeKind::Middle);
            for dy in [-0.25f32, 0.05, 0.35] {
                p.line_segment([pos2(r.left() + s * 0.25, c.y + dy * s), pos2(r.right() - s * 0.25, c.y + dy * s)], st);
            }
        }
        Glyph::Grid => {
            for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let r = Rect::from_center_size(pos2(c.x + dx * s * 0.45, c.y + dy * s * 0.45), vec2(s * 0.7, s * 0.7));
                p.rect_stroke(r, 1.5, st, egui::StrokeKind::Middle);
            }
        }
        Glyph::Chart => {
            for (i, h) in [(-1.0f32, 0.7f32), (0.0, 1.5), (1.0, 1.05)] {
                let x = c.x + i * s * 0.6;
                p.line_segment([pos2(x, c.y + s * 0.8), pos2(x, c.y + s * 0.8 - h * s)], Stroke::new(st.width * 1.6, col));
            }
        }
        Glyph::Jar => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.45, c.y - s * 0.55),
                    pos2(c.x + s * 0.45, c.y - s * 0.55),
                    pos2(c.x + s * 0.7, c.y - s * 0.2),
                    pos2(c.x + s * 0.7, c.y + s * 0.85),
                    pos2(c.x - s * 0.7, c.y + s * 0.85),
                    pos2(c.x - s * 0.7, c.y - s * 0.2),
                ],
                st,
            ));
            p.line_segment([pos2(c.x - s * 0.5, c.y - s * 0.85), pos2(c.x + s * 0.5, c.y - s * 0.85)], st);
            p.circle_stroke(pos2(c.x, c.y + s * 0.25), s * 0.22, st);
        }
        Glyph::Import => {
            p.line_segment([pos2(c.x, c.y - s * 0.9), pos2(c.x, c.y + s * 0.2)], st);
            p.add(Shape::line(vec![pos2(c.x - s * 0.4, c.y - s * 0.2), pos2(c.x, c.y + s * 0.2), pos2(c.x + s * 0.4, c.y - s * 0.2)], st));
            p.add(Shape::line(
                vec![
                    pos2(c.x - s * 0.85, c.y + s * 0.2),
                    pos2(c.x - s * 0.85, c.y + s * 0.8),
                    pos2(c.x + s * 0.85, c.y + s * 0.8),
                    pos2(c.x + s * 0.85, c.y + s * 0.2),
                ],
                st,
            ));
        }
        Glyph::Arrow => {
            p.line_segment([pos2(c.x - s * 0.8, c.y), pos2(c.x + s * 0.8, c.y)], st);
            p.add(Shape::line(vec![pos2(c.x + s * 0.35, c.y - s * 0.45), pos2(c.x + s * 0.8, c.y), pos2(c.x + s * 0.35, c.y + s * 0.45)], st));
        }
        Glyph::Split => {
            p.line_segment([pos2(c.x - s * 0.85, c.y), pos2(c.x - s * 0.1, c.y)], st);
            p.add(Shape::line(vec![pos2(c.x + s * 0.85, c.y - s * 0.6), pos2(c.x + s * 0.15, c.y - s * 0.6), pos2(c.x - s * 0.1, c.y)], st));
            p.add(Shape::line(vec![pos2(c.x + s * 0.85, c.y + s * 0.6), pos2(c.x + s * 0.15, c.y + s * 0.6), pos2(c.x - s * 0.1, c.y)], st));
        }
        Glyph::Bolt => {
            p.add(Shape::closed_line(
                vec![
                    pos2(c.x + s * 0.2, c.y - s * 0.95),
                    pos2(c.x - s * 0.55, c.y + s * 0.1),
                    pos2(c.x - s * 0.02, c.y + s * 0.1),
                    pos2(c.x - s * 0.2, c.y + s * 0.95),
                    pos2(c.x + s * 0.55, c.y - s * 0.1),
                    pos2(c.x + s * 0.02, c.y - s * 0.1),
                ],
                st,
            ));
        }
        Glyph::Swap => {
            p.line_segment([pos2(c.x - s * 0.8, c.y - s * 0.35), pos2(c.x + s * 0.8, c.y - s * 0.35)], st);
            p.add(Shape::line(
                vec![pos2(c.x + s * 0.4, c.y - s * 0.75), pos2(c.x + s * 0.8, c.y - s * 0.35), pos2(c.x + s * 0.4, c.y + s * 0.05)],
                st,
            ));
            p.line_segment([pos2(c.x - s * 0.8, c.y + s * 0.4), pos2(c.x + s * 0.8, c.y + s * 0.4)], st);
            p.add(Shape::line(vec![pos2(c.x - s * 0.4, c.y), pos2(c.x - s * 0.8, c.y + s * 0.4), pos2(c.x - s * 0.4, c.y + s * 0.8)], st));
        }
    }
}

/// A round badge with someone's initials.
pub fn avatar(p: &Painter, c: Pos2, size: f32, name: &str, t: &Tokens) {
    let col = t.tag(name);
    p.circle_filled(c, size / 2.0, col);
    let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
    let initials = if initials.is_empty() { "?".into() } else { initials };
    p.text(c, egui::Align2::CENTER_CENTER, initials, crate::theme::bold(size * 0.44), Color32::WHITE);
}

/// The app icon in `rect`: a teal tile with three stacked coins and a rising line. Our own design; `xtask icon`
/// renders it into the macOS .icns.
pub fn app_icon(p: &Painter, rect: Rect) {
    let s = rect.width();
    let tile = Rect::from_center_size(rect.center() - vec2(0.0, s * 0.01), vec2(s * 0.805, s * 0.805));
    let r = s * 0.18;
    p.rect_filled(tile.translate(vec2(0.0, s * 0.012)), r, Color32::from_black_alpha(60));
    p.rect_filled(tile, r, Color32::from_rgb(0x0F, 0x76, 0x6E));
    let top = Rect::from_min_max(tile.min, pos2(tile.right(), tile.center().y));
    p.rect_filled(top, egui::CornerRadius { nw: r as u8, ne: r as u8, sw: 0, se: 0 }, Color32::from_rgba_unmultiplied(255, 255, 255, 20));
    let gold = Color32::from_rgb(0xF4, 0xC4, 0x4E);
    let gold_dark = Color32::from_rgb(0xC9, 0x93, 0x1F);
    let cw = tile.width() * 0.42;
    let ch = tile.height() * 0.1;
    let base = pos2(tile.left() + tile.width() * 0.36, tile.bottom() - tile.height() * 0.2);
    for i in 0..3 {
        let c = pos2(base.x, base.y - i as f32 * ch * 1.05);
        let body = Rect::from_center_size(pos2(c.x, c.y + ch * 0.25), vec2(cw, ch));
        p.rect_filled(body, ch * 0.5, gold_dark);
        p.rect_filled(Rect::from_center_size(c, vec2(cw, ch)), ch * 0.5, gold);
    }
    let pts = [(0.56, 0.62), (0.68, 0.48), (0.76, 0.54), (0.86, 0.28)];
    let line: Vec<Pos2> = pts.iter().map(|(x, y)| pos2(tile.left() + tile.width() * x, tile.top() + tile.height() * y)).collect();
    p.add(Shape::line(line.clone(), Stroke::new(s * 0.035, Color32::WHITE)));
    if let Some(end) = line.last() {
        p.circle_filled(*end, s * 0.035, Color32::WHITE);
    }
}
