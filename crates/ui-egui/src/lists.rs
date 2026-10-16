//! Two simple screens: the payee rules (what the classifier books by itself) and the imports (what came from which file,
//! and undoing one).

use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::RuleOrigin;
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::pickers::{self, Choices};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

/// A screen title with an explanation under it.
pub fn heading(ui: &mut Ui, title: &str, text: &str, t: &Tokens) {
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).font(theme::bold(18.0)).color(t.text));
            ui.label(egui::RichText::new(text).size(12.5).color(t.text_dim));
        });
    });
    ui.add_space(12.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.border));
}

pub fn rules(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    heading(
        ui,
        "Payee rules",
        "A rule books a payee on a category by itself. Two identical answers in Review make one; a payee answered two ways is mixed and goes to Review again.",
        t,
    );
    let book = app.session.book().clone();
    let mut actions: Vec<(&'static str, Json)> = Vec::new();
    let mut rules: Vec<_> = book.rules().cloned().collect();
    rules.sort_by_key(|r| (std::cmp::Reverse(r.uses), r.id));
    if rules.is_empty() {
        crate::register::centered(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            icons::glyph(ui.painter(), r.center(), 28.0, Glyph::Bolt, t.text_faint);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No rules yet").font(theme::bold(16.0)).color(t.text));
            ui.label(egui::RichText::new("Answer transactions in Review; Pecunia learns rules from your answers.").size(13.0).color(t.text_dim));
        });
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for r in &rules {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
            ui.painter().line_segment(
                [pos2(rect.left() + 20.0, rect.bottom()), pos2(rect.right(), rect.bottom())],
                Stroke::new(1.0, t.border.gamma_multiply(0.7)),
            );
            let cy = rect.center().y;
            icons::glyph(ui.painter(), pos2(rect.left() + 32.0, cy), 13.0, Glyph::Bolt, if r.mixed { t.text_faint } else { t.accent });
            let what = match (&r.payee_prefix, &r.iban) {
                (Some(p), Some(i)) => format!("{p} · {i}"),
                (Some(p), None) => p.clone(),
                (None, Some(i)) => i.clone(),
                (None, None) => String::new(),
            };
            let what = widgets::elide(ui, &what, theme::body(13.5), rect.width() * 0.38);
            ui.painter().text(pos2(rect.left() + 52.0, cy), Align2::LEFT_CENTER, what, theme::body(13.5), if r.mixed { t.text_dim } else { t.text });
            let mut note = format!("{} · used {}×", if r.origin == RuleOrigin::Learned { "learned" } else { "yours" }, r.uses);
            if r.mixed {
                note.push_str(" · mixed answers, not used");
            }
            ui.painter().text(pos2(rect.left() + 52.0 + rect.width() * 0.4, cy), Align2::LEFT_CENTER, note, theme::body(11.5), t.text_faint);
            let right = Rect::from_min_max(pos2(rect.right() - 340.0, rect.top()), pos2(rect.right() - 16.0, rect.bottom()));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if widgets::icon_button(&mut child, Glyph::Trash, "Delete rule", t).clicked() {
                actions.push(("rule.delete", json!({ "id": r.id.0 })));
            }
            if let Some(a) = pickers::pill(&mut child, &book, Choices::Categories, Some(r.account), "Category", t) {
                actions.push(("rule.update", json!({ "id": r.id.0, "to": a.0 })));
            }
        }
    });
    for (id, p) in actions {
        app.run_any(id, p);
    }
}

pub fn imports(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    heading(ui, "Imports", "Every bank file you imported. Importing a file again only adds what is new.", t);
    let book = app.session.book().clone();
    let mut actions: Vec<(&'static str, Json)> = Vec::new();
    let list = book.imports();
    if list.is_empty() {
        crate::register::centered(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            icons::glyph(ui.painter(), r.center(), 28.0, Glyph::Import, t.text_faint);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Nothing imported yet").font(theme::bold(16.0)).color(t.text));
            ui.label(egui::RichText::new("Export a CSV, CAMT.053 or MT940 file from your bank and import it here.").size(13.0).color(t.text_dim));
            ui.add_space(10.0);
            if widgets::primary_button(ui, "Import bank file", t).clicked() {
                actions.push(("import.file", json!({})));
            }
        });
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (i, left) in list {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, t.hover);
            }
            ui.painter().line_segment(
                [pos2(rect.left() + 20.0, rect.bottom()), pos2(rect.right(), rect.bottom())],
                Stroke::new(1.0, t.border.gamma_multiply(0.7)),
            );
            icons::glyph(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), 14.0, Glyph::Import, t.text_dim);
            let title = format!("{} → {}", i.file, book.account_label(i.account));
            ui.painter().text(
                pos2(rect.left() + 52.0, rect.top() + 16.0),
                Align2::LEFT_CENTER,
                widgets::elide(ui, &title, theme::body(13.5), rect.width() - 240.0),
                theme::body(13.5),
                t.text,
            );
            let when = pecunia_engine::model::date_of(i.at);
            let line = format!(
                "{} · {} · {} rows: {} new, {} known, {} transfers, {} duplicates",
                when, i.format, i.rows, i.new, i.known, i.transfers, i.duplicates
            );
            ui.painter().text(
                pos2(rect.left() + 52.0, rect.top() + 34.0),
                Align2::LEFT_CENTER,
                widgets::elide(ui, &line, theme::body(11.5), rect.width() - 240.0),
                theme::body(11.5),
                t.text_dim,
            );
            widgets::a11y(&resp, &title);
            let right = Rect::from_min_max(pos2(rect.right() - 220.0, rect.top()), pos2(rect.right() - 16.0, rect.bottom()));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if left > 0
                && widgets::secondary_button(&mut child, "Undo import", t)
                    .on_hover_text("Removes what it brought that nobody checked since")
                    .clicked()
            {
                actions.push(("import.undo", json!({ "id": i.id.0 })));
            }
            if resp.clicked() {
                actions.push(("view.filter", json!({ "replace": true, "import": i.id.0 })));
            }
        }
    });
    for (id, p) in actions {
        app.run_any(id, p);
    }
}
