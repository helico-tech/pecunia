//! The Overview screen: net worth, free to spend, this month and last, bank and cash over the last year, the biggest
//! spending of last month, and what needs attention. The reports are pure model functions; their results are cached
//! while the book and the date stay the same, so big books stay smooth.

use std::rc::Rc;

use chrono::Datelike;
use egui::{Align2, Color32, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use pecunia_engine::model::reports::{self, FreeToSpend, IncomeExpenses, MonthEnd, NeedsAttention, NetWorth, PhysicalBalance};
use pecunia_engine::model::{AccountId, AccountKind, Book, Group, Money, NaiveDate, Txn};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

/// Wider than this, the tiles stand in two columns.
const WIDE: f32 = 900.0;
const GAP: f32 = 16.0;
const PAD: f32 = 20.0;
const LIST_ROW_H: f32 = 30.0;
const ATTENTION_ROW_H: f32 = 48.0;
const CHART_H: f32 = 190.0;
/// Categories in the "biggest spending" tile.
const TOP_CATEGORIES: usize = 6;
/// Items in the "needs attention" tile before "and n more".
const ATTENTION_MAX: usize = 8;

/// Everything the Overview shows, computed once per book revision and day.
struct Data {
    today: NaiveDate,
    net_worth: NetWorth,
    free: FreeToSpend,
    this_month: IncomeExpenses,
    last_month: IncomeExpenses,
    month_ends: Vec<MonthEnd>,
    attention: NeedsAttention,
}

impl Data {
    fn compute(book: &Book, today: NaiveDate) -> Data {
        let tree = book.tree();
        let txns: Vec<&Txn> = book.txns().map(|t| t.as_ref()).collect();
        let (lm_from, lm_to) = reports::last_full_month(today);
        let year_ago = today.checked_sub_months(chrono::Months::new(11)).map(reports::month_start).unwrap_or(today);
        Data {
            today,
            net_worth: reports::net_worth(&tree, &txns, today),
            free: reports::free_to_spend(&tree, &txns, today),
            this_month: reports::income_and_expenses(&tree, &txns, reports::month_start(today), today),
            last_month: reports::income_and_expenses(&tree, &txns, lm_from, lm_to),
            month_ends: reports::bank_and_cash_over_time(&tree, &txns, year_ago, today),
            attention: reports::needs_attention(book, today),
        }
    }
}

type Key = (u64, NaiveDate, usize);

thread_local! {
    static CACHE: std::cell::RefCell<Option<(Key, Rc<Data>)>> = const { std::cell::RefCell::new(None) };
}

fn data(app: &PecuniaApp) -> Rc<Data> {
    let book = app.session.book();
    let today = app.session.today();
    let key: Key = (app.session.revision(), today, book as *const Book as usize);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_ref() {
            Some((k, d)) if *k == key => d.clone(),
            _ => {
                let d = Rc::new(Data::compute(book, today));
                *c = Some((key, d.clone()));
                d
            }
        }
    })
}

/// What a click on the Overview asks for; run after drawing.
enum Act {
    Run(&'static str, Json),
    Show(AccountId),
    /// The register with this filter (replaces the current one).
    Register(Json),
    Dialog(&'static str, Json),
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let d = data(app);
    let mut acts: Vec<Act> = Vec::new();
    title_row(ui, &d, t);
    if app.session.book().txn_count() == 0 && d.net_worth.have.is_empty() && d.net_worth.owe.is_empty() {
        empty(ui, t, &mut acts);
        run(app, acts);
        return;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(PAD as i8)).show(ui, |ui| {
            let w = ui.available_width();
            let tiles: [&dyn Fn(&mut Ui, &mut Vec<Act>); 6] = [
                &|ui, a| net_worth_tile(ui, &d, t, a),
                &|ui, a| free_tile(ui, &d, t, a),
                &|ui, a| month_tile(ui, &d, t, a),
                &|ui, a| attention_tile(ui, &d, t, a),
                &|ui, _| chart_tile(ui, &d, t),
                &|ui, a| spending_tile(ui, &d, t, a),
            ];
            if w >= WIDE {
                let col = ((w - GAP) / 2.0).floor();
                for (i, pair) in tiles.chunks(2).enumerate() {
                    tile_row(ui, ("overview-row", i), col, pair, t, &mut acts);
                    ui.add_space(GAP);
                }
            } else {
                // One column: what needs a look comes right after the money you have.
                for i in [0usize, 1, 3, 2, 4, 5] {
                    if let Some(tile) = tiles.get(i) {
                        tile_row(ui, ("overview-row-narrow", i), w, std::slice::from_ref(tile), t, &mut acts);
                        ui.add_space(GAP);
                    }
                }
            }
        });
    });

