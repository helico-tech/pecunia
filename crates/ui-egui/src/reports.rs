//! The Reports screen: income and spending, net worth, cash flow, fixed costs and the bank check, for a chosen period.
//! The numbers come from the pure report functions in the model, cached while the book, the report and the period stay
//! the same, so big books stay smooth. Every click goes through an engine command (`view.report`, `view.filter`, …).

use std::sync::Arc;

use chrono::{Datelike, Days, Months};
use egui::{Align2, Color32, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use pecunia_engine::model::reports::{
    self, AccountFlow, BankCheck, BankCheckStatus, CashFlow, CategoryAmount, FixedCosts, Frequency, IncomeExpenses, NetWorth, PhysicalBalance,
};
use pecunia_engine::model::{AccountId, AccountKind, Book, Group, Money, NaiveDate};
use pecunia_engine::view::ReportKind;
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

type Actions = Vec<(&'static str, Json)>;

/// The widest the report column gets; wider windows keep it readable.
const MAX_W: f32 = 1080.0;
/// Room for an amount column.
const AMOUNT_W: f32 = 108.0;

/// What a report shows, computed once per book revision, report and period.
enum Data {
    Income(IncomeExpenses),
    NetWorth(NetWorth),
    CashFlow(CashFlow),
    FixedCosts(FixedCosts),
    BankCheck(Vec<BankCheck>),
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    revision: u64,
    kind: ReportKind,
    from: NaiveDate,
    to: NaiveDate,
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<(Key, Arc<Data>)>> = const { std::cell::RefCell::new(None) };
}

fn data(app: &PecuniaApp, kind: ReportKind, from: NaiveDate, to: NaiveDate) -> Arc<Data> {
    let key = Key { revision: app.session.revision(), kind, from, to };
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, d)) = c.as_ref()
            && *k == key
        {
            return d.clone();
        }
        let book = app.session.book();
        let tree = book.tree();
        let txns: Vec<_> = book.txns().map(|t| t.as_ref()).collect();
        let d = Arc::new(match kind {
            ReportKind::IncomeExpenses => Data::Income(reports::income_and_expenses(&tree, &txns, from, to)),
            ReportKind::NetWorth => Data::NetWorth(reports::net_worth(&tree, &txns, to)),
            ReportKind::CashFlow => Data::CashFlow(reports::cash_flow(&tree, &txns, from, to)),
            ReportKind::FixedCosts => Data::FixedCosts(reports::fixed_costs(&tree, &txns, to)),
            ReportKind::BankCheck => Data::BankCheck(reports::bank_checks(&tree, &txns, &book.meta.balances)),
        });
        *c = Some((key, d.clone()));
        d
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Periods

/// The period on screen: the view's dates, else this month up to today.
fn period(app: &PecuniaApp) -> (NaiveDate, NaiveDate) {
    let today = app.session.today();
    let from = app.session.view.from.unwrap_or_else(|| reports::month_start(today));
    let to = app.session.view.to.unwrap_or(today).max(from);
    (from, to)
}

/// The quick choices, with their dates as of `today`.
fn presets(today: NaiveDate) -> Vec<(&'static str, NaiveDate, NaiveDate)> {
    let mut out = vec![("This month", reports::month_start(today), today)];
    let (lm_from, lm_to) = reports::last_full_month(today);
    out.push(("Last month", lm_from, lm_to));
    if let Some(jan) = NaiveDate::from_ymd_opt(today.year(), 1, 1) {
        out.push(("This year", jan, today));
    }
    if let (Some(a), Some(b)) = (NaiveDate::from_ymd_opt(today.year() - 1, 1, 1), NaiveDate::from_ymd_opt(today.year() - 1, 12, 31)) {
        out.push(("Last year", a, b));
    }
    if let Some(a) = reports::month_start(today).checked_sub_months(Months::new(11)) {
        out.push(("Last 12 months", a, today));
    }
    out
}

fn is_month_end(d: NaiveDate) -> bool {
    d == reports::month_end(d)
}

/// The period before (`back`) or after this one, as long as it: whole months step by their number of months (a year
/// by a year), anything else by its number of days. `None` when the next one would start after today.
fn step(from: NaiveDate, to: NaiveDate, back: bool, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let (nf, nt) = if from.day() == 1 && (is_month_end(to) || to == today) {
        let whole_year = from.month() == 1 && to.year() == from.year() && ((to.month() == 12 && to.day() == 31) || (to == today && to.month() > 1));
        let n = if whole_year { 12 } else { reports::months_between(from, to) + 1 };
        let n = u32::try_from(n).ok().filter(|n| *n > 0)?;
        let nf = if back { from.checked_sub_months(Months::new(n))? } else { from.checked_add_months(Months::new(n))? };
        (nf, reports::month_end(nf.checked_add_months(Months::new(n - 1))?))
    } else {
        let days = Days::new(u64::try_from((to - from).num_days() + 1).ok()?);
        if back { (from.checked_sub_days(days)?, to.checked_sub_days(days)?) } else { (from.checked_add_days(days)?, to.checked_add_days(days)?) }
    };
    if nf > today {
        return None;
    }
    Some((nf, nt.min(today)))
}

/// `16 Oct 2026`.
fn day(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// `1 – 16 Oct 2026`, `1 Nov 2025 – 16 Oct 2026`.
fn range_text(from: NaiveDate, to: NaiveDate) -> String {
    if from == to {
        day(to)
    } else if from.year() == to.year() && from.month() == to.month() {
        format!("{} – {}", from.day(), day(to))
    } else if from.year() == to.year() {
        format!("{} – {}", from.format("%-d %b"), day(to))
    } else {
        format!("{} – {}", day(from), day(to))
    }
}

/// The name of a period: a quick choice, a month, a year, or its dates.
fn period_label(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> String {
    if let Some((name, _, _)) = presets(today).into_iter().find(|(_, a, b)| *a == from && *b == to) {
        return name.to_string();
    }
    if from.day() == 1 && is_month_end(to) {
        if from.year() == to.year() && from.month() == to.month() {
            return from.format("%B %Y").to_string();
        }
        if from.year() == to.year() && from.month() == 1 && to.month() == 12 {
            return from.year().to_string();
        }
    }
    range_text(from, to)
}

// ---------------------------------------------------------------------------------------------------------------------
// The screen

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let mut actions: Actions = Vec::new();
    let kind = app.session.view.report;
    let today = app.session.today();
    let (from, to) = period(app);
    heading(ui, kind, from, to, today, t, &mut actions);
    tabs(ui, kind, t, &mut actions);

    let d = data(app, kind, from, to);
    let book = app.session.book().clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 20, top: 18, bottom: 28 }).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(MAX_W));
            ui.spacing_mut().item_spacing.y = 0.0;
            match d.as_ref() {
                Data::Income(r) => income(ui, r, t, &mut actions),
                Data::NetWorth(r) => net_worth(ui, r, t, &mut actions),
                Data::CashFlow(r) => cash_flow(ui, &book, r, t, &mut actions),
                Data::FixedCosts(r) => fixed_costs(ui, &book, r, t),
                Data::BankCheck(r) => bank_check(ui, &book, r, t, &mut actions),
            }
        });
    });
    for (id, p) in actions {
        app.run_any(id, p);
    }
}

