//! The right-hand pane for the selected transaction: what it is, the question Review asks about it (with one-click
//! answers), where the money sits, its parts, and its note. Text fields keep a buffer while focused and commit with
//! `txn.update` when focus leaves.

use egui::{Align2, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::classify::counter_account;
use pecunia_engine::model::register::{arrow, household_amount, leaf_label};
use pecunia_engine::model::{AccountKind, Book, Group, ReviewKind, Txn, TxnId, TxnKind, TxnStatus};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::pickers::{self, Choices};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

#[derive(Default)]
pub struct Buffers {
    pub txn: Option<TxnId>,
    pub payee: String,
    pub note: String,
    pub date: String,
    /// "Always for this payee": the next answer also makes a rule.
    pub always: bool,
}

const PAYEE_ID: &str = "detail-payee";
const NOTE_ID: &str = "detail-note";
const DATE_ID: &str = "detail-date";

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let Some(id) = app.session.selection else { return };
    let book = app.session.book().clone();
    let Some(txn) = book.txn(id).cloned() else { return };
    let ctx = ui.ctx().clone();
    let focus = |name: &str| ctx.memory(|m| m.has_focus(egui::Id::new(name)));
    let b = &mut app.ui.buffers;
    if b.txn != Some(id) {
        *b = Buffers { txn: Some(id), ..Default::default() };
    }
    if !focus(PAYEE_ID) {
        b.payee = txn.payee.clone();
    }
    if !focus(NOTE_ID) {
        b.note = txn.note.clone();
    }
    if !focus(DATE_ID) {
        b.date = txn.date.to_string();
    }

    let full = ui.max_rect();
    ui.painter().line_segment([full.left_top(), full.left_bottom()], Stroke::new(1.0, t.border));
    let mut actions: Vec<(&'static str, Json)> = Vec::new();
    let mut close = false;
    header(ui, &book, &txn, t, &mut actions, &mut close);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 18, bottom: 24 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            summary(app, ui, &book, &txn, t, &mut actions);
            if let Some(kind) = book.review_kind(&txn) {
                ui.add_space(16.0);
                question(app, ui, &book, &txn, kind, t, &mut actions);
            }
            ui.add_space(18.0);
            properties(app, ui, &book, &txn, t, &mut actions);
            ui.add_space(18.0);
            parts(ui, &book, &txn, t);
            ui.add_space(18.0);
            facts(ui, &book, &txn, t);
        });
    });
    if close {
        app.select(None);
    }
    for (cmd, p) in actions {
        app.run_any(cmd, p);
    }
}

fn header(ui: &mut Ui, book: &Book, txn: &Txn, t: &Tokens, actions: &mut Vec<(&'static str, Json)>, close: &mut bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let inner = rect.shrink2(vec2(20.0, 0.0));
    let mut right = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
    if widgets::icon_button(&mut right, Glyph::Close, "Close (Esc)", t).clicked() {
        *close = true;
    }
    let more = widgets::icon_button(&mut right, Glyph::More, "More", t);
    egui::Popup::menu(&more).frame(widgets::popup_frame(t)).show(|ui| {
        if txn.account.is_some() && widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Split, t.text_dim), "Split…", false, t).clicked() {
            actions.push(("ui.split", json!({ "id": txn.id.0 })));
            ui.close();
        }
        if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Bolt, t.text_dim), "Make a payee rule…", false, t).clicked() {
            if let Some(c) = counter_account(&book.tree(), txn).filter(|c| book.account(*c).is_some_and(|a| a.kind == AccountKind::Category)) {
                actions.push(("rule.create", json!({ "payee": txn.payee_norm, "to": c.0 })));
            } else {
                actions.push(("view.rules", json!({})));
            }
            ui.close();
        }
        if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Trash, t.danger), "Delete transaction", false, t).clicked() {
            actions.push(("txn.delete", json!({ "id": txn.id.0 })));
            ui.close();
        }
    });
    let mut left = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let (r, _) = left.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    icons::status(left.painter(), r.center(), 15.0, txn.status, t);
    left.label(egui::RichText::new(txn.status.label()).size(12.5).color(t.text_dim));
    left.label(egui::RichText::new(format!("· #{}", txn.id.0)).size(12.5).color(t.text_faint));
}

fn summary(app: &mut PecuniaApp, ui: &mut Ui, book: &Book, txn: &Txn, t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let b = &mut app.ui.buffers;
    let payee = ui.add(
        egui::TextEdit::singleline(&mut b.payee)
            .id(egui::Id::new(PAYEE_ID))
            .font(theme::bold(18.0))
            .frame(egui::Frame::NONE)
            .hint_text("Payee")
            .desired_width(f32::INFINITY),
    );
    if payee.lost_focus() && b.payee.trim() != txn.payee {
        actions.push(("txn.update", json!({ "id": txn.id.0, "payee": b.payee })));
    }
    ui.add_space(6.0);
    let tree = book.tree();
    let amount = household_amount(&tree, txn);
    let col = if amount.is_positive() && txn.kind != TxnKind::PotMove { t.positive } else { t.text };
    ui.label(egui::RichText::new(amount.format()).font(theme::bold(26.0)).color(col));
    let (from, to) = arrow(&tree, txn);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(from).size(13.0).color(t.text_dim));
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
        icons::glyph(ui.painter(), r.center(), 12.0, Glyph::Arrow, t.text_faint);
        let unsorted = to.contains("Not sorted yet");
        ui.label(egui::RichText::new(to).size(13.0).color(if unsorted { t.warning } else { t.text_dim }));
    });
}