    run(app, acts);
}

fn run(app: &mut PecuniaApp, acts: Vec<Act>) {
    for a in acts {
        match a {
            Act::Run(id, p) => {
                app.run_any(id, p);
            }
            Act::Show(id) => app.show_account(id),
            Act::Register(p) => {
                app.ui.search.clear();
                app.run("view.filter", p);
            }
            Act::Dialog(name, p) => app.open_dialog(name, p),
        }
    }
}

/// A new, empty book: where to start.
fn empty(ui: &mut Ui, t: &Tokens, acts: &mut Vec<Act>) {
    crate::register::centered(ui, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        icons::glyph(ui.painter(), r.center(), 28.0, Glyph::Grid, t.text_faint);
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Nothing to show yet").font(theme::bold(16.0)).color(t.text));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Add your bank accounts, then import a bank file. Your net worth, spending and balances show up here.")
                .size(13.0)
                .color(t.text_dim),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            // Centre the two buttons (a horizontal row starts on the left).
            let width = |text: &str, font| ui.painter().layout_no_wrap(text.to_string(), font, t.text).size().x + 24.0;
            let both = width("New account", theme::bold(13.0)) + width("Import bank file", theme::body(13.0)) + ui.spacing().item_spacing.x;
            ui.add_space(((ui.available_width() - both) / 2.0).max(0.0));
            if widgets::primary_button(ui, "New account", t).clicked() {
                acts.push(Act::Dialog("newAccount", json!({})));
            }
            if widgets::secondary_button(ui, "Import bank file", t).clicked() {
                acts.push(Act::Run("import.file", json!({})));
            }
        });
    });
}

fn title_row(ui: &mut Ui, d: &Data, t: &Tokens) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    let p = ui.painter();
    p.text(pos2(rect.left() + 18.0, rect.center().y), Align2::LEFT_CENTER, "Overview", theme::bold(18.0), t.text);
    p.text(
        pos2(rect.right() - 20.0, rect.center().y),
        Align2::RIGHT_CENTER,
        d.today.format("%A %-d %B %Y").to_string(),
        theme::body(12.5),
        t.text_dim,
    );
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
}

/// Tiles side by side with the same height: each row remembers the tallest tile of the last frame.
fn tile_row(ui: &mut Ui, key: (&str, usize), width: f32, tiles: &[&dyn Fn(&mut Ui, &mut Vec<Act>)], t: &Tokens, acts: &mut Vec<Act>) {
    let id = ui.id().with(key);
    let min_h = ui.data(|m| m.get_temp::<f32>(id)).unwrap_or(0.0);
    let mut tallest: f32 = 0.0;
    let mut row_h: f32 = 0.0;
    let origin = ui.cursor().min;
    for (i, tile) in tiles.iter().enumerate() {
        let slot = Rect::from_min_size(pos2(origin.x + i as f32 * (width + GAP), origin.y), vec2(width, 100_000.0));
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt((key, i)).max_rect(slot).layout(egui::Layout::top_down(egui::Align::Min)));
        let card = widgets::card(t).show(&mut child, |ui| {
            // The frame adds its margin (16) and stroke (1) on each side.
            ui.set_width(width - 34.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            tile(ui, acts);
            let h = ui.min_rect().height();
            tallest = tallest.max(h);
            // add_space comes after the item spacing of the last line.
            let pad = min_h - h - ui.spacing().item_spacing.y;
            if pad > 0.0 {
                ui.add_space(pad);
            }
        });
        row_h = row_h.max(card.response.rect.height());
    }
    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
    if (tallest - min_h).abs() > 0.5 {
        ui.data_mut(|m| m.insert_temp(id, tallest));
        ui.ctx().request_repaint();
    }
}

/// A tile's heading: small capitals on the left, an optional link on the right. Returns whether the link was clicked.
fn tile_head(ui: &mut Ui, title: &str, link: Option<&str>, t: &Tokens) -> bool {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, title.to_uppercase(), theme::bold(11.0), t.text_faint);
    let Some(link) = link else { return false };
    let g = ui.painter().layout_no_wrap(link.to_string(), theme::body(12.0), t.accent);
    let r = Rect::from_min_max(pos2(rect.right() - g.size().x - 14.0, rect.top()), rect.right_bottom());
    let resp = ui.interact(r, ui.id().with(("tile-link", title)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    widgets::a11y(&resp, link);
    let col = if resp.hovered() { t.text } else { t.accent };
    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, col);
    icons::glyph(ui.painter(), pos2(r.right() - 4.0, r.center().y), 9.0, Glyph::ChevronRight, col);
    resp.clicked()
}