/// The title row, with the period chooser on the right.
fn heading(ui: &mut Ui, kind: ReportKind, from: NaiveDate, to: NaiveDate, today: NaiveDate, t: &Tokens, actions: &mut Actions) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.label(egui::RichText::new("Reports").font(theme::bold(18.0)).color(t.text));
    if kind == ReportKind::BankCheck {
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new("The latest balances your bank stated").size(12.0).color(t.text_dim));
        });
        return;
    }
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let next = step(from, to, false, today);
        if arrow_button(ui, false, next.is_some(), "Next period", t).clicked()
            && let Some((a, b)) = next
        {
            actions.push(("view.report", json!({ "from": a.to_string(), "to": b.to_string() })));
        }
        let label = period_label(from, to, today);
        let r = widgets::pill_outlined(ui, |p, c| icons::glyph(p, c, 13.0, Glyph::Sprint, t.text_dim), &label, false, t);
        egui::Popup::menu(&r).frame(widgets::popup_frame(t)).show(|ui| {
            for (name, a, b) in presets(today) {
                let on = a == from && b == to;
                if widgets::menu_row(ui, |p, c| icons::glyph(p, c, 12.0, Glyph::Sprint, t.text_faint), name, on, t).clicked() {
                    // This month is the view's default: no dates, so it follows the calendar.
                    let p = if name == "This month" {
                        json!({ "from": null, "to": null })
                    } else {
                        json!({ "from": a.to_string(), "to": b.to_string() })
                    };
                    actions.push(("view.report", p));
                    ui.close();
                }
            }
        });
        if let Some((a, b)) = step(from, to, true, today)
            && arrow_button(ui, true, true, "Previous period", t).clicked()
        {
            actions.push(("view.report", json!({ "from": a.to_string(), "to": b.to_string() })));
        }
        ui.add_space(6.0);
        // What the dates are: the range, or the day a balance is taken on.
        let note = match kind {
            ReportKind::NetWorth => format!("On {}", day(to)),
            ReportKind::FixedCosts => format!("As of {}", day(to)),
            _ if label.chars().next().is_some_and(|c| c.is_ascii_digit()) => String::new(),
            _ => range_text(from, to),
        };
        ui.label(egui::RichText::new(note).size(12.0).color(t.text_dim));
    });
}

/// A previous or next arrow; dimmed and inert when there is nowhere to go.
fn arrow_button(ui: &mut Ui, back: bool, enabled: bool, tip: &str, t: &Tokens) -> Response {
    if !back {
        let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, t.hover);
        }
        let col = if !enabled {
            t.text_faint.gamma_multiply(0.5)
        } else if resp.hovered() {
            t.text
        } else {
            t.text_dim
        };
        icons::glyph(ui.painter(), rect.center(), 14.0, Glyph::ChevronRight, col);
        widgets::a11y(&resp, tip);
        return if enabled { resp.on_hover_text(tip) } else { resp };
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    // The mirror of the right chevron in `icons`.
    let (c, s) = (rect.center(), 7.0);
    let col = if resp.hovered() { t.text } else { t.text_dim };
    ui.painter().add(Shape::line(
        vec![pos2(c.x + s * 0.25, c.y - s * 0.55), pos2(c.x - s * 0.3, c.y), pos2(c.x + s * 0.25, c.y + s * 0.55)],
        Stroke::new((14.0f32 / 11.0).max(1.25), col),
    ));
    widgets::a11y(&resp, tip);
    resp.on_hover_text(tip)
}

