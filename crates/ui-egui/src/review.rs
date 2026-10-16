//! The Review screen: transactions waiting for a person, in tabs (possible duplicates, transfers, suggestions,
//! unknown), each with its quickest answer on the row. The detail pane asks the full question for the selected one.

use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::register::household_amount;
use pecunia_engine::model::{ReviewItem, ReviewKind, TxnId};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::pickers::{self, Choices};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

const ROW_H: f32 = 56.0;
/// Phone layout: the quick answer gets a line of its own under the payee.
const ROW_H_COMPACT: f32 = 96.0;

fn items(app: &PecuniaApp) -> Vec<ReviewItem> {
    let tab = app.session.view.review_tab;
    app.session.book().review().into_iter().filter(|i| tab.is_none_or(|t| i.kind == t)).collect()
}

/// The transaction order on screen (for ↑/↓).
pub fn nav_order(app: &PecuniaApp) -> Vec<TxnId> {
    items(app).into_iter().map(|i| i.id).collect()
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let all = app.session.book().review();
    let mut actions: Vec<(&'static str, Json)> = Vec::new();
    // Title row.
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.label(egui::RichText::new("Review").font(theme::bold(18.0)).color(t.text));
    let suggested = all.iter().filter(|i| i.kind == ReviewKind::Suggested).count();
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if suggested > 0
            && widgets::primary_button(ui, &format!("Accept {suggested} suggestion{}", if suggested == 1 { "" } else { "s" }), t)
                .on_hover_text("Book each suggestion on its top guess")
                .clicked()
        {
            actions.push(("review.acceptSuggestions", json!({})));
        }
    });
    // Tabs.
    let (rect2, _) = ui.allocate_exact_size(vec2(full_w, 40.0), Sense::hover());
    ui.painter().line_segment([rect2.left_bottom(), rect2.right_bottom()], Stroke::new(1.0, t.border));
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect2.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let current = app.session.view.review_tab;
    let mut tabs: Vec<(Option<ReviewKind>, String)> = vec![(None, format!("All {}", all.len()))];
    for k in ReviewKind::ALL {
        let n = all.iter().filter(|i| i.kind == k).count();
        if n > 0 || current == Some(k) {
            tabs.push((Some(k), format!("{} {n}", k.label())));
        }
    }
    // Phone layout: the tabs scroll sideways.
    let compact = app.ui.compact;
    let mut tab_row = |row: &mut Ui| {
        for (k, label) in &tabs {
            tab(row, *k, label, current, t, &mut actions);
        }
    };
    if compact {
        egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut row, |ui| {
            ui.horizontal_centered(|ui| tab_row(ui));
        });
    } else {
        tab_row(&mut row);
    }

    let list = items(app);
    if list.is_empty() {
        crate::register::centered(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            icons::status(ui.painter(), r.center(), 30.0, pecunia_engine::model::TxnStatus::Reviewed, t);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Nothing to review").font(theme::bold(16.0)).color(t.text));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Every transaction has a place. New imports that the rules don't know show up here.")
                    .size(13.0)
                    .color(t.text_dim),
            );
        });
    } else {
        rows(app, ui, &list, t, &mut actions);
    }
    for (id, p) in actions {
        app.run_any(id, p);
    }
}