/// A big amount, like the net worth.
fn big_amount(ui: &mut Ui, m: Money, col: Color32) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(m.format_euros()).font(theme::bold(28.0)).color(col));
}

fn caption(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.label(egui::RichText::new(text).size(12.0).color(t.text_dim));
}

/// A clickable list line in a tile: an icon, a name and an amount.
fn list_row(
    ui: &mut Ui,
    paint: impl FnOnce(&egui::Painter, egui::Pos2),
    label: &str,
    amount: String,
    amount_col: Color32,
    t: &Tokens,
) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, LIST_ROW_H), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect.expand2(vec2(6.0, 0.0)), 6.0, t.hover);
    }
    paint(p, pos2(rect.left() + 8.0, rect.center().y));
    let ag = p.layout_no_wrap(amount, theme::body(13.0), amount_col);
    let aw = ag.size().x;
    p.galley(pos2(rect.right() - aw, rect.center().y - ag.size().y / 2.0), ag, amount_col);
    let name = widgets::elide(ui, label, theme::body(13.0), rect.width() - aw - 40.0);
    ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, name, theme::body(13.0), t.text);
    widgets::a11y(&resp, label);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small heading inside a tile with a total on the right.
fn sub_head(ui: &mut Ui, label: &str, total: Money, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let p = ui.painter();
    p.text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::bold(12.0), t.text_dim);
    p.text(pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, total.format(), theme::bold(12.0), t.text_dim);
}

fn divider(ui: &mut Ui, t: &Tokens) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.border));
    ui.add_space(4.0);
}

fn account_rows(ui: &mut Ui, lines: &[PhysicalBalance], group: Group, t: &Tokens, acts: &mut Vec<Act>) {
    for l in lines {
        let col = if l.balance.is_negative() { t.danger } else { t.text };
        let r = list_row(ui, |p, c| icons::account(p, c, 15.0, l.kind, group, t), &l.name, l.balance.format(), col, t);
        if r.on_hover_text("Show its transactions").clicked() {
            acts.push(Act::Show(l.account));
        }
    }
}

fn net_worth_tile(ui: &mut Ui, d: &Data, t: &Tokens, acts: &mut Vec<Act>) {
    let nw = &d.net_worth;
    if tile_head(ui, "Net worth", Some("Report"), t) {
        acts.push(Act::Run("view.report", json!({ "report": "netWorth" })));
    }
    big_amount(ui, nw.net_worth, if nw.net_worth.is_negative() { t.danger } else { t.text });
    caption(ui, "What you have minus what you owe, today.", t);
    ui.add_space(10.0);
    sub_head(ui, "Money you have", nw.total_have, t);
    account_rows(ui, &nw.have, Group::Have, t, acts);
    if !nw.owe.is_empty() {
        divider(ui, t);
        sub_head(ui, "Money you owe", nw.total_owe, t);
        account_rows(ui, &nw.owe, Group::Owe, t, acts);
    }
}

fn free_tile(ui: &mut Ui, d: &Data, t: &Tokens, acts: &mut Vec<Act>) {
    let f = &d.free;
    if tile_head(ui, "Free to spend", Some("Pots"), t) {
        acts.push(Act::Run("view.pots", json!({})));
    }
    big_amount(ui, f.amount, if f.amount.is_negative() { t.danger } else { t.positive });
    caption(ui, "Money not set aside in a pot, on current accounts and cash, minus what you owe on credit cards.", t);
    ui.add_space(10.0);
    if f.unassigned.is_empty() {
        caption(ui, "Add a current account or cash to see what you can spend.", t);
        return;
    }
    sub_head(ui, "Unassigned", f.unassigned.iter().map(|u| u.amount).sum(), t);
    for u in &f.unassigned {
        let kind = d.net_worth.have.iter().find(|h| h.account == u.account).map(|h| h.kind).unwrap_or(AccountKind::Current);
        let col = if u.amount.is_negative() { t.danger } else { t.text };
        let r = list_row(ui, |p, c| icons::account(p, c, 15.0, kind, Group::Have, t), &u.name, u.amount.format(), col, t);
        if r.on_hover_text("Show its transactions").clicked() {
            acts.push(Act::Show(u.account));
        }
    }
    let cards: Vec<&PhysicalBalance> = d.net_worth.owe.iter().filter(|o| o.kind == AccountKind::CreditCard && o.balance.is_positive()).collect();
    if !cards.is_empty() {
        divider(ui, t);
        sub_head(ui, "Credit card debt", -f.card_debt, t);
        for c in cards {
            let r = list_row(ui, |p, at| icons::account(p, at, 15.0, c.kind, Group::Owe, t), &c.name, (-c.balance).format(), t.text, t);
            if r.on_hover_text("Show its transactions").clicked() {
                acts.push(Act::Show(c.account));
            }
        }
    }
}