fn tab_glyph(k: ReportKind) -> Glyph {
    match k {
        ReportKind::IncomeExpenses => Glyph::Chart,
        ReportKind::NetWorth => Glyph::Layers,
        ReportKind::CashFlow => Glyph::Swap,
        ReportKind::FixedCosts => Glyph::Sprint,
        ReportKind::BankCheck => Glyph::Check,
    }
}

/// The report tabs, like the Review tabs; they wrap when the window is narrow.
fn tabs(ui: &mut Ui, current: ReportKind, t: &Tokens, actions: &mut Actions) {
    ui.add_space(6.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 0)).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for k in ReportKind::ALL {
                let on = k == current;
                let r = widgets::pill_outlined(
                    ui,
                    |p, c| icons::glyph(p, c, 13.0, tab_glyph(k), if on { t.accent } else { t.text_dim }),
                    k.label(),
                    !on,
                    t,
                );
                if on {
                    ui.painter().rect_stroke(r.rect, 7.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
                }
                if r.clicked() && !on {
                    actions.push(("view.report", json!({ "report": k.id() })));
                }
            }
        });
    });
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.border));
}

// ---------------------------------------------------------------------------------------------------------------------
// Building blocks

/// A row of summary tiles: a label in small capitals and a big number.
fn tiles(ui: &mut Ui, items: &[(&str, String, Color32)], t: &Tokens) {
    let n = items.len().max(1) as f32;
    let gap = 12.0;
    let w = ui.available_width();
    let tile_w = ((w - gap * (n - 1.0)) / n).max(80.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 78.0), Sense::hover());
    let p = ui.painter();
    for (i, (label, value, col)) in items.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * (tile_w + gap), rect.top()), vec2(tile_w, rect.height()));
        p.rect(r, 10.0, t.surface, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.text(pos2(r.left() + 16.0, r.top() + 22.0), Align2::LEFT_CENTER, label.to_uppercase(), theme::bold(11.0), t.text_faint);
        let value = widgets::elide(ui, value, theme::bold(21.0), tile_w - 32.0);
        p.text(pos2(r.left() + 16.0, r.top() + 51.0), Align2::LEFT_CENTER, value, theme::bold(21.0), *col);
    }
    ui.add_space(14.0);
}

/// A card section filling the width.
fn card(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui)) {
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
    ui.add_space(14.0);
}

/// A card's title, with a total on the right.
fn card_title(ui: &mut Ui, title: &str, total: Option<Money>, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let p = ui.painter();
    p.text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, title, theme::bold(14.5), t.text);
    if let Some(m) = total {
        let g = p.layout_no_wrap(m.format(), theme::bold(14.5), t.text);
        p.galley(pos2(rect.right() - 6.0 - g.size().x, rect.center().y - g.size().y / 2.0), g, t.text);
    }
    ui.add_space(6.0);
}

/// Column headings in small capitals: the first on the left, the others right-aligned at their right edge.
fn columns(ui: &mut Ui, first: &str, cols: &[(&str, f32)], t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let p = ui.painter();
    let cy = rect.center().y;
    p.text(pos2(rect.left() + 6.0, cy), Align2::LEFT_CENTER, first.to_uppercase(), theme::bold(10.5), t.text_faint);
    for (label, right) in cols {
        p.text(pos2(rect.left() + right, cy), Align2::RIGHT_CENTER, label.to_uppercase(), theme::bold(10.5), t.text_faint);
    }
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
}

/// A line in a card: hover background when it can be clicked, a hairline under it.
fn row(ui: &mut Ui, h: f32, clickable: bool, label: String, line: bool, t: &Tokens) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), if clickable { Sense::click() } else { Sense::hover() });
    if clickable && resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    if line {
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
    }
    widgets::a11y(&resp, label);
    let resp = if clickable { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    (rect, resp)
}

/// An amount right-aligned at `x_right`, in `col`.
fn amount(p: &egui::Painter, x_right: f32, y: f32, m: Money, font: egui::FontId, col: Color32) {
    let g = p.layout_no_wrap(m.format(), font, col);
    p.galley(pos2(x_right - g.size().x, y - g.size().y / 2.0), g, col);
}

/// Text cut to fit and drawn left-aligned.
fn text(ui: &Ui, x: f32, y: f32, s: &str, font: egui::FontId, col: Color32, max: f32) {
    let s = widgets::elide(ui, s, font.clone(), max.max(20.0));
    ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, s, font, col);
}

/// A thin bar from 0 to 1 painted in `rect`.
fn bar_at(p: &egui::Painter, rect: Rect, fraction: f32, col: Color32, t: &Tokens) {
    p.rect_filled(rect, rect.height() / 2.0, t.border);
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    if f > 0.0 {
        p.rect_filled(Rect::from_min_size(rect.min, vec2((rect.width() * f).max(rect.height()), rect.height())), rect.height() / 2.0, col);
    }
}

/// A small rounded status label; returns its width.
fn chip(p: &egui::Painter, x_right: f32, y: f32, s: &str, g: Glyph, fg: Color32, bg: Color32) -> f32 {
    let galley = p.layout_no_wrap(s.to_string(), theme::bold(11.5), fg);
    let w = galley.size().x + 34.0;
    let r = Rect::from_min_size(pos2(x_right - w, y - 11.0), vec2(w, 22.0));
    p.rect_filled(r, 11.0, bg);
    icons::glyph(p, pos2(r.left() + 13.0, y), 11.0, g, fg);
    p.galley(pos2(r.left() + 24.0, y - galley.size().y / 2.0), galley, fg);
    w
}

