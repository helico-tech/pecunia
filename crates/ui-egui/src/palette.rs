//! The command palette (⌘K): jump to an account or a transaction, or run a command by name.

use egui::{Align2, Key, Sense, Stroke, pos2, vec2};
use pecunia_engine::Screen;
use pecunia_engine::command_specs;
use pecunia_engine::model::{AccountId, TxnId};
use serde_json::json;

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, UI_COMMANDS};

#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub cursor: usize,
    focused: bool,
}

impl Palette {
    /// A palette opened with `query` already typed.
    pub fn with_query(query: &str) -> Palette {
        Palette { query: query.to_string(), ..Default::default() }
    }
}

#[derive(Clone)]
enum Hit {
    Account(AccountId),
    Txn(TxnId),
    Command(&'static str, &'static str, Option<&'static str>),
}

fn hits(app: &PecuniaApp, q: &str) -> Vec<Hit> {
    let q = q.trim().to_lowercase();
    let book = app.session.book();
    let mut out = Vec::new();
    if !q.is_empty() {
        let words: Vec<&str> = q.split_whitespace().collect();
        let tree = book.tree();
        for (a, _) in tree.walk_depth().into_iter().filter(|(a, _)| a.system.is_none() && !a.unassigned) {
            if words.iter().all(|w| a.name.to_lowercase().contains(w)) && out.len() < 6 {
                out.push(Hit::Account(a.id));
            }
        }
        let txns =
            book.txns().rev().filter(|t| words.iter().all(|w| t.payee.to_lowercase().contains(w) || t.note.to_lowercase().contains(w))).take(8);
        out.extend(txns.map(|t| Hit::Txn(t.id)));
    }
    let matches = |label: &str| q.is_empty() || q.split_whitespace().all(|w| label.to_lowercase().contains(w));
    for spec in command_specs() {
        if !spec.menu.is_empty() && (spec.enabled)(&app.session).is_ok() && matches(spec.label) {
            out.push(Hit::Command(spec.id, spec.label, spec.shortcut));
        }
    }
    for (id, label, sc) in UI_COMMANDS {
        if *id != "ui.palette" && matches(label) {
            out.push(Hit::Command(id, label, *sc));
        }
    }
    out
}

pub fn show(app: &mut PecuniaApp, ctx: &egui::Context, t: &Tokens) {
    let Some(mut pal) = app.ui.palette.take() else { return };
    let found = hits(app, &pal.query);
    pal.cursor = pal.cursor.min(found.len().saturating_sub(1));
    let mut chosen: Option<Hit> = None;
    let mut close = false;
    let frame = egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border_strong))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(8))
        .shadow(egui::epaint::Shadow { offset: [0, 12], blur: 36, spread: 0, color: t.shadow });
    let screen = ctx.content_rect();
    let modal = egui::Modal::new(egui::Id::new("palette"))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(if t.dark { 100 } else { 40 }))
        .area(egui::Modal::default_area(egui::Id::new("palette-area")).anchor(Align2::CENTER_TOP, vec2(0.0, screen.height() * 0.14)));
    let book = app.session.book().clone();
    let resp = modal.show(ctx, |ui| {
        ui.set_width(600.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 32.0), Sense::hover());
            icons::glyph(ui.painter(), r.center(), 14.0, Glyph::Search, t.text_faint);
            let te = ui.add(
                egui::TextEdit::singleline(&mut pal.query)
                    .font(theme::body(16.0))
                    .frame(egui::Frame::NONE)
                    .hint_text("Go to an account or a transaction, or run a command…")
                    .desired_width(f32::INFINITY),
            );
            if !pal.focused {
                te.request_focus();
                pal.focused = true;
            }
            if te.changed() {
                pal.cursor = 0;
            }
        });
        ui.add_space(4.0);
        ui.separator();
        if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
            pal.cursor = (pal.cursor + 1).min(found.len().saturating_sub(1));
        }
        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
            pal.cursor = pal.cursor.saturating_sub(1);
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            chosen = found.get(pal.cursor).cloned();
        }
        if found.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Nothing found").color(t.text_faint));
            ui.add_space(8.0);
        }
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            let mut last_kind = None;
            for (i, h) in found.iter().enumerate() {
                let group = match h {
                    Hit::Account(_) => 0,
                    Hit::Txn(_) => 1,
                    Hit::Command(..) => 2,
                };
                if last_kind != Some(group) {
                    ui.add_space(4.0);
                    let heading = ["ACCOUNTS", "TRANSACTIONS", "COMMANDS"].get(group).copied().unwrap_or("");
                    ui.label(egui::RichText::new(heading).font(theme::bold(10.5)).color(t.text_faint));
                    last_kind = Some(group);
                }
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                if i == pal.cursor || r.hovered() {
                    ui.painter().rect_filled(rect, 6.0, if i == pal.cursor { t.selected } else { t.hover });
                }
                if r.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                    pal.cursor = i;
                }
                let cy = rect.center().y;
                match h {
                    Hit::Account(id) => {
                        if let Some(a) = book.account(*id) {
                            icons::account(ui.painter(), pos2(rect.left() + 14.0, cy), 14.0, a.kind, a.group, t);
                            let label = crate::widgets::elide(ui, &book.account_label(*id), theme::body(13.5), rect.width() - 60.0);
                            ui.painter().text(pos2(rect.left() + 34.0, cy), Align2::LEFT_CENTER, label, theme::body(13.5), t.text);
                        }
                    }
                    Hit::Txn(id) => {
                        if let Some(x) = book.txn(*id) {
                            icons::status(ui.painter(), pos2(rect.left() + 14.0, cy), 13.0, x.status, t);
                            ui.painter().text(
                                pos2(rect.left() + 34.0, cy),
                                Align2::LEFT_CENTER,
                                x.date.format("%-d %b %Y").to_string(),
                                theme::body(12.0),
                                t.text_faint,
                            );
                            let payee = crate::widgets::elide(ui, &x.payee, theme::body(13.5), rect.width() - 260.0);
                            ui.painter().text(pos2(rect.left() + 120.0, cy), Align2::LEFT_CENTER, payee, theme::body(13.5), t.text);
                            let amount = x.bank_amount.unwrap_or_else(|| x.size());
                            ui.painter().text(pos2(rect.right() - 12.0, cy), Align2::RIGHT_CENTER, amount.format(), theme::body(12.5), t.text_dim);
                        }
                    }
                    Hit::Command(_, label, sc) => {
                        icons::glyph(ui.painter(), pos2(rect.left() + 14.0, cy), 12.0, Glyph::ChevronRight, t.text_faint);
                        ui.painter().text(pos2(rect.left() + 34.0, cy), Align2::LEFT_CENTER, label.trim_end_matches('…'), theme::body(13.5), t.text);
                        if let Some(sc) = sc {
                            ui.painter().text(pos2(rect.right() - 12.0, cy), Align2::RIGHT_CENTER, *sc, theme::body(12.0), t.text_faint);
                        }
                    }
                }
                if i == pal.cursor && ui.input(|i| i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowUp)) {
                    r.scroll_to_me(None);
                }
                if r.clicked() {
                    chosen = Some(h.clone());
                }
            }
        });
    });
    if resp.should_close() {
        close = true;
    }
    match chosen {
        Some(Hit::Account(id)) => app.show_account(id),
        Some(Hit::Txn(id)) => {
            if !matches!(app.session.view.screen, Screen::Transactions | Screen::Review) {
                app.ui.search.clear();
                app.run("view.filter", json!({ "replace": true }));
            }
            app.select(Some(id));
            app.ui.scroll_to_selection = true;
        }
        Some(Hit::Command(id, _, _)) => {
            app.run_any(id, json!({}));
        }
        None if !close => app.ui.palette = Some(pal),
        None => {}
    }
}
