//! The left sidebar: the book, New transaction, Import, the screens, and every account with its balance (a click shows
//! its register).

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use pecunia_engine::Screen;
use pecunia_engine::model::{AccountId, Group};
use serde_json::json;

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

/// A sidebar row; `paint` draws the icon at the given centre, `right` is a count or an amount.
fn row(
    ui: &mut Ui,
    paint: impl FnOnce(&egui::Painter, egui::Pos2),
    label: &str,
    right: Option<(String, Color32)>,
    active: bool,
    dim: bool,
    t: &Tokens,
) -> egui::Response {
    let w = ui.available_width();
    let h = if crate::is_compact(ui.ctx()) { 40.0 } else { 28.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let bg = if active {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, bg);
    paint(ui.painter(), pos2(rect.left() + 16.0, rect.center().y));
    let right_w = right.as_ref().map(|(c, _)| ui.painter().layout_no_wrap(c.clone(), theme::body(11.5), t.text_faint).size().x + 12.0).unwrap_or(0.0);
    let font = if active { theme::bold(13.0) } else { theme::body(13.0) };
    let text = widgets::elide(ui, label, font.clone(), rect.width() - 42.0 - right_w);
    ui.painter().text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, text, font, if dim { t.text_faint } else { t.text });
    if let Some((c, col)) = right {
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, c, theme::body(11.5), col);
    }
    widgets::a11y(&resp, label);
    resp
}

fn section(ui: &mut Ui, title: &str, t: &Tokens) {
    ui.add_space(14.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, title, theme::bold(11.0), t.text_faint);
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 0.0), Sense::hover());
    let full = Rect::from_min_max(rect.min, pos2(rect.right(), ui.max_rect().bottom()));
    ui.painter().line_segment([full.right_top(), full.right_bottom()], Stroke::new(1.0, t.border));
    egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 8 }).show(ui, |ui| {
        header(app, ui, t);
        ui.add_space(10.0);
        buttons(app, ui, t);
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| body(app, ui, t));
    });
}

fn header(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let name = app.session.book().name().to_string();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, t.hover);
    }
    let badge = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(24.0, 24.0));
    ui.painter().rect_filled(badge, 6.0, t.accent);
    ui.painter().text(badge.center(), Align2::CENTER_CENTER, "€", theme::bold(13.0), Color32::WHITE);
    let demo = app.session.is_demo();
    let room = rect.width() - 64.0 - if demo { 52.0 } else { 0.0 };
    let text = widgets::elide(ui, &name, theme::bold(14.0), room);
    let g = ui.painter().layout_no_wrap(text, theme::bold(14.0), t.text);
    let text_w = g.size().x;
    ui.painter().galley(pos2(rect.left() + 38.0, rect.center().y - g.size().y / 2.0), g, t.text);
    if demo {
        let badge = Rect::from_min_size(pos2(rect.left() + 44.0 + text_w, rect.center().y - 9.0), vec2(44.0, 18.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(badge));
        widgets::demo_badge(&mut child, 11.0, t);
    }
    icons::glyph(ui.painter(), pos2(rect.right() - 14.0, rect.center().y), 10.0, Glyph::ChevronDown, t.text_dim);
    widgets::a11y(&resp, format!("Book {name}"));
    let resp = resp.on_hover_text(app.session.location());
    let mut action: Option<(&str, serde_json::Value)> = None;
    egui::Popup::menu(&resp).frame(widgets::popup_frame(t)).width(260.0).show(|ui| {
        if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Tag, t.text_dim), "Rename book…", false, t).clicked() {
            action = Some(("ui.rename", json!({})));
            ui.close();
        }
        if app.session.store_path().is_some() {
            let label = if app.session.encrypted() { "Change password…" } else { "Protect with a password…" };
            if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Lock, t.text_dim), label, false, t).clicked() {
                action = Some(("file.setPassword", json!({})));
                ui.close();
            }
            if app.session.encrypted()
                && widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Lock, t.text_faint), "Remove the password", false, t).clicked()
            {
                action = Some(("file.removePassword", json!({})));
                ui.close();
            }
        }
        ui.separator();
        if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Plus, t.text_dim), "New book…", false, t).clicked() {
            action = Some(("file.new", json!({})));
            ui.close();
        }
        if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Folder, t.text_dim), "Open book…", false, t).clicked() {
            action = Some(("file.open", json!({})));
            ui.close();
        }
        let current = app.session.store_path().map(|p| p.display().to_string());
        let recent: Vec<String> = app.ui.recent.iter().filter(|r| Some(*r) != current.as_ref()).take(6).cloned().collect();
        if !recent.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("Recent").size(11.0).color(t.text_faint));
            for r in recent {
                let label = std::path::Path::new(&r).file_stem().and_then(|s| s.to_str()).unwrap_or(&r).to_string();
                if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Folder, t.text_faint), &label, false, t).on_hover_text(&r).clicked() {
                    action = Some(("file.open", json!({ "path": r })));
                    ui.close();
                }
            }
        }
    });
    if let Some((id, p)) = action {
        app.ui.menu = false;
        app.run_any(id, p);
    }
}