/// A note under a report: a soft box with an icon, a sentence (wrapped to fit) and maybe a button.
fn note(ui: &mut Ui, g: Glyph, s: &str, warn: bool, button: Option<&str>, t: &Tokens) -> bool {
    let (fill, stroke, icon_col, text_col) =
        if warn { (t.warning_soft, t.warning.gamma_multiply(0.5), t.warning, t.text) } else { (t.sidebar, t.border, t.text_dim, t.text_dim) };
    let full = ui.available_width();
    let button_w = button.map_or(0.0, |b| ui.painter().layout_no_wrap(b.to_string(), theme::body(13.0), t.text).size().x + 24.0 + 14.0);
    let galley = ui.painter().layout(s.to_string(), theme::body(12.5), text_col, (full - 50.0 - 14.0 - button_w).max(80.0));
    let h = (galley.size().y + 22.0).max(if button.is_some() { 50.0 } else { 40.0 });
    let (rect, _) = ui.allocate_exact_size(vec2(full, h), Sense::hover());
    let p = ui.painter();
    p.rect(rect, 8.0, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    let text_top = rect.center().y - galley.size().y / 2.0;
    icons::glyph(p, pos2(rect.left() + 23.0, text_top + 9.0), 14.0, g, icon_col);
    p.galley(pos2(rect.left() + 42.0, text_top), galley, text_col);
    let mut clicked = false;
    if let Some(b) = button {
        let r = Rect::from_min_max(pos2(rect.right() - button_w, rect.top()), pos2(rect.right() - 12.0, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::right_to_left(egui::Align::Center)));
        clicked = widgets::secondary_button(&mut child, b, t).clicked();
    }
    ui.add_space(10.0);
    clicked
}

/// An empty card body.
fn empty_line(ui: &mut Ui, s: &str, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, s, theme::body(12.5), t.text_faint);
}

fn signed_col(m: Money, t: &Tokens) -> Color32 {
    if m.is_negative() {
        t.danger
    } else if m.is_positive() {
        t.positive
    } else {
        t.text
    }
}

fn account_kind(book: &Book, id: AccountId) -> (AccountKind, Group) {
    book.account(id).map_or((AccountKind::Current, Group::Have), |a| (a.kind, a.group))
}

/// The register of one account for a period.
fn register_of(id: AccountId, from: NaiveDate, to: NaiveDate) -> (&'static str, Json) {
    ("view.filter", json!({ "replace": true, "account": id.0, "from": from.to_string(), "to": to.to_string() }))
}

// ---------------------------------------------------------------------------------------------------------------------
// Income and spending

fn income(ui: &mut Ui, r: &IncomeExpenses, t: &Tokens, actions: &mut Actions) {
    tiles(
        ui,
        &[
            ("Income", r.total_income.format(), t.positive),
            ("Spending", r.total_expenses.format(), t.text),
            ("Left over", r.left_over.format(), signed_col(r.left_over, t)),
        ],
        t,
    );
    if r.not_sorted_in.is_positive() || r.not_sorted_out.is_positive() {
        let parts: Vec<String> = [(r.not_sorted_out, "out"), (r.not_sorted_in, "in")]
            .into_iter()
            .filter(|(m, _)| m.is_positive())
            .map(|(m, w)| format!("{} {w}", m.format()))
            .collect();
        let verb = if parts.len() > 1 { "are" } else { "is" };
        let s = format!("{} {verb} not sorted yet, so not in these totals.", parts.join(" and "));
        if note(ui, Glyph::Inbox, &s, true, Some("Sort in Review"), t) {
            actions.push(("view.review", json!({})));
        }
    }
    if r.loan_repayments.is_positive() {
        let s = format!("You also paid off {} on loans. That lowers what you owe, so it is not spending.", r.loan_repayments.format());
        note(ui, Glyph::Arrow, &s, false, None, t);
    }
    ui.add_space(4.0);
    let sides = [
        ("Income", &r.income_groups, &r.income, r.total_income, Group::Comes, "No income in this period."),
        ("Spending", &r.expense_groups, &r.expenses, r.total_expenses, Group::Goes, "No spending in this period."),
    ];
    for (title, groups, cats, total, group, none) in sides {
        card(ui, t, |ui| {
            card_title(ui, title, Some(total), t);
            if groups.is_empty() {
                empty_line(ui, none, t);
                return;
            }
            let col = t.group(group);
            for (i, g) in groups.iter().enumerate() {
                let children: Vec<&CategoryAmount> = cats.iter().filter(|c| c.category_group == Some(g.account)).collect();
                if i > 0 {
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
                    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
                }
                if share_row(ui, &g.name, g.amount, total, col, false, t).clicked() {
                    actions.push(register_of(g.account, r.from, r.to));
                }
                for c in &children {
                    if share_row(ui, &c.name, c.amount, total, col.gamma_multiply(0.55), true, t).clicked() {
                        actions.push(register_of(c.account, r.from, r.to));
                    }
                }
            }
        });
    }
}