fn tab(row: &mut Ui, k: Option<ReviewKind>, label: &str, current: Option<ReviewKind>, t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let on = current == k;
    let r = widgets::pill_outlined(
        row,
        |p, c| match k {
            Some(k) => icons::review(p, c, 13.0, k, t),
            None => icons::glyph(p, c, 13.0, Glyph::Inbox, t.text_dim),
        },
        label,
        !on,
        t,
    );
    if on {
        row.painter().rect_stroke(r.rect, 7.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    if r.clicked() {
        actions.push(("view.reviewTab", json!({ "tab": k.map(|k| k.id()) })));
    }
}

fn rows(app: &mut PecuniaApp, ui: &mut Ui, list: &[ReviewItem], t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let book = app.session.book().clone();
    let tree = book.tree();
    let selected = app.session.selection;
    let scroll_to = std::mem::take(&mut app.ui.scroll_to_selection);
    let mut select: Option<Option<TxnId>> = None;
    let compact = app.ui.compact;
    let row_h = if compact { ROW_H_COMPACT } else { ROW_H };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let w = ui.available_width();
        let (area, _) = ui.allocate_exact_size(vec2(w, list.len() as f32 * row_h + 24.0), Sense::hover());
        for (i, item) in list.iter().enumerate() {
            let y = i as f32 * row_h;
            let rect = Rect::from_min_size(pos2(area.left(), area.top() + y), vec2(w, row_h));
            if scroll_to && Some(item.id) == selected {
                ui.scroll_to_rect(rect.expand(8.0), None);
            }
            if y + row_h < viewport.top() - row_h || y > viewport.bottom() + row_h {
                continue;
            }
            let Some(txn) = book.txn(item.id) else { continue };
            let resp = ui.interact(rect, ui.id().with(("review", item.id.0)), Sense::click());
            widgets::a11y(&resp, format!("{} {}", txn.date, txn.payee));
            let p = ui.painter().clone();
            if Some(item.id) == selected {
                p.rect_filled(rect, 0.0, t.selected);
                p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, t.accent);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, t.hover);
            }
            p.line_segment(
                [pos2(rect.left() + 16.0, rect.bottom()), pos2(rect.right(), rect.bottom())],
                Stroke::new(1.0, t.border.gamma_multiply(0.7)),
            );
            // Phone layout: payee and amount on top, the quick answer on a line below.
            let cy = if compact { rect.top() + 28.0 } else { rect.center().y };
            icons::review(&p, pos2(rect.left() + 26.0, cy), 16.0, item.kind, t);
            let x = rect.left() + 48.0;
            let amount = household_amount(&tree, txn);
            let ag = p.layout_no_wrap(amount.format(), theme::bold(13.5), if amount.is_positive() { t.positive } else { t.text });
            // Quick answers on the right, before the amount.
            let quick_w = if compact { 0.0 } else { 300.0f32.min(w * 0.4) };
            let amount_x = rect.right() - if compact { 16.0 } else { 20.0 } - ag.size().x;
            p.galley(pos2(amount_x, cy - ag.size().y / 2.0), ag, t.text);
            let text_room = (amount_x - quick_w - 20.0 - x).max(80.0);
            let payee = widgets::elide(ui, &txn.payee, theme::body(13.5), text_room);
            p.text(pos2(x, cy - 9.0), Align2::LEFT_CENTER, payee, theme::body(13.5), t.text);
            let mut second = format!("{} · {}", txn.date.format("%-d %b %Y"), txn.account.map(|a| book.account_label(a)).unwrap_or_default());
            if let Some((a, n)) = item.history.first() {
                second.push_str(&format!(" · earlier {} {n}×", book.account_label(*a)));
            }
            let second = widgets::elide(ui, &second, theme::body(11.5), text_room);
            p.text(pos2(x, cy + 10.0), Align2::LEFT_CENTER, second, theme::body(11.5), t.text_dim);
            let quick = if compact {
                Rect::from_min_max(pos2(x, rect.bottom() - 44.0), pos2(rect.right() - 16.0, rect.bottom() - 10.0))
            } else {
                Rect::from_min_max(pos2(amount_x - quick_w - 12.0, rect.top() + 12.0), pos2(amount_x - 12.0, rect.bottom() - 12.0))
            };
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(quick).layout(egui::Layout::right_to_left(egui::Align::Center)));
            quick_answer(&mut child, &book, item, t, actions);
            if resp.clicked() {
                select = Some(if Some(item.id) == selected { None } else { Some(item.id) });
            }
        }
    });
    if let Some(s) = select {
        app.select(s);
    }
}

/// The fastest answer for a row: the top guess, the transfer, or a picker.
fn quick_answer(ui: &mut Ui, book: &pecunia_engine::model::Book, item: &ReviewItem, t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let Some(txn) = book.txn(item.id) else { return };
    let id = item.id.0;
    match item.kind {
        ReviewKind::Duplicate => {
            if widgets::secondary_button(ui, "Keep both", t).clicked() {
                actions.push(("review.duplicate", json!({ "id": id, "keep": true })));
            }
            if widgets::danger_button(ui, "Delete copy", t).clicked() {
                actions.push(("review.duplicate", json!({ "id": id, "keep": false })));
            }
        }
        ReviewKind::Transfer | ReviewKind::Suggested => {
            if let Some(a) = pickers::pill(ui, book, Choices::Counter, None, "Other…", t) {
                actions.push(("txn.answer", json!({ "id": id, "to": a.0 })));
            }
            if let Some(g) = txn.guesses.first() {
                let label = widgets::elide(ui, &book.account_label(g.account), theme::bold(13.0), 160.0);
                if widgets::primary_button(ui, &label, t).on_hover_text(format!("{:.0}% sure", g.probability * 100.0)).clicked() {
                    actions.push(("txn.answer", json!({ "id": id, "to": g.account.0 })));
                }
            }
        }
        ReviewKind::Unknown => {
            if let Some(a) = pickers::pill(ui, book, Choices::Counter, None, "Where did it go?", t) {
                actions.push(("txn.answer", json!({ "id": id, "to": a.0 })));
            }
        }
    }
}