fn month_name(d: NaiveDate) -> String {
    d.format("%B").to_string()
}

fn month_tile(ui: &mut Ui, d: &Data, t: &Tokens, acts: &mut Vec<Act>) {
    let (now, last) = (&d.this_month, &d.last_month);
    let title = format!("This month · {}", month_name(now.from));
    if tile_head(ui, &title, Some("Report"), t) {
        acts.push(Act::Run("view.report", json!({ "report": "incomeExpenses", "from": now.from.to_string(), "to": now.to.to_string() })));
    }
    ui.add_space(10.0);
    let stats = [
        ("In", now.total_income, last.total_income, t.positive),
        ("Out", now.total_expenses, last.total_expenses, t.text),
        ("Left over", now.left_over, last.left_over, if now.left_over.is_negative() { t.danger } else { t.text }),
    ];
    let w = ui.available_width();
    let col_w = w / 3.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
    let p = ui.painter();
    for (i, (label, value, before, col)) in stats.iter().enumerate() {
        let x = rect.left() + col_w * i as f32;
        p.text(pos2(x, rect.top() + 8.0), Align2::LEFT_CENTER, *label, theme::body(12.0), t.text_dim);
        p.text(pos2(x, rect.top() + 30.0), Align2::LEFT_CENTER, value.format_euros(), theme::bold(20.0), *col);
        p.text(
            pos2(x, rect.top() + 54.0),
            Align2::LEFT_CENTER,
            format!("{} {}", short_month(last.from), before.format_euros()),
            theme::body(11.5),
            t.text_faint,
        );
    }
    ui.add_space(12.0);
    // In and out of both months on one scale.
    let max = [now.total_income, now.total_expenses, last.total_income, last.total_expenses].into_iter().max().unwrap_or(Money::ZERO);
    for (label, m) in [(format!("{} so far", month_name(now.from)), now), (month_name(last.from), last)] {
        compare_bars(ui, &label, m.total_income, m.total_expenses, max, t);
    }
    let waiting = now.not_sorted_in + now.not_sorted_out;
    if waiting.is_positive() {
        ui.add_space(8.0);
        let text = format!("{} waits on Not sorted yet and is not counted.", waiting.format());
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r.expand2(vec2(6.0, 0.0)), 6.0, t.warning_soft);
        }
        icons::status(ui.painter(), pos2(r.left() + 7.0, r.center().y), 12.0, pecunia_engine::model::TxnStatus::ToReview, t);
        let shown = widgets::elide(ui, &text, theme::body(12.0), r.width() - 24.0);
        ui.painter().text(pos2(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, shown, theme::body(12.0), t.warning);
        widgets::a11y(&resp, &text);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open Review").clicked() {
            acts.push(Act::Run("view.review", json!({})));
        }
    }
}

fn short_month(d: NaiveDate) -> String {
    d.format("%b").to_string()
}

