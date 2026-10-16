//! The Transactions screen: a toolbar (what is shown, search, period, Review only) and the register, grouped by month.
//! Rows are virtualised, so large books scroll smoothly: only rows in view are laid out and painted.

use std::sync::Arc;

use chrono::Datelike;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::register::{self, Filter, Row};
use pecunia_engine::model::{Book, Money, NaiveDate, TxnId, reports};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::pickers::{self, Choices};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

const HEADER_H: f32 = 34.0;
const ROW_H: f32 = 46.0;

/// The register rows of the last frame, kept while the book and the filter stay the same.
#[derive(Default)]
pub struct Cache {
    key: Option<(u64, Filter)>,
    rows: Arc<Vec<Row>>,
}

impl Cache {
    pub fn rows(&mut self, app: &PecuniaApp) -> Arc<Vec<Row>> {
        let book = app.session.book();
        let key = (app.session.revision(), app.session.view.filter.clone());
        if self.key.as_ref() != Some(&key) {
            self.rows = Arc::new(register::rows(book, &book.tree(), &app.session.view.filter));
            self.key = Some(key);
        }
        self.rows.clone()
    }
}

thread_local! {
    static CACHE: std::cell::RefCell<Cache> = std::cell::RefCell::new(Cache::default());
}

/// The rows for the current filter (cached).
pub fn current_rows(app: &PecuniaApp) -> Arc<Vec<Row>> {
    CACHE.with(|c| c.borrow_mut().rows(app))
}

/// The transaction order on screen (for ↑/↓).
pub fn nav_order(app: &PecuniaApp) -> Vec<TxnId> {
    current_rows(app).iter().map(|r| r.txn.id).collect()
}

enum Line {
    Month { label: String, money_in: Money, money_out: Money },
    Txn(usize),
}

/// `October 2026`.
fn month_label(d: NaiveDate) -> String {
    d.format("%B %Y").to_string()
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    toolbar(app, ui, t);
    let rows = current_rows(app);
    if rows.is_empty() {
        empty(app, ui, t);
        return;
    }
    // Month headings with what came in and went out that month.
    let mut lines: Vec<Line> = Vec::new();
    let mut month: Option<(i32, u32)> = None;
    for (i, r) in rows.iter().enumerate() {
        let m = (r.txn.date.year(), r.txn.date.month());
        if month != Some(m) {
            month = Some(m);
            let (mut money_in, mut money_out) = (Money::ZERO, Money::ZERO);
            for x in rows.iter().skip(i).take_while(|x| (x.txn.date.year(), x.txn.date.month()) == m) {
                if x.amount.is_positive() {
                    money_in += x.amount;
                } else {
                    money_out += x.amount;
                }
            }
            lines.push(Line::Month { label: month_label(r.txn.date), money_in, money_out });
        }
        lines.push(Line::Txn(i));
    }
    let total: f32 = lines.iter().map(|l| if matches!(l, Line::Month { .. }) { HEADER_H } else { ROW_H }).sum::<f32>() + 24.0;
    let selected = app.session.selection;
    let scroll_to = std::mem::take(&mut app.ui.scroll_to_selection);
    let mut select: Option<Option<TxnId>> = None;
    let mut action: Option<(&'static str, Json)> = None;
    let book = app.session.book().clone();
    let in_category = app.session.view.filter.account.and_then(|a| book.account(a)).is_some_and(|a| !a.is_physical());
    let tree = book.tree();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let w = ui.available_width();
        let (area, _) = ui.allocate_exact_size(vec2(w, total), Sense::hover());
        let origin = area.min;
        let mut y = 0.0;
        for line in &lines {
            let h = if matches!(line, Line::Month { .. }) { HEADER_H } else { ROW_H };
            let rect = Rect::from_min_size(pos2(origin.x, origin.y + y), vec2(w, h));
            let visible = y + h >= viewport.top() - 2.0 * ROW_H && y <= viewport.bottom() + 2.0 * ROW_H;
            if let Line::Txn(i) = line
                && scroll_to
                && rows.get(*i).map(|r| r.txn.id) == selected
            {
                ui.scroll_to_rect(rect.expand(8.0), None);
            }
            if visible {
                match line {
                    Line::Month { label, money_in, money_out } => month_header(ui, rect, label, *money_in, *money_out, t),
                    Line::Txn(i) => {
                        if let Some(r) = rows.get(*i) {
                            let neutral = in_category
                                || (app.session.view.filter.account.is_none() && pecunia_engine::model::ledger::is_transfer(&tree, &r.txn));
                            let resp = txn_row(ui, rect, &book, r, Some(r.txn.id) == selected, neutral, t);
                            if resp.clicked() {
                                select = Some(if Some(r.txn.id) == selected { None } else { Some(r.txn.id) });
                            }
                            resp.context_menu(|ui| {
                                if let Some(a) = txn_menu(ui, &book, r.txn.id, t) {
                                    action = Some(a);
                                }
                            });
                        }
                    }
                }
            }
            y += h;
        }
    });
    if let Some(s) = select {
        app.select(s);
    }
    if let Some((id, p)) = action {
        app.run_any(id, p);
    }
}