/// The question Review asks about this transaction, with answers one click away.
fn question(app: &mut PecuniaApp, ui: &mut Ui, book: &Book, txn: &Txn, kind: ReviewKind, t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let (fill, stroke) = match kind {
        ReviewKind::Duplicate => (t.danger_soft, t.danger),
        _ => (t.warning_soft, t.warning),
    };
    let id = txn.id.0;
    egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, stroke.gamma_multiply(0.6))).corner_radius(9).inner_margin(egui::Margin::same(14)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::review(ui.painter(), r.center(), 16.0, kind, t);
                let title = match kind {
                    ReviewKind::Duplicate => "Is this a copy?",
                    ReviewKind::Transfer => "Money between your own accounts?",
                    ReviewKind::Suggested => "Is this right?",
                    ReviewKind::Unknown => "Where did this money go?",
                };
                ui.label(egui::RichText::new(title).font(theme::bold(14.0)).color(t.text));
            });
            ui.add_space(6.0);
            match kind {
                ReviewKind::Duplicate => {
                    let other = txn.duplicate_of.and_then(|d| book.txn(d));
                    let text = match other {
                        Some(o) => format!("It looks like #{} on {} ({}), from another file of the same account.", o.id.0, o.date, o.payee),
                        None => "It looks like a transaction from another file of the same account.".into(),
                    };
                    ui.label(egui::RichText::new(text).size(12.5).color(t.text_dim));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::danger_button(ui, "Delete this copy", t).clicked() {
                            actions.push(("review.duplicate", json!({ "id": id, "keep": false })));
                        }
                        if widgets::secondary_button(ui, "Keep both", t).clicked() {
                            actions.push(("review.duplicate", json!({ "id": id, "keep": true })));
                        }
                    });
                }
                ReviewKind::Transfer => {
                    let guess = txn.guesses.first().map(|g| g.account);
                    let name = guess.map(|g| book.account_label(g)).unwrap_or_else(|| "another account of yours".into());
                    ui.label(egui::RichText::new(format!("{} — the other side looks like {name}.", txn.via)).size(12.5).color(t.text_dim));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if let Some(g) = guess
                            && widgets::primary_button(ui, &format!("Yes, to {}", book.account_label(g)), t).clicked()
                        {
                            actions.push(("txn.answer", json!({ "id": id, "to": g.0 })));
                        }
                        if widgets::secondary_button(ui, "Not a transfer", t).clicked() {
                            actions.push(("review.notATransfer", json!({ "id": id })));
                        }
                    });
                }
                ReviewKind::Suggested | ReviewKind::Unknown => {
                    if !txn.via.is_empty() {
                        ui.label(egui::RichText::new(&txn.via).size(12.5).color(t.text_dim));
                    }
                    let items = book.review();
                    if let Some(item) = items.iter().find(|i| i.id == txn.id).filter(|i| !i.history.is_empty()) {
                        let earlier: Vec<String> = item.history.iter().map(|(a, n)| format!("{} {n}×", book.account_label(*a))).collect();
                        ui.label(egui::RichText::new(format!("Earlier: {}", earlier.join(", "))).size(12.5).color(t.text_dim));
                    }
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for g in &txn.guesses {
                            let label = format!("{} · {:.0}%", book.account_label(g.account), g.probability * 100.0);
                            if widgets::primary_button(ui, &label, t).clicked() {
                                actions.push(("txn.answer", json!({ "id": id, "to": g.account.0, "always": app.ui.buffers.always })));
                            }
                        }
                        if let Some(a) = pickers::pill(ui, book, Choices::Counter, None, "Choose…", t) {
                            actions.push(("txn.answer", json!({ "id": id, "to": a.0, "always": app.ui.buffers.always })));
                        }
                    });
                    if txn.account.is_some() && !txn.payee_norm.is_empty() {
                        ui.add_space(4.0);
                        ui.checkbox(&mut app.ui.buffers.always, egui::RichText::new(format!("Always for {}", txn.payee_norm)).size(12.5));
                    }
                }
            }
        },
    );
}

fn prop_row(ui: &mut Ui, label: &str, t: &Tokens, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(96.0, 28.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, label, theme::body(12.5), t.text_dim);
        add(ui);
    });
}