/// Two thin bars, in and out, for one month.
fn compare_bars(ui: &mut Ui, label: &str, money_in: Money, money_out: Money, max: Money, t: &Tokens) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    let p = ui.painter();
    let label_w = 120.0f32.min(w * 0.3);
    p.text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::body(12.0), t.text_dim);
    let bar_x = rect.left() + label_w;
    let bar_w = (rect.right() - bar_x - 90.0).max(20.0);
    let frac = |m: Money| if max.is_positive() { (m.cents() as f64 / max.cents() as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
    for (dy, m, col) in [(-8.0, money_in, t.positive), (8.0, money_out, t.text_faint)] {
        let y = rect.center().y + dy;
        let track = Rect::from_min_size(pos2(bar_x, y - 3.0), vec2(bar_w, 6.0));
        p.rect_filled(track, 3.0, t.border.gamma_multiply(0.6));
        let f = frac(m);
        if f > 0.0 {
            p.rect_filled(Rect::from_min_size(track.min, vec2((bar_w * f).max(3.0), 6.0)), 3.0, col);
        }
        let what = if dy < 0.0 { "in" } else { "out" };
        p.text(
            pos2(rect.right(), y),
            Align2::RIGHT_CENTER,
            format!("{what} {}", m.format_euros()),
            theme::body(11.5),
            if dy < 0.0 { t.positive } else { t.text_dim },
        );
    }
}

/// A round number just above `x` cents: 1, 2 or 5 times a power of ten.
fn nice_step(x: f64) -> f64 {
    if !x.is_finite() || x <= 0.0 {
        return 100.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let f = x / p;
    let n = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 2.5 {
        2.5
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    (n * p).max(100.0)
}

fn chart_tile(ui: &mut Ui, d: &Data, t: &Tokens) {
    tile_head(ui, "Bank and cash", None, t);
    let now = d.month_ends.last().map(|m| m.total).unwrap_or(Money::ZERO);
    big_amount(ui, now, if now.is_negative() { t.danger } else { t.text });
    let first = d.month_ends.first().map(|m| m.total).unwrap_or(Money::ZERO);
    let change = now - first;
    let since = d.month_ends.first().map(|m| m.date.format("%B %Y").to_string()).unwrap_or_default();
    let sign = if change.is_positive() { "+" } else { "" };
    caption(ui, &format!("Current and savings accounts and cash at the end of each month · {sign}{} since {since}", change.format_euros()), t);
    ui.add_space(12.0);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, CHART_H), Sense::hover());
    line_chart(ui, rect, &d.month_ends, resp.hover_pos(), t);
}

fn line_chart(ui: &Ui, rect: Rect, points: &[MonthEnd], hover: Option<egui::Pos2>, t: &Tokens) {
    let p = ui.painter();
    if points.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, "No balances yet", theme::body(12.0), t.text_faint);
        return;
    }
    let values: Vec<f64> = points.iter().map(|m| m.total.cents() as f64).collect();
    let (mut lo, mut hi) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    // Start at zero when that does not flatten the line too much; always show zero when a balance is negative.
    if lo >= 0.0 && lo < hi * 0.6 {
        lo = 0.0;
    }
    if hi <= 0.0 && hi > lo * 0.6 {
        hi = 0.0;
    }
    if (hi - lo).abs() < 1.0 {
        hi += 10_000.0;
        lo -= 10_000.0;
    }
    let step = nice_step((hi - lo) / 4.0);
    let lo = (lo / step).floor() * step;
    let hi = (hi / step).ceil() * step;

    // Axis labels on the left, months along the bottom.
    let font = theme::body(11.0);
    let mut label_w: f32 = 0.0;
    let mut ticks = Vec::new();
    let mut v = lo;
    while v <= hi + step * 0.5 && ticks.len() < 12 {
        let text = Money(v.round() as i64).format_euros();
        label_w = label_w.max(p.layout_no_wrap(text.clone(), font.clone(), t.text_faint).size().x);
        ticks.push((v, text));
        v += step;
    }
    let plot = Rect::from_min_max(pos2(rect.left() + label_w + 10.0, rect.top() + 6.0), pos2(rect.right() - 6.0, rect.bottom() - 22.0));
    let y_of = |v: f64| plot.bottom() - ((v - lo) / (hi - lo)) as f32 * plot.height();
    for (v, text) in &ticks {
        let y = y_of(*v);
        let col = if *v == 0.0 { t.border_strong } else { t.border.gamma_multiply(0.8) };
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, col));
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, text, font.clone(), t.text_faint);
    }
    let n = points.len();
    let x_of = |i: usize| if n <= 1 { plot.center().x } else { plot.left() + plot.width() * i as f32 / (n - 1) as f32 };
    let pts: Vec<egui::Pos2> = values.iter().enumerate().map(|(i, v)| pos2(x_of(i), y_of(*v))).collect();
    let label_every = if plot.width() / n.max(1) as f32 >= 34.0 { 1 } else { 2 };
    for (i, m) in points.iter().enumerate() {
        if (n - 1 - i).is_multiple_of(label_every) {
            let text = if m.date.month() == 1 { m.date.format("%b %y").to_string() } else { short_month(m.date) };
            p.text(pos2(x_of(i), plot.bottom() + 12.0), Align2::CENTER_CENTER, text, font.clone(), t.text_faint);
        }
    }

    // The area under the line (one trapezoid per month, each convex), then the line and the points.
    let base = y_of(lo.max(0.0).min(hi));
    let fill = t.accent.gamma_multiply(if t.dark { 0.18 } else { 0.10 });
    for pair in pts.windows(2) {
        if let [a, b] = pair {
            p.add(Shape::convex_polygon(vec![*a, *b, pos2(b.x, base), pos2(a.x, base)], fill, Stroke::NONE));
        }
    }
    if pts.len() > 1 {
        p.add(Shape::line(pts.clone(), Stroke::new(2.0, t.accent)));
    }
    for pt in &pts {
        p.circle_filled(*pt, 3.0, t.surface);
        p.circle_stroke(*pt, 3.0, Stroke::new(1.6, t.accent));
    }

    // Hover: the nearest month with its balance.
    let Some(h) = hover.filter(|h| rect.contains(*h)) else { return };
    let Some((i, pt)) = pts.iter().enumerate().min_by(|a, b| (a.1.x - h.x).abs().total_cmp(&(b.1.x - h.x).abs())) else { return };
    let Some(m) = points.get(i) else { return };
    p.line_segment([pos2(pt.x, plot.top()), pos2(pt.x, plot.bottom())], Stroke::new(1.0, t.border_strong));
    p.circle_filled(*pt, 4.5, t.accent);
    let text = format!("{}  {}", m.date.format("%-d %b %Y"), m.total.format());
    let g = p.layout_no_wrap(text, theme::body(12.0), t.text);
    let size = g.size() + vec2(16.0, 10.0);
    let mut x = pt.x - size.x / 2.0;
    x = x.clamp(rect.left(), (rect.right() - size.x).max(rect.left()));
    let y = (pt.y - size.y - 10.0).max(rect.top());
    let tip = Rect::from_min_size(pos2(x, y), size);
    p.rect(tip, 6.0, t.surface, Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    p.galley(tip.min + vec2(8.0, 5.0), g, t.text);
}