fn month_header(ui: &mut Ui, rect: Rect, label: &str, money_in: Money, money_out: Money, t: &Tokens) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.sidebar);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let cy = rect.center().y;
    p.text(pos2(rect.left() + 20.0, cy), Align2::LEFT_CENTER, label, theme::bold(12.5), t.text);
    let mut x = rect.right() - 20.0;
    let out = p.layout_no_wrap(format!("out {}", money_out.abs().format()), theme::body(11.5), t.text_dim);
    x -= out.size().x;
    p.galley(pos2(x, cy - out.size().y / 2.0), out, t.text_dim);
    let inn = p.layout_no_wrap(format!("in {}", money_in.format()), theme::body(11.5), t.positive);
    x -= inn.size().x + 14.0;
    p.galley(pos2(x, cy - inn.size().y / 2.0), inn, t.positive);
}

fn txn_row(ui: &mut Ui, rect: Rect, book: &Book, r: &Row, selected: bool, neutral: bool, t: &Tokens) -> egui::Response {
    let txn = &r.txn;
    let resp = ui.interact(rect, ui.id().with(("txn", txn.id.0)), Sense::click());
    widgets::a11y(&resp, format!("{} {} {}", txn.date, txn.payee, r.amount.format()));
    let p = ui.painter().clone();
    if selected {
        p.rect_filled(rect, 0.0, t.selected);
        p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, t.accent);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.hover);
    }
    p.line_segment([pos2(rect.left() + 16.0, rect.bottom()), pos2(rect.right(), rect.bottom())], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
    let x = rect.left();
    let top = rect.top() + 15.0;
    let bottom = rect.top() + 32.0;
    match book.review_kind(txn) {
        Some(k) => icons::review(&p, pos2(x + 24.0, rect.center().y), 15.0, k, t),
        None => icons::status(&p, pos2(x + 24.0, rect.center().y), 14.0, txn.status, t),
    }
    p.text(pos2(x + 44.0, rect.center().y), Align2::LEFT_CENTER, txn.date.format("%-d %b").to_string(), theme::body(12.0), t.text_dim);

    // Right side: the amount, and under it the running balance.
    let right = rect.right() - 20.0;
    let amount_col = if r.amount.is_positive() && !neutral { t.positive } else { t.text };
    let ag = p.layout_no_wrap(r.amount.format(), theme::bold(13.5), amount_col);
    let amount_w = ag.size().x;
    p.galley(pos2(right - amount_w, top - ag.size().y / 2.0), ag, amount_col);
    if let Some(b) = r.balance {
        let bg = p.layout_no_wrap(b.format(), theme::body(11.5), t.text_faint);
        p.galley(pos2(right - bg.size().x, bottom - bg.size().y / 2.0), bg, t.text_faint);
    }
    let text_x = x + 100.0;
    let room = (right - 130.0 - text_x).max(60.0);
    let payee = widgets::elide(ui, &txn.payee, theme::body(13.5), room);
    p.text(pos2(text_x, top), Align2::LEFT_CENTER, payee, theme::body(13.5), t.text);
    let mut second = format!("{} → {}", r.from, r.to);
    if !txn.note.is_empty() {
        second.push_str(" · ");
        second.push_str(&txn.note);
    }
    let second = widgets::elide(ui, &second, theme::body(11.5), room);
    let unsorted = r.to.contains("Not sorted yet") || r.from.contains("Not sorted yet");
    p.text(pos2(text_x, bottom), Align2::LEFT_CENTER, second, theme::body(11.5), if unsorted { t.warning } else { t.text_dim });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The right-click menu of a row. Returns the command to run.
pub fn txn_menu(ui: &mut Ui, book: &Book, id: TxnId, t: &Tokens) -> Option<(&'static str, Json)> {
    let mut out = None;
    ui.set_min_width(210.0);
    ui.menu_button("Where did it go?", |ui| {
        if let Some(a) = pickers::menu(ui, book, Choices::Counter, None, t) {
            out = Some(("txn.answer", json!({ "id": id.0, "to": a.0 })));
        }
    });
    if book.txn(id).is_some_and(|x| book.review_kind(x).is_some()) && ui.button("Mark as checked").clicked() {
        out = Some(("txn.confirm", json!({ "id": id.0 })));
        ui.close();
    }
    if ui.button("Split…").clicked() {
        out = Some(("ui.split", json!({ "id": id.0 })));
        ui.close();
    }
    ui.separator();
    if ui.button(egui::RichText::new("Delete").color(t.danger)).clicked() {
        out = Some(("txn.delete", json!({ "id": id.0 })));
        ui.close();
    }
    out
}

/// What the register shows, in words.
pub fn title(app: &PecuniaApp) -> String {
    let b = app.session.book();
    let f = &app.session.view.filter;
    if let Some(a) = f.account {
        return b.account_label(a);
    }
    if let Some(i) = f.import.and_then(|i| b.meta.imports.iter().find(|x| x.id == i)) {
        return format!("Import: {}", i.file);
    }
    if f.review {
        return "Waiting in Review".into();
    }
    "All transactions".into()
}

fn toolbar(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let book = app.session.book().clone();
    let f = app.session.view.filter.clone();
    if let Some(a) = f.account.and_then(|a| book.account(a)) {
        icons::account(row.painter(), pos2(row.cursor().left() + 9.0, rect.center().y), 18.0, a.kind, a.group, t);
        row.add_space(24.0);
    }
    row.label(egui::RichText::new(title(app)).font(theme::bold(18.0)).color(t.text));
    let mut actions: Vec<(&'static str, Json)> = Vec::new();
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let te = egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.border_strong))
            .corner_radius(7)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    icons::glyph(ui.painter(), r.center(), 12.0, Glyph::Search, t.text_faint);
                    ui.add(egui::TextEdit::singleline(&mut app.ui.search).frame(egui::Frame::NONE).hint_text("Search  ⌘F").desired_width(200.0))
                })
                .inner
            })
            .inner;
        if app.ui.focus_search {
            te.request_focus();
            app.ui.focus_search = false;
        }
        if te.changed() {
            actions.push(("view.filter", json!({ "text": app.ui.search })));
        }
        if let Some(a) = f.account
            && book.account(a).is_some_and(|a| a.is_physical())
        {
            ui.add_space(8.0);
            if widgets::secondary_button(ui, "Edit account", t).clicked() {
                actions.push(("ui.editAccount", json!({ "id": a.0 })));
            }
        }
    });
    // Row 2: filters.
    let (rect2, _) = ui.allocate_exact_size(vec2(full_w, 40.0), Sense::hover());
    ui.painter().line_segment([rect2.left_bottom(), rect2.right_bottom()], Stroke::new(1.0, t.border));
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect2.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let today = app.session.today();
    let this_month = (reports::month_start(today), today);
    let last_month = reports::last_full_month(today);
    let this_year = (today.with_ordinal(1).unwrap_or(today), today);
    let periods: [(&str, Option<(NaiveDate, NaiveDate)>); 4] =
        [("All time", None), ("This month", Some(this_month)), ("Last month", Some(last_month)), ("This year", Some(this_year))];
    let current = match (f.from, f.to) {
        (None, None) => Some(0),
        (Some(a), Some(b)) => periods.iter().position(|(_, p)| *p == Some((a, b))),
        _ => None,
    };
    let label = current
        .and_then(|i| periods.get(i))
        .map(|(l, _)| l.to_string())
        .unwrap_or_else(|| format!("{} – {}", f.from.map(|d| d.to_string()).unwrap_or_default(), f.to.map(|d| d.to_string()).unwrap_or_default()));
    let r = widgets::pill_outlined(&mut row, |p, c| icons::glyph(p, c, 12.0, Glyph::Chart, t.text_dim), &label, false, t);
    egui::Popup::menu(&r).frame(widgets::popup_frame(t)).show(|ui| {
        for (i, (l, p)) in periods.iter().enumerate() {
            if widgets::menu_row(ui, |pa, c| icons::glyph(pa, c, 12.0, Glyph::Chart, t.text_faint), l, current == Some(i), t).clicked() {
                actions.push(("view.filter", json!({ "from": p.map(|x| x.0.to_string()), "to": p.map(|x| x.1.to_string()) })));
                ui.close();
            }
        }
    });
    if let Some(a) = pickers::pill(&mut row, &book, Choices::Any, f.account, "Any account", t) {
        actions.push(("view.filter", json!({ "account": a.0 })));
    }
    let on = f.review;
    let review = widgets::pill_outlined(
        &mut row,
        |p, c| icons::glyph(p, c, 12.0, Glyph::Inbox, if on { t.warning } else { t.text_faint }),
        "Waiting in Review",
        !on,
        t,
    );
    if review.clicked() {
        actions.push(("view.filter", json!({ "review": !on })));
    }
    if !f.is_empty() && widgets::icon_button(&mut row, Glyph::Close, "Clear filters", t).clicked() {
        app.ui.search.clear();
        actions.push(("view.clearFilter", json!({})));
    }
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let rows = current_rows(app);
        let n = rows.len();
        ui.label(egui::RichText::new(format!("{n} transaction{}", if n == 1 { "" } else { "s" })).size(12.0).color(t.text_dim));
    });
    for (id, p) in actions {
        app.run_any(id, p);
    }
}