/// A category (or group) with its share of the total as a bar.
fn share_row(ui: &mut Ui, name: &str, m: Money, total: Money, col: Color32, child: bool, t: &Tokens) -> Response {
    let h = if child { 30.0 } else { 36.0 };
    let (rect, resp) = row(ui, h, true, format!("{name} {}", m.format()), false, t);
    let share = if total.is_positive() { m.cents() as f64 / total.cents() as f64 } else { 0.0 } as f32;
    let cy = rect.center().y;
    let right = rect.right() - 6.0;
    let pct_right = right - AMOUNT_W;
    let bar_w = (rect.width() * 0.26).clamp(56.0, 240.0);
    let bar_right = pct_right - 50.0;
    let bar_left = bar_right - bar_w;
    let p = ui.painter();
    let (font, name_col) = if child { (theme::body(13.0), t.text_dim) } else { (theme::bold(13.5), t.text) };
    let x = rect.left() + if child { 26.0 } else { 6.0 };
    text(ui, x, cy, name, font.clone(), name_col, bar_left - 12.0 - x);
    bar_at(
        p,
        Rect::from_min_max(pos2(bar_left, cy - if child { 2.5 } else { 3.5 }), pos2(bar_right, cy + if child { 2.5 } else { 3.5 })),
        share,
        col,
        t,
    );
    let pct = if share > 0.0 && share < 0.01 { "<1%".to_string() } else { format!("{:.0}%", (share * 100.0).max(0.0)) };
    p.text(pos2(pct_right - 8.0, cy), Align2::RIGHT_CENTER, pct, theme::body(11.5), t.text_faint);
    amount(p, right, cy, m, font, if child { t.text_dim } else { t.text });
    resp
}

// ---------------------------------------------------------------------------------------------------------------------
// Net worth

fn net_worth(ui: &mut Ui, r: &NetWorth, t: &Tokens, actions: &mut Actions) {
    tiles(
        ui,
        &[
            ("You have", r.total_have.format(), t.text),
            ("You owe", r.total_owe.format(), t.text),
            ("Net worth", r.net_worth.format(), signed_col(r.net_worth, t)),
        ],
        t,
    );
    for (title, lines, total, group) in [("Money you have", &r.have, r.total_have, Group::Have), ("Money you owe", &r.owe, r.total_owe, Group::Owe)] {
        card(ui, t, |ui| {
            card_title(ui, title, Some(total), t);
            if lines.is_empty() {
                empty_line(ui, "No accounts.", t);
                return;
            }
            for (i, a) in lines.iter().enumerate() {
                if let Some(id) = balance_rows(ui, a, group, i + 1 < lines.len(), t) {
                    actions.push(("view.filter", json!({ "replace": true, "account": id.0 })));
                }
            }
        });
    }
    let s = "Net worth is what you have minus what you owe, at the end of that day. Pots are parts of their account, not extra money.";
    note(ui, Glyph::Layers, s, false, None, t);
}

/// An account with its balance and, under it, its pots. Returns the account or pot that was clicked.
fn balance_rows(ui: &mut Ui, a: &PhysicalBalance, group: Group, line: bool, t: &Tokens) -> Option<AccountId> {
    let mut clicked = None;
    let has_pots = a.pots.iter().any(|p| !p.unassigned);
    let (rect, resp) = row(ui, 44.0, true, format!("{} {}", a.name, a.balance.format()), line && !has_pots, t);
    let cy = rect.center().y;
    let right = rect.right() - 6.0;
    icons::account(ui.painter(), pos2(rect.left() + 16.0, cy), 20.0, a.kind, group, t);
    let room = right - AMOUNT_W - rect.left() - 40.0;
    if a.in_pots.is_positive() {
        text(ui, rect.left() + 36.0, cy - 8.0, &a.name, theme::body(13.5), t.text, room);
        text(ui, rect.left() + 36.0, cy + 9.0, &format!("{} of it in pots", a.in_pots.format()), theme::body(11.5), t.text_dim, room);
    } else {
        text(ui, rect.left() + 36.0, cy, &a.name, theme::body(13.5), t.text, room);
    }
    amount(ui.painter(), right, cy, a.balance, theme::bold(13.5), t.text);
    if resp.clicked() {
        clicked = Some(a.account);
    }
    if has_pots {
        for (i, p) in a.pots.iter().enumerate() {
            let last = i + 1 == a.pots.len();
            let (rect, resp) = row(ui, 30.0, true, format!("{} {}", p.name, p.balance.format()), false, t);
            let cy = rect.center().y;
            icons::account(ui.painter(), pos2(rect.left() + 44.0, cy), 15.0, AccountKind::Pot, group, t);
            let name_room = (rect.width() * 0.3).max(90.0);
            text(ui, rect.left() + 60.0, cy, &p.name, theme::body(12.5), t.text_dim, name_room);
            if let Some(g) = &p.goal {
                let x = rect.left() + 66.0 + name_room;
                let bar_w = (right - AMOUNT_W - x - 150.0).clamp(0.0, 160.0);
                if bar_w >= 40.0 {
                    bar_at(
                        ui.painter(),
                        Rect::from_min_max(pos2(x, cy - 2.5), pos2(x + bar_w, cy + 2.5)),
                        g.percent as f32 / 100.0,
                        t.group(group),
                        t,
                    );
                }
                let what = if g.reached { format!("goal {} reached", g.goal.format()) } else { format!("{}% of {}", g.percent, g.goal.format()) };
                let tx = if bar_w >= 40.0 { x + bar_w + 10.0 } else { x };
                text(ui, tx, cy, &what, theme::body(11.5), t.text_faint, right - AMOUNT_W - tx);
            }
            amount(ui.painter(), right, cy, p.balance, theme::body(12.5), t.text_dim);
            if resp.clicked() {
                clicked = Some(p.account);
            }
            if last && line {
                ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
            }
        }
        if !line {
            ui.add_space(2.0);
        }
    }
    clicked
}