fn spending_tile(ui: &mut Ui, d: &Data, t: &Tokens, acts: &mut Vec<Act>) {
    let last = &d.last_month;
    let title = format!("Biggest spending · {}", month_name(last.from));
    let (from, to) = (last.from.to_string(), last.to.to_string());
    if tile_head(ui, &title, Some("Report"), t) {
        acts.push(Act::Run("view.report", json!({ "report": "incomeExpenses", "from": from, "to": to })));
    }
    ui.add_space(6.0);
    if last.expenses.is_empty() {
        ui.add_space(20.0);
        caption(ui, &format!("Nothing spent in {} yet.", month_name(last.from)), t);
        return;
    }
    caption(ui, &format!("{} spent in all, {} came in.", last.total_expenses.format(), last.total_income.format()), t);
    ui.add_space(8.0);
    let max = last.expenses.first().map(|c| c.amount).unwrap_or(Money::ZERO);
    let col = t.group(Group::Goes);
    for c in last.expenses.iter().take(TOP_CATEGORIES) {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect.expand2(vec2(6.0, 0.0)), 6.0, t.hover);
        }
        let top = rect.top() + 13.0;
        let ag = p.layout_no_wrap(c.amount.format(), theme::body(13.0), t.text);
        let aw = ag.size().x;
        p.galley(pos2(rect.right() - aw, top - ag.size().y / 2.0), ag, t.text);
        let share = if last.total_expenses.is_positive() { c.amount.cents() * 100 / last.total_expenses.cents() } else { 0 };
        let pct = format!("{share}%");
        let pg = p.layout_no_wrap(pct, theme::body(11.5), t.text_faint);
        p.galley(pos2(rect.right() - aw - 10.0 - pg.size().x, top - pg.size().y / 2.0), pg, t.text_faint);
        let name = widgets::elide(ui, &c.name, theme::body(13.0), rect.width() - aw - 70.0);
        let p = ui.painter();
        p.text(pos2(rect.left(), top), Align2::LEFT_CENTER, name, theme::body(13.0), t.text);
        let track = Rect::from_min_size(pos2(rect.left(), rect.top() + 26.0), vec2(rect.width(), 6.0));
        p.rect_filled(track, 3.0, t.border.gamma_multiply(0.6));
        let f = if max.is_positive() { (c.amount.cents() as f64 / max.cents() as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
        if f > 0.0 {
            p.rect_filled(Rect::from_min_size(track.min, vec2((track.width() * f).max(3.0), 6.0)), 3.0, col);
        }
        widgets::a11y(&resp, &c.name);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show these transactions").clicked() {
            acts.push(Act::Register(json!({ "replace": true, "account": c.account.0, "from": from, "to": to })));
        }
    }
    let rest = last.expenses.len().saturating_sub(TOP_CATEGORIES);
    if rest > 0 {
        let sum: Money = last.expenses.iter().skip(TOP_CATEGORIES).map(|c| c.amount).sum();
        ui.add_space(4.0);
        caption(ui, &format!("{rest} more categor{} · {}", if rest == 1 { "y" } else { "ies" }, sum.format()), t);
    }
}