fn properties(app: &mut PecuniaApp, ui: &mut Ui, book: &Book, txn: &Txn, t: &Tokens, actions: &mut Vec<(&'static str, Json)>) {
    let tree = book.tree();
    let id = txn.id.0;
    prop_row(ui, "Date", t, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut app.ui.buffers.date).id(egui::Id::new(DATE_ID)).desired_width(110.0));
        if r.lost_focus() && app.ui.buffers.date.trim() != txn.date.to_string() {
            actions.push(("txn.update", json!({ "id": id, "date": app.ui.buffers.date.trim() })));
        }
    });
    if let Some(acc) = txn.account {
        prop_row(ui, "Account", t, |ui| {
            if let Some(a) = book.account(acc) {
                let r = widgets::pill(ui, |p, c| icons::account(p, c, 13.0, a.kind, a.group, t), &a.name, false, t);
                if r.on_hover_text("Show this account's transactions").clicked() {
                    actions.push(("view.filter", json!({ "replace": true, "account": acc.0 })));
                }
            }
        });
        // Which pot of the account pays (Have accounts with pots).
        let pots = tree.pots(acc);
        if pots.len() > 1 {
            let own = txn.postings.iter().find(|p| tree.physical_of(p.account) == Some(acc)).map(|p| p.account);
            prop_row(ui, "Pot", t, |ui| {
                if let Some(p) = pickers::pill(ui, book, Choices::PotsOf(acc), own, "Unassigned", t)
                    && Some(p) != own
                {
                    actions.push(("txn.moveToPot", json!({ "id": id, "pot": p.0 })));
                }
            });
        }
        let counter = counter_account(&tree, txn);
        let label = if txn.bank_amount.is_some_and(|m| m.is_positive()) { "From" } else { "Category" };
        prop_row(ui, label, t, |ui| {
            let split = counter.is_none();
            if split {
                ui.label(egui::RichText::new("Split over several").size(13.0).color(t.text_dim));
                if ui.link("Change").clicked() {
                    actions.push(("ui.split", json!({ "id": id })));
                }
            } else if let Some(a) = pickers::pill(ui, book, Choices::Counter, counter, "Not sorted yet", t) {
                actions.push(("txn.answer", json!({ "id": id, "to": a.0, "always": app.ui.buffers.always })));
            }
        });
    } else if txn.kind == TxnKind::Manual {
        let category = txn.postings.iter().find(|p| tree.physical_of(p.account).is_none()).map(|p| p.account);
        prop_row(ui, "Category", t, |ui| {
            if let Some(a) = pickers::pill(ui, book, Choices::Counter, category, "Choose…", t) {
                actions.push(("txn.answer", json!({ "id": id, "to": a.0 })));
            }
        });
    }
    if txn.status != TxnStatus::Reviewed && book.review_kind(txn).is_none() {
        prop_row(ui, "", t, |ui| {
            if widgets::secondary_button(ui, "Mark as checked  ⌘↩", t).clicked() {
                actions.push(("txn.confirm", json!({ "id": id })));
            }
        });
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new("Note").size(12.5).color(t.text_dim));
    let r = ui.add(
        egui::TextEdit::multiline(&mut app.ui.buffers.note)
            .id(egui::Id::new(NOTE_ID))
            .hint_text("Add a note…")
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    if r.lost_focus() && app.ui.buffers.note.trim_end() != txn.note {
        actions.push(("txn.update", json!({ "id": id, "note": app.ui.buffers.note })));
    }
}

/// The postings, in plain words: which account got how much.
fn parts(ui: &mut Ui, book: &Book, txn: &Txn, t: &Tokens) {
    let tree = book.tree();
    widgets::section(ui, "Where the money sits", t);
    ui.add_space(4.0);
    for p in &txn.postings {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        if let Some(a) = book.account(p.account) {
            icons::account(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), 12.0, a.kind, a.group, t);
            let name = leaf_label(&tree, p.account);
            let name = if p.memo.is_empty() { name } else { format!("{name} · {}", p.memo) };
            let name = widgets::elide(ui, &name, theme::body(13.0), rect.width() - 130.0);
            let col = if a.group == Group::Equity { t.text_faint } else { t.text };
            ui.painter().text(pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(13.0), col);
        }
        widgets::money(ui.painter(), rect.right(), rect.center().y, p.amount, false, 13.0, t);
    }
}

fn facts(ui: &mut Ui, book: &Book, txn: &Txn, t: &Tokens) {
    let mut lines: Vec<String> = Vec::new();
    if !txn.description.is_empty() && !txn.payee.contains(&txn.description) {
        lines.push(txn.description.clone());
    }
    if let Some(i) = &txn.counterparty_iban {
        lines.push(format!("Other account: {i}"));
    }
    if let Some(w) = &txn.who {
        lines.push(format!("Card holder: {w}"));
    }
    let source = match txn.import.and_then(|i| book.meta.imports.iter().find(|x| x.id == i)) {
        Some(i) => format!("Imported from {} ({})", i.file, i.format),
        None => txn.kind.label().to_string(),
    };
    lines.push(source);
    if !txn.via.is_empty() && txn.status == TxnStatus::Auto {
        lines.push(format!("Sorted by: {}", txn.via));
    }
    for l in lines {
        ui.label(egui::RichText::new(l).size(11.5).color(t.text_faint));
    }
}