fn empty(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let mut act = None;
    let none_at_all = app.session.book().txn_count() == 0;
    centered(ui, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        icons::glyph(ui.painter(), r.center(), 28.0, if none_at_all { Glyph::Import } else { Glyph::Search }, t.text_faint);
        ui.add_space(10.0);
        let (title, text) = if none_at_all {
            ("No transactions yet", "Import a bank file (CSV from your bank, CAMT.053 or MT940), or enter one by hand.")
        } else {
            ("Nothing matches", "Try another search, period or account.")
        };
        ui.label(egui::RichText::new(title).font(theme::bold(16.0)).color(t.text));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(text).size(13.0).color(t.text_dim));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if none_at_all {
                if widgets::primary_button(ui, "Import bank file", t).clicked() {
                    act = Some("import.file");
                }
                if widgets::secondary_button(ui, "New transaction", t).clicked() {
                    act = Some("txn.create");
                }
            } else if widgets::secondary_button(ui, "Clear filters", t).clicked() {
                act = Some("view.clearFilter");
            }
        });
    });
    if let Some(a) = act {
        if a == "view.clearFilter" {
            app.ui.search.clear();
        }
        app.run_any(a, json!({}));
    }
}

/// Lays out `add` in a narrow column in the middle of the free space (empty states).
pub fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let avail = ui.available_rect_before_wrap();
    let r = Rect::from_center_size(avail.center() - vec2(0.0, 40.0), vec2(avail.width().min(440.0), 260.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Center)));
    add(&mut child);
}