// ---------------------------------------------------------------------------------------------------------------------
// Cash flow

fn cash_flow(ui: &mut Ui, book: &Book, r: &CashFlow, t: &Tokens, actions: &mut Actions) {
    // Money you have at the start and the end: transfers between own accounts cancel out here.
    let have = |f: fn(&AccountFlow) -> Money| r.accounts.iter().filter(|a| a.group == Group::Have).map(f).sum::<Money>();
    let (start, end) = (have(|a| a.opening), have(|a| a.closing));
    tiles(
        ui,
        &[
            (&format!("You had on {}", r.from.format("%-d %b")), start.format(), t.text),
            (&format!("You have on {}", r.to.format("%-d %b")), end.format(), t.text),
            ("Change", (end - start).format(), signed_col(end - start, t)),
        ],
        t,
    );
    for (title, group) in [("Money you have", Group::Have), ("Money you owe", Group::Owe)] {
        let lines: Vec<&AccountFlow> = r
            .accounts
            .iter()
            .filter(|a| a.group == group && !(a.opening.is_zero() && a.money_in.is_zero() && a.money_out.is_zero() && a.closing.is_zero()))
            .collect();
        if lines.is_empty() {
            continue;
        }
        card(ui, t, |ui| {
            card_title(ui, title, None, t);
            let w = ui.available_width();
            let col_w = ((w - 190.0) / 4.0).clamp(84.0, 130.0);
            let rights: Vec<f32> = (0..4).map(|i| w - 6.0 - (3 - i) as f32 * col_w).collect();
            let names = ["Opening", "In", "Out", "Closing"];
            let cols: Vec<(&str, f32)> = names.iter().copied().zip(rights.iter().copied()).collect();
            columns(ui, "Account", &cols, t);
            for a in &lines {
                let (rect, resp) = row(ui, 40.0, true, format!("{} in {} out {}", a.name, a.money_in.format(), a.money_out.format()), true, t);
                let cy = rect.center().y;
                let (kind, g) = account_kind(book, a.account);
                icons::account(ui.painter(), pos2(rect.left() + 16.0, cy), 20.0, kind, g, t);
                let first = rights.first().copied().unwrap_or(w);
                text(ui, rect.left() + 36.0, cy, &a.name, theme::body(13.5), t.text, first - col_w - 36.0 - 8.0);
                flow_amounts(ui, rect, &rights, [a.opening, a.money_in, -a.money_out, a.closing], false, t);
                if resp.clicked() {
                    actions.push(register_of(a.account, r.from, r.to));
                }
            }
            let sum = |f: fn(&&AccountFlow) -> Money| lines.iter().map(f).sum::<Money>();
            let (rect, _) = row(ui, 38.0, false, format!("{title} total"), false, t);
            ui.painter().text(pos2(rect.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, "Total", theme::bold(13.0), t.text);
            let totals = [sum(|a| a.opening), sum(|a| a.money_in), -sum(|a| a.money_out), sum(|a| a.closing)];
            flow_amounts(ui, rect, &rights, totals, true, t);
        });
    }
    let s = "In and out count money into and out of each account, transfers between your own accounts too. Paying off a card is money into the card.";
    note(ui, Glyph::Swap, s, false, None, t);
}

fn flow_amounts(ui: &Ui, rect: Rect, rights: &[f32], values: [Money; 4], bold: bool, t: &Tokens) {
    let p = ui.painter();
    for (i, (x, m)) in rights.iter().zip(values).enumerate() {
        let col = if m.is_zero() {
            t.text_faint
        } else if i == 1 {
            t.positive
        } else if i == 0 {
            t.text_dim
        } else {
            t.text
        };
        let font = if bold || i == 3 { theme::bold(13.0) } else { theme::body(13.0) };
        amount(p, rect.left() + x, rect.center().y, m, font, col);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Fixed costs

fn frequency(f: Frequency) -> String {
    match f {
        Frequency::Monthly => "Monthly".into(),
        Frequency::TimesAYear { times } => format!("{times}× a year"),
    }
}

fn fixed_costs(ui: &mut Ui, book: &Book, r: &FixedCosts, t: &Tokens) {
    let n = r.items.len();
    tiles(
        ui,
        &[
            ("Per month", r.total_per_month.format(), t.text),
            ("Per year", r.total_per_year.format(), t.text),
            ("Repeating costs", n.to_string(), t.text),
        ],
        t,
    );
    card(ui, t, |ui| {
        card_title(ui, "Repeating costs", None, t);
        if r.items.is_empty() {
            empty_line(ui, "None found yet. A payment counts after three payments to the same payee about a month apart.", t);
            return;
        }
        let w = ui.available_width();
        let next_w = 76.0;
        let col_w = ((w - 260.0 - next_w) / 3.0).clamp(80.0, 120.0);
        let next_r = w - 6.0;
        let pm_r = next_r - next_w;
        let am_r = pm_r - col_w;
        let how_r = am_r - col_w;
        columns(ui, "Payee", &[("How often", how_r), ("Amount", am_r), ("Per month", pm_r), ("Next", next_r)], t);
        for (i, c) in r.items.iter().enumerate() {
            let (rect, _) = row(ui, 46.0, false, format!("{} {} per month", c.name, c.per_month.format()), i + 1 < n, t);
            let cy = rect.center().y;
            let room = rect.left() + how_r - col_w - rect.left() - 6.0;
            let mut what: Vec<String> = c.counters.iter().map(|a| book.account_label(*a)).collect();
            if c.parts > 1 {
                what.push(format!("{} payments", c.parts));
            }
            text(ui, rect.left() + 6.0, cy - 8.0, &c.name, theme::body(13.5), t.text, room);
            let unsorted = what.iter().any(|w| w.contains("Not sorted yet"));
            text(ui, rect.left() + 6.0, cy + 9.0, &what.join(" · "), theme::body(11.5), if unsorted { t.warning } else { t.text_dim }, room);
            let p = ui.painter();
            p.text(pos2(rect.left() + how_r, cy), Align2::RIGHT_CENTER, frequency(c.frequency), theme::body(12.5), t.text_dim);
            amount(p, rect.left() + am_r, cy, c.amount, theme::body(13.0), t.text_dim);
            amount(p, rect.left() + pm_r, cy, c.per_month, theme::bold(13.0), t.text);
            p.text(pos2(rect.left() + next_r, cy), Align2::RIGHT_CENTER, c.next_date.format("%-d %b").to_string(), theme::body(12.5), t.text_dim);
        }
        let (rect, _) = row(ui, 38.0, false, "Total".into(), false, t);
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.border));
        let p = ui.painter();
        p.text(pos2(rect.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, "Total", theme::bold(13.0), t.text);
        p.text(
            pos2(rect.left() + how_r, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{} a year", r.total_per_year.format()),
            theme::body(12.5),
            t.text_dim,
        );
        amount(p, rect.left() + pm_r, rect.center().y, r.total_per_month, theme::bold(13.0), t.text);
    });
    if !r.price_changes.is_empty() {
        card(ui, t, |ui| {
            card_title(ui, "Price changes", None, t);
            let n = r.price_changes.len();
            for (i, c) in r.price_changes.iter().enumerate() {
                let (rect, _) = row(ui, 44.0, false, format!("{} {} to {}", c.name, c.old_amount.format(), c.new_amount.format()), i + 1 < n, t);
                let cy = rect.center().y;
                let more = c.per_year_more;
                let col = if more.is_positive() { t.danger } else { t.positive };
                let right = rect.right() - 6.0;
                let label = format!("{}{} a year", if more.is_positive() { "+" } else { "" }, more.format());
                let lw = ui.painter().layout_no_wrap(label.clone(), theme::bold(13.0), col).size().x;
                ui.painter().text(pos2(right, cy), Align2::RIGHT_CENTER, label, theme::bold(13.0), col);
                let room = right - lw - 16.0 - rect.left() - 6.0;
                text(ui, rect.left() + 6.0, cy - 8.0, &c.name, theme::body(13.5), t.text, room);
                let s = format!("{} → {} since {}", c.old_amount.format(), c.new_amount.format(), day(c.since));
                text(ui, rect.left() + 6.0, cy + 9.0, &s, theme::body(11.5), t.text_dim, room);
            }
        });
    }
    if !r.new.is_empty() {
        card(ui, t, |ui| {
            card_title(ui, "New, not counted yet", None, t);
            let n = r.new.len();
            for (i, c) in r.new.iter().enumerate() {
                let (rect, _) = row(ui, 44.0, false, format!("{} {}", c.name, c.amount.format()), i + 1 < n, t);
                let cy = rect.center().y;
                let right = rect.right() - 6.0;
                let room = right - AMOUNT_W - rect.left() - 6.0;
                text(ui, rect.left() + 6.0, cy - 8.0, &c.name, theme::body(13.5), t.text, room);
                let s = format!("Around day {} · {} months in a row · next {}", c.day_of_month, c.months_in_a_row, day(c.next_date));
                text(ui, rect.left() + 6.0, cy + 9.0, &s, theme::body(11.5), t.text_dim, room);
                amount(ui.painter(), right, cy, c.amount, theme::bold(13.0), t.text);
            }
        });
    }
    if !r.stopped.is_empty() {
        card(ui, t, |ui| {
            card_title(ui, "Stopped", None, t);
            let n = r.stopped.len();
            for (i, c) in r.stopped.iter().enumerate() {
                let (rect, _) = row(ui, 44.0, false, format!("{} {}", c.name, c.amount.format()), i + 1 < n, t);
                let cy = rect.center().y;
                let right = rect.right() - 6.0;
                let room = right - AMOUNT_W - rect.left() - 6.0;
                text(ui, rect.left() + 6.0, cy - 8.0, &c.name, theme::body(13.5), t.text_dim, room);
                let s = format!("Last paid {} · expected {}", day(c.last_date), day(c.expected));
                text(ui, rect.left() + 6.0, cy + 9.0, &s, theme::body(11.5), t.text_faint, room);
                amount(ui.painter(), right, cy, c.amount, theme::body(13.0), t.text_faint);
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Bank check

fn bank_check(ui: &mut Ui, book: &Book, checks: &[BankCheck], t: &Tokens, actions: &mut Actions) {
    if book.meta.balances.is_empty() || checks.is_empty() {
        card(ui, t, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                icons::glyph(ui.painter(), r.center(), 26.0, Glyph::Check, t.text_faint);
                ui.add_space(8.0);
                ui.label(egui::RichText::new("No balances from your bank yet").font(theme::bold(16.0)).color(t.text));
                ui.add_space(6.0);
                ui.scope(|ui| {
                    ui.set_max_width(460.0);
                    ui.label(
                        egui::RichText::new(
                            "The bank check compares the books with the balance your bank states in its files. Only MT940 and CAMT.053 files \
                             state one; CSV exports do not. Import one of those to check your accounts.",
                        )
                        .size(13.0)
                        .color(t.text_dim),
                    );
                });
                ui.add_space(14.0);
                if widgets::primary_button(ui, "Import bank file", t).clicked() {
                    actions.push(("import.file", json!({})));
                }
                ui.add_space(10.0);
            });
        });
        return;
    }
    let off = checks.iter().filter(|c| matches!(c.status, BankCheckStatus::Differs { .. })).count();
    let synced = checks.iter().filter(|c| matches!(c.status, BankCheckStatus::InSync { .. })).count();
    tiles(
        ui,
        &[
            ("In sync", synced.to_string(), if synced > 0 { t.positive } else { t.text }),
            ("Differs", off.to_string(), if off > 0 { t.danger } else { t.text }),
        ],
        t,
    );
    card(ui, t, |ui| {
        card_title(ui, "Accounts", None, t);
        let n = checks.len();
        for (i, c) in checks.iter().enumerate() {
            let (rect, resp) = row(ui, 48.0, true, format!("{} {}", c.name, status_word(&c.status)), i + 1 < n, t);
            let cy = rect.center().y;
            let right = rect.right() - 6.0;
            let (kind, g) = account_kind(book, c.account);
            icons::account(ui.painter(), pos2(rect.left() + 16.0, cy), 20.0, kind, g, t);
            let (chip_w, line) = match &c.status {
                BankCheckStatus::InSync { date, balance } => (
                    chip(ui.painter(), right, cy, "In sync", Glyph::Check, t.positive, t.positive.gamma_multiply(0.14)),
                    format!("The bank said {} on {}, and so do the books", balance.format(), day(*date)),
                ),
                BankCheckStatus::Differs { by, since, date, stated, books } => (
                    chip(ui.painter(), right, cy, &format!("Differs {}", by.format()), Glyph::Close, t.danger, t.danger_soft),
                    format!("Bank {} and books {} on {} · off since {}", stated.format(), books.format(), day(*date), day(*since)),
                ),
                BankCheckStatus::NoStatement => (0.0, "The bank never stated a balance".into()),
            };
            let room = right - chip_w - 16.0 - rect.left() - 36.0;
            text(ui, rect.left() + 36.0, cy - 8.0, &c.name, theme::body(13.5), t.text, room);
            text(ui, rect.left() + 36.0, cy + 9.0, &line, theme::body(11.5), t.text_dim, room);
            if resp.clicked() {
                actions.push(("view.filter", json!({ "replace": true, "account": c.account.0 })));
            }
        }
    });
    let unchecked: Vec<String> =
        book.tree().all_physical().into_iter().filter(|a| !a.closed && !checks.iter().any(|c| c.account == a.id)).map(|a| a.name.clone()).collect();
    if !unchecked.is_empty() {
        let s = format!(
            "No balance from the bank for {}. Only MT940 and CAMT.053 files state one; CSV exports and accounts you keep by hand do not.",
            unchecked.join(", ")
        );
        note(ui, Glyph::Inbox, &s, false, None, t);
    }
    if off > 0 {
        note(
            ui,
            Glyph::Search,
            "A difference usually means a missing or doubled transaction after the last date the books and the bank agreed. Open the account to look.",
            false,
            None,
            t,
        );
    }
}

fn status_word(s: &BankCheckStatus) -> String {
    match s {
        BankCheckStatus::InSync { .. } => "in sync".into(),
        BankCheckStatus::Differs { by, .. } => format!("differs by {}", by.format()),
        BankCheckStatus::NoStatement => "no statement".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap_or_default()
    }

    #[test]
    fn periods_step_by_their_own_length() {
        let today = d(2026, 10, 16);
        // This month back is September; forward from September is this month up to today.
        assert_eq!(step(d(2026, 10, 1), today, true, today), Some((d(2026, 9, 1), d(2026, 9, 30))));
        assert_eq!(step(d(2026, 9, 1), d(2026, 9, 30), false, today), Some((d(2026, 10, 1), today)));
        assert_eq!(step(d(2026, 10, 1), today, false, today), None);
        // A year steps by a year, the last 12 months by 12 months.
        assert_eq!(step(d(2026, 1, 1), today, true, today), Some((d(2025, 1, 1), d(2025, 12, 31))));
        assert_eq!(step(d(2025, 11, 1), today, true, today), Some((d(2024, 11, 1), d(2025, 10, 31))));
        // Any other range by its days.
        assert_eq!(step(d(2026, 3, 5), d(2026, 3, 11), true, today), Some((d(2026, 2, 26), d(2026, 3, 4))));
        assert_eq!(period_label(d(2026, 9, 1), d(2026, 9, 30), today), "Last month");
        assert_eq!(period_label(d(2026, 8, 1), d(2026, 8, 31), today), "August 2026");
        assert_eq!(period_label(d(2024, 1, 1), d(2024, 12, 31), today), "2024");
    }
}