/// One line in "Needs attention".
struct Item {
    icon: Box<dyn Fn(&egui::Painter, egui::Pos2)>,
    title: String,
    detail: String,
    button: Option<(&'static str, bool)>,
    act: Act,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn attention_items(d: &Data, t: &Tokens) -> Vec<Item> {
    let a = &d.attention;
    let mut items = Vec::new();
    let r = a.review;
    if r.to_review > 0 {
        let mut parts = Vec::new();
        for (n, one, many) in [
            (r.duplicates, "possible duplicate", "possible duplicates"),
            (r.transfers, "transfer", "transfers"),
            (r.suggested, "suggestion", "suggestions"),
            (r.unknown, "unknown", "unknown"),
        ] {
            if n > 0 {
                parts.push(plural(n, one, many));
            }
        }
        let warning = t.warning;
        items.push(Item {
            icon: Box::new(move |p, c| icons::glyph(p, c, 15.0, Glyph::Inbox, warning)),
            title: format!("{} to review", plural(r.to_review, "transaction", "transactions")),
            detail: parts.join(" · "),
            button: Some(("Open Review", true)),
            act: Act::Run("view.review", json!({})),
        });
    }
    for b in &a.bank_differences {
        if let reports::BankCheckStatus::Differs { by, date, stated, books, .. } = &b.status {
            let danger = t.danger;
            items.push(Item {
                icon: Box::new(move |p, c| {
                    p.circle_filled(c, 7.5, danger);
                    p.text(c, Align2::CENTER_CENTER, "!", theme::bold(11.0), Color32::WHITE);
                }),
                title: format!("{} differs from the bank by {}", b.name, by.abs().format()),
                detail: format!("The bank said {} on {}; the books say {}.", stated.format(), date.format("%-d %b %Y"), books.format()),
                button: None,
                act: Act::Show(b.account),
            });
        }
    }
    let fixed = || Act::Run("view.report", json!({ "report": "fixedCosts" }));
    for c in &a.price_changes {
        let col = if c.per_year_more.is_positive() { t.warning } else { t.positive };
        let more = if c.per_year_more.is_positive() { "more" } else { "less" };
        items.push(Item {
            icon: Box::new(move |p, at| icons::glyph(p, at, 14.0, Glyph::Chart, col)),
            title: format!("Costs {more}: {}", c.name),
            detail: format!(
                "{} now, was {} · since {} · {} {more} a year",
                c.new_amount.abs().format(),
                c.old_amount.abs().format(),
                c.since.format("%-d %b"),
                c.per_year_more.abs().format_euros()
            ),
            button: None,
            act: fixed(),
        });
    }
    for c in &a.new_fixed_costs {
        let col = t.accent;
        items.push(Item {
            icon: Box::new(move |p, at| icons::glyph(p, at, 14.0, Glyph::Sprint, col)),
            title: format!("New fixed cost: {} {} a month", c.name, c.amount.abs().format()),
            detail: format!("{} months in a row around day {} · next on {}", c.months_in_a_row, c.day_of_month, c.next_date.format("%-d %b")),
            button: None,
            act: fixed(),
        });
    }
    for c in &a.stopped_fixed_costs {
        let col = t.text_dim;
        items.push(Item {
            icon: Box::new(move |p, at| icons::glyph(p, at, 14.0, Glyph::Sprint, col)),
            title: format!("{} did not come: {} expected on {}", c.name, c.amount.abs().format(), c.expected.format("%-d %b")),
            detail: format!("Last paid on {}. Stopped, or paid another way?", c.last_date.format("%-d %b %Y")),
            button: None,
            act: fixed(),
        });
    }
    for c in &a.own_accounts {
        let col = t.group(Group::Have);
        let who = if c.name.is_empty() { c.iban.clone() } else { format!("{} ({})", c.name, c.iban) };
        items.push(Item {
            icon: Box::new(move |p, at| icons::glyph(p, at, 15.0, Glyph::Swap, col)),
            title: format!("Is {who} one of your accounts?"),
            detail: format!(
                "Money went there {}× ({}) and came back {}× ({})",
                c.times_out,
                c.total_out.format_euros(),
                c.times_in,
                c.total_in.format_euros()
            ),
            button: Some(("Add account", false)),
            act: Act::Dialog("newAccount", json!({ "kind": "savings", "name": c.name, "iban": c.iban })),
        });
    }
    items
}

fn attention_tile(ui: &mut Ui, d: &Data, t: &Tokens, acts: &mut Vec<Act>) {
    let n = d.attention.count();
    tile_head(ui, "Needs attention", None, t);
    if n > 0 {
        // A count next to the heading.
        let head = ui.min_rect();
        let x = head.left() + ui.painter().layout_no_wrap("NEEDS ATTENTION".into(), theme::bold(11.0), t.text_faint).size().x + 8.0;
        let g = ui.painter().layout_no_wrap(n.to_string(), theme::bold(10.5), t.text);
        let pill = Rect::from_min_size(pos2(x, head.top() + 1.0), vec2(g.size().x + 12.0, 16.0));
        ui.painter().rect_filled(pill, 8.0, t.warning_soft);
        ui.painter().galley(pill.center() - g.size() / 2.0, g, t.warning);
    }
    let items = attention_items(d, t);
    if items.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            icons::status(ui.painter(), r.center(), 28.0, pecunia_engine::model::TxnStatus::Reviewed, t);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("All caught up").font(theme::bold(15.0)).color(t.text));
            ui.label(
                egui::RichText::new("Nothing waits in Review, the bank checks agree and the fixed costs hold no news.").size(12.5).color(t.text_dim),
            );
        });
        return;
    }
    ui.add_space(6.0);
    let total = items.len();
    for (i, item) in items.into_iter().take(ATTENTION_MAX).enumerate() {
        if i > 0 {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().line_segment([pos2(r.left() + 28.0, r.center().y), r.right_center()], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
        }
        attention_row(ui, item, i, t, acts);
    }
    if total > ATTENTION_MAX {
        ui.add_space(4.0);
        caption(ui, &format!("and {} more", total - ATTENTION_MAX), t);
    }
}