fn buttons(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let w = ui.available_width();
    let half = (w - 6.0) / 2.0;
    let mut new = false;
    let mut import = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (label, g, tip, hit) in
            [("New", Glyph::Plus, "New transaction (⌘N)", &mut new), ("Import", Glyph::Import, "Import a bank file (⌘I)", &mut import)]
        {
            let (rect, resp) = ui.allocate_exact_size(vec2(half, 32.0), Sense::click());
            let fill = if resp.hovered() { t.hover } else { t.surface };
            ui.painter().rect(rect, 7.0, fill, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
            icons::glyph(ui.painter(), pos2(rect.left() + 16.0, rect.center().y), 13.0, g, t.text);
            ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, theme::body(13.0), t.text);
            widgets::a11y(&resp, tip);
            if resp.on_hover_text(tip).clicked() {
                *hit = true;
            }
        }
    });
    if new || import {
        app.ui.menu = false;
    }
    if new {
        app.new_txn();
    }
    if import {
        app.run("import.file", json!({}));
    }
}

fn body(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let screen = app.session.view.screen;
    let filter_account = app.session.view.filter.account.filter(|_| screen == Screen::Transactions);
    let book = app.session.book().clone();
    let waiting = book.review_count();
    let mut go: Option<Screen> = None;
    let mut show: Option<AccountId> = None;

    let nav: [(Screen, Glyph, Option<(String, Color32)>); 5] = [
        (Screen::Overview, Glyph::Grid, None),
        (Screen::Review, Glyph::Inbox, (waiting > 0).then(|| (waiting.to_string(), t.warning))),
        (Screen::Transactions, Glyph::List, None),
        (Screen::Pots, Glyph::Jar, None),
        (Screen::Reports, Glyph::Chart, None),
    ];
    for (s, g, right) in nav {
        let active = screen == s && !(s == Screen::Transactions && filter_account.is_some());
        let label = if s == Screen::Transactions { "All transactions" } else { s.label() };
        if row(ui, |p, c| icons::glyph(p, c, 14.0, g, if active { t.accent } else { t.text_dim }), label, right, active, false, t).clicked() {
            go = Some(s);
        }
    }

    let tree = book.tree();
    let today = app.session.today();
    let balances = book.balances_at(today);
    for (group, title) in [(Group::Have, "MONEY YOU HAVE"), (Group::Owe, "MONEY YOU OWE")] {
        let accounts: Vec<_> = tree.physical_accounts(group).into_iter().filter(|a| !a.closed).collect();
        if accounts.is_empty() && group == Group::Owe {
            continue;
        }
        section(ui, title, t);
        for a in accounts {
            let shown = balances.shown(&tree, a.id);
            let col = if shown.is_negative() { t.danger } else { t.text_dim };
            let r = row(
                ui,
                |p, c| icons::account(p, c, 15.0, a.kind, a.group, t),
                &a.name,
                Some((shown.format(), col)),
                filter_account == Some(a.id),
                false,
                t,
            );
            if r.on_hover_text(a.iban.as_deref().unwrap_or(a.kind.label())).clicked() {
                show = Some(a.id);
            }
        }
    }
    let mut new_account = false;
    if row(ui, |p, c| icons::glyph(p, c, 12.0, Glyph::Plus, t.text_faint), "New account", None, false, true, t).clicked() {
        new_account = true;
    }

    section(ui, "BOOK", t);
    for (s, g) in [(Screen::Accounts, Glyph::Layers), (Screen::Rules, Glyph::Bolt), (Screen::Imports, Glyph::Import)] {
        let active = screen == s;
        let right = match s {
            Screen::Rules => Some((book.rules().count().to_string(), t.text_faint)),
            Screen::Imports => Some((book.meta.imports.len().to_string(), t.text_faint)),
            _ => None,
        };
        if row(ui, |p, c| icons::glyph(p, c, 13.0, g, if active { t.accent } else { t.text_dim }), s.label(), right, active, false, t).clicked() {
            go = Some(s);
        }
    }
    ui.add_space(8.0);

    if go.is_some() || show.is_some() || new_account {
        // Phone layout: picking something closes the menu and shows it, not a transaction picked before.
        if app.ui.menu {
            app.ui.menu = false;
            app.select(None);
        }
    }
    if let Some(s) = go {
        if s == Screen::Transactions {
            app.ui.search.clear();
            app.run("view.filter", json!({ "replace": true }));
        } else {
            app.go(s);
        }
    }
    if let Some(a) = show {
        app.show_account(a);
    }
    if new_account {
        app.open_dialog("newAccount", json!({}));
    }
}