fn attention_row(ui: &mut Ui, item: Item, i: usize, t: &Tokens, acts: &mut Vec<Act>) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, ATTENTION_ROW_H), Sense::hover());
    // The button (when there is one) sits on the right; the rest of the row is clickable.
    let mut button_w = 0.0;
    let mut clicked = false;
    if let Some((label, primary)) = item.button {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let r = if primary { widgets::primary_button(&mut child, label, t) } else { widgets::secondary_button(&mut child, label, t) };
        button_w = r.rect.width() + 12.0;
        clicked |= r.clicked();
    }
    let hit = Rect::from_min_max(rect.min, pos2(rect.right() - button_w, rect.bottom()));
    let resp = ui.interact(hit, ui.id().with(("attention", i)), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(hit.expand2(vec2(6.0, 0.0)), 6.0, t.hover);
    }
    (item.icon)(p, pos2(rect.left() + 9.0, rect.top() + 16.0));
    let room = (hit.width() - 40.0).max(40.0);
    let title = widgets::elide(ui, &item.title, theme::body(13.0), room);
    let detail = widgets::elide(ui, &item.detail, theme::body(11.5), room);
    let p = ui.painter();
    p.text(pos2(rect.left() + 28.0, rect.top() + 16.0), Align2::LEFT_CENTER, title, theme::body(13.0), t.text);
    p.text(pos2(rect.left() + 28.0, rect.top() + 34.0), Align2::LEFT_CENTER, detail, theme::body(11.5), t.text_dim);
    if item.button.is_none() {
        icons::glyph(p, pos2(hit.right() - 6.0, rect.center().y), 10.0, Glyph::ChevronRight, t.text_faint);
    }
    widgets::a11y(&resp, &item.title);
    clicked |= resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
    if clicked {
        acts.push(item.act);
    }
}

#[cfg(test)]
mod tests {
    use super::nice_step;

    #[test]
    fn nice_steps_are_round() {
        assert_eq!(nice_step(0.0), 100.0);
        assert_eq!(nice_step(f64::NAN), 100.0);
        assert_eq!(nice_step(130_000.0), 200_000.0);
        assert_eq!(nice_step(480_000.0), 500_000.0);
        assert_eq!(nice_step(2_100_000.0), 2_500_000.0);
    }
}
