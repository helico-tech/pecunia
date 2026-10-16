//! The Accounts and categories screen: the whole account tree in four sections (money you have, owe, where it goes,
//! where it comes from), each account with its kind, IBAN or note, and its balance as people see it. Categories show
//! what went through them in the last 12 months. A click shows the register; hover buttons edit and add.

use std::cell::RefCell;
use std::sync::Arc;

use chrono::Months;
use egui::{Align2, Color32, Id, Rect, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::catalogue::{self, Entry};
use pecunia_engine::model::{Account, AccountId, AccountKind, AccountTree, Balances, Book, Group, Money, NaiveDate};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

const HEADER_H: f32 = 40.0;
const TOP_H: f32 = 46.0;
const CHILD_H: f32 = 36.0;
const EMPTY_H: f32 = 40.0;
/// Room for the amount on the right.
const AMOUNT_W: f32 = 120.0;

/// The four sections, in the order people think of money; Equity stays hidden.
const SECTIONS: [Group; 4] = [Group::Have, Group::Owe, Group::Goes, Group::Comes];

/// Balances today and over the last 12 months, kept while the book and the day stay the same.
struct Sums {
    now: Balances,
    year: Balances,
}

#[derive(Default)]
struct Cache {
    key: Option<(u64, NaiveDate)>,
    sums: Option<Arc<Sums>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn sums(app: &PecuniaApp) -> Arc<Sums> {
    let today = app.session.today();
    let key = (app.session.revision(), today);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let (Some(k), Some(s)) = (c.key, &c.sums)
            && k == key
        {
            return s.clone();
        }
        let book = app.session.book();
        let from = year_start(today);
        let s = Arc::new(Sums { now: book.balances_at(today), year: Balances::between(book.txns().map(|t| t.as_ref()), from, today) });
        c.key = Some(key);
        c.sums = Some(s.clone());
        s
    })
}

/// The first day of the 12 months that end today.
fn year_start(today: NaiveDate) -> NaiveDate {
    today.checked_sub_months(Months::new(12)).and_then(|d| d.succ_opt()).unwrap_or(today)
}

enum Line<'a> {
    Section { group: Group, total: Money },
    Account { a: &'a Account, depth: usize, amount: Money, children: usize },
    Empty(&'static str),
}

impl Line<'_> {
    fn height(&self) -> f32 {
        match self {
            Line::Section { .. } => HEADER_H,
            Line::Account { depth: 0, .. } => TOP_H,
            Line::Account { .. } => CHILD_H,
            Line::Empty(_) => EMPTY_H,
        }
    }
}

/// What a click asks for; run after drawing.
enum Act {
    Show(AccountId),
    Run(&'static str, Json),
    Dialog(&'static str, Json),
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let show_closed_id = Id::new("accounts-show-closed");
    let show_closed = ui.data(|d| d.get_temp::<bool>(show_closed_id)).unwrap_or(false);
    let book = app.session.book().clone();
    let tree = book.tree();
    let sums = sums(app);
    let mut acts: Vec<Act> = Vec::new();

    ui.spacing_mut().item_spacing.y = 0.0;
    toolbar(ui, &book, &tree, show_closed, show_closed_id, &mut acts, t);

    let mut lines: Vec<Line> = Vec::new();
    for group in SECTIONS {
        let balances = if matches!(group, Group::Goes | Group::Comes) { &sums.year } else { &sums.now };
        let roots = tree.roots(group);
        let total: Money = roots.iter().map(|a| balances.shown(&tree, a.id)).sum();
        lines.push(Line::Section { group, total });
        let before = lines.len();
        for a in roots {
            push_branch(&mut lines, &tree, balances, a, 0, show_closed);
        }
        if lines.len() == before {
            lines.push(Line::Empty(match group {
                Group::Have => "No accounts yet. Add your current account to start.",
                Group::Owe => "Nothing you owe. Add a credit card, loan or mortgage when you have one.",
                _ => "No categories yet.",
            }));
        }
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let w = ui.available_width();
        for line in &lines {
            let (rect, _) = ui.allocate_exact_size(vec2(w, line.height()), Sense::hover());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            match line {
                Line::Section { group, total } => section(ui, rect, *group, *total, &mut acts, t),
                Line::Account { a, depth, amount, children } => account_row(ui, rect, a, *depth, *amount, *children, &mut acts, t),
                Line::Empty(text) => {
                    ui.painter().text(pos2(rect.left() + 48.0, rect.center().y), Align2::LEFT_CENTER, *text, theme::body(12.5), t.text_faint);
                    ui.painter().line_segment(
                        [pos2(rect.left() + 16.0, rect.bottom()), pos2(rect.right(), rect.bottom())],
                        Stroke::new(1.0, t.border.gamma_multiply(0.7)),
                    );
                }
            }
        }
        ui.add_space(24.0);
    });

    for a in acts {
        match a {
            Act::Show(id) => app.show_account(id),
            Act::Run(id, p) => {
                app.run_any(id, p);
            }
            Act::Dialog(name, p) => app.open_dialog(name, p),
        }
    }
}

/// Adds an account and what sits under it. An account whose only pot is Unassigned shows no pots.
fn push_branch<'a>(lines: &mut Vec<Line<'a>>, tree: &'a AccountTree, balances: &Balances, a: &'a Account, depth: usize, show_closed: bool) {
    if a.closed && !show_closed {
        return;
    }
    let kids: Vec<&Account> = tree.children(a.id).into_iter().filter(|k| show_closed || !k.closed).collect();
    let only_unassigned = kids.iter().all(|k| k.unassigned);
    let children = if a.kind == AccountKind::CategoryGroup { kids.len() } else { 0 };
    lines.push(Line::Account { a, depth, amount: balances.shown(tree, a.id), children });
    if only_unassigned || depth > 8 {
        return;
    }
    for k in kids {
        push_branch(lines, tree, balances, k, depth + 1, show_closed);
    }
}

fn toolbar(ui: &mut Ui, book: &Book, tree: &AccountTree, show_closed: bool, show_closed_id: Id, acts: &mut Vec<Act>, t: &Tokens) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.label(egui::RichText::new("Accounts and categories").font(theme::bold(18.0)).color(t.text));
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let missing: Vec<&'static Entry> = catalogue::ENTRIES
            .iter()
            .filter(|e| !book.accounts().any(|a| a.kind == AccountKind::Category && a.name.eq_ignore_ascii_case(e.name)))
            .collect();
        if !missing.is_empty() {
            let r = widgets::pill_outlined(ui, |p, c| icons::glyph(p, c, 12.0, Glyph::Plus, t.text_dim), "Add a usual category", false, t);
            egui::Popup::menu(&r)
                .frame(widgets::popup_frame(t))
                .width(340.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| usual_categories(ui, &missing, acts, t));
        }
        let closed = tree.walk().into_iter().filter(|a| a.closed && !a.unassigned && a.group != Group::Equity).count();
        if closed > 0 {
            ui.add_space(4.0);
            let label = format!("Show closed ({closed})");
            let r = widgets::pill_outlined(
                ui,
                |p, c| {
                    icons::glyph(
                        p,
                        c,
                        12.0,
                        if show_closed { Glyph::Check } else { Glyph::Filter },
                        if show_closed { t.accent } else { t.text_faint },
                    )
                },
                &label,
                !show_closed,
                t,
            );
            if r.on_hover_text("Closed accounts keep their history").clicked() {
                ui.data_mut(|d| d.insert_temp(show_closed_id, !show_closed));
            }
        }
    });
}

/// The menu of usual household categories the book does not have yet, under their category group.
fn usual_categories(ui: &mut Ui, missing: &[&'static Entry], acts: &mut Vec<Act>, t: &Tokens) {
    ui.label(egui::RichText::new("Common Dutch household categories. A missing group is made too.").size(11.5).color(t.text_faint));
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        let mut heading: Option<&str> = None;
        for e in missing {
            if heading != Some(e.group_name) {
                heading = Some(e.group_name);
                ui.add_space(6.0);
                ui.label(egui::RichText::new(e.group_name.to_uppercase()).font(theme::bold(10.5)).color(t.text_faint));
            }
            let w = ui.available_width().max(300.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 5.0, t.hover);
            }
            icons::account(ui.painter(), pos2(rect.left() + 14.0, rect.top() + 12.0), 12.0, AccountKind::Category, e.group, t);
            ui.painter().text(pos2(rect.left() + 28.0, rect.top() + 12.0), Align2::LEFT_CENTER, e.name, theme::body(13.0), t.text);
            let desc = widgets::elide(ui, e.description, theme::body(11.5), rect.width() - 40.0);
            ui.painter().text(pos2(rect.left() + 28.0, rect.top() + 28.0), Align2::LEFT_CENTER, desc, theme::body(11.5), t.text_faint);
            if resp.hovered() {
                icons::glyph(ui.painter(), pos2(rect.right() - 14.0, rect.top() + 12.0), 11.0, Glyph::Plus, t.accent);
            }
            widgets::a11y(&resp, format!("Add {}", e.name));
            if resp.clicked() {
                acts.push(Act::Run("category.add", json!({ "name": e.name })));
            }
        }
    });
}

/// What the section's add button makes, and its label.
fn section_new(group: Group) -> (&'static str, Json) {
    match group {
        Group::Owe => ("New account", json!({ "kind": "credit_card" })),
        Group::Goes => ("New category group", json!({ "kind": "category_group", "group": "goes" })),
        Group::Comes => ("New category group", json!({ "kind": "category_group", "group": "comes" })),
        _ => ("New account", json!({ "kind": "current" })),
    }
}

fn section(ui: &mut Ui, rect: Rect, group: Group, total: Money, acts: &mut Vec<Act>, t: &Tokens) {
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, t.sidebar);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let cy = rect.center().y;
    p.circle_filled(pos2(rect.left() + 30.0, cy), 4.0, t.group(group));
    let label = p.layout_no_wrap(group.label().to_string(), theme::bold(13.0), t.text);
    let label_w = label.size().x;
    p.galley(pos2(rect.left() + 48.0, cy - label.size().y / 2.0), label, t.text);

    // The add button, just after the title.
    let (text, params) = section_new(group);
    let g = p.layout_no_wrap(text.to_string(), theme::body(12.0), t.text_dim);
    let btn = Rect::from_min_size(pos2(rect.left() + 48.0 + label_w + 12.0, cy - 12.0), vec2(g.size().x + 30.0, 24.0));
    let resp = ui.interact(btn, ui.id().with(("section-new", group.id())), Sense::click());
    widgets::a11y(&resp, format!("{text} in {}", group.label()));
    if resp.hovered() {
        p.rect_filled(btn, 6.0, t.border);
    }
    let col = if resp.hovered() { t.text } else { t.text_dim };
    icons::glyph(&p, pos2(btn.left() + 12.0, cy), 11.0, Glyph::Plus, col);
    p.galley(pos2(btn.left() + 22.0, cy - g.size().y / 2.0), g, col);
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        acts.push(Act::Dialog("newAccount", params));
    }

    // The total, in line with the amounts, with what it means.
    let right = rect.right() - 20.0;
    let tg = p.layout_no_wrap(total.format(), theme::bold(12.5), t.text);
    let tw = tg.size().x;
    p.galley(pos2(right - tw, cy - tg.size().y / 2.0), tg, t.text);
    let caption = match group {
        Group::Have => "today",
        Group::Owe => "owed today",
        Group::Goes => "spent in the last 12 months",
        _ => "came in over the last 12 months",
    };
    p.text(pos2(right - tw - 10.0, cy), Align2::RIGHT_CENTER, caption, theme::body(11.5), t.text_faint);
}

/// `NL55INGB0008810123` → `NL55 INGB 0008 8101 23`.
fn iban_spaced(iban: &str) -> String {
    let chars: Vec<char> = pecunia_engine::model::normalize_iban(iban).chars().collect();
    chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// The note next to an account: IBAN, pot goal, what belongs in a category, how many categories a group has.
fn note(a: &Account, children: usize) -> String {
    match a.kind {
        AccountKind::Pot if a.unassigned => "Money in the account no pot holds".into(),
        AccountKind::Pot => match (a.goal, a.goal_date) {
            (Some(g), Some(d)) => format!("Goal {} by {}", g.format(), d.format("%-d %b %Y")),
            (Some(g), None) => format!("Goal {}", g.format()),
            _ => String::new(),
        },
        AccountKind::Category => a.description.clone(),
        AccountKind::CategoryGroup => format!("{children} categor{}", if children == 1 { "y" } else { "ies" }),
        _ => a.iban.as_deref().map(iban_spaced).unwrap_or_default(),
    }
}

fn amount_colour(a: &Account, m: Money, t: &Tokens) -> Color32 {
    if m.is_zero() || a.closed {
        t.text_faint
    } else if a.group == Group::Have && m.is_negative() {
        t.danger
    } else if a.group == Group::Comes && m.is_positive() {
        t.positive
    } else {
        t.text
    }
}

#[allow(clippy::too_many_arguments)]
fn account_row(ui: &mut Ui, rect: Rect, a: &Account, depth: usize, amount: Money, children: usize, acts: &mut Vec<Act>, t: &Tokens) {
    let id = ui.id().with(("account", a.id.0));
    let resp = ui.interact(rect, id, Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let p = ui.painter().clone();
    let group_row = a.kind == AccountKind::CategoryGroup;
    if hovered {
        p.rect_filled(rect, 0.0, t.hover);
    }
    p.line_segment([pos2(rect.left() + 16.0, rect.bottom()), pos2(rect.right(), rect.bottom())], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
    let cy = rect.center().y;
    let indent = depth as f32 * 26.0;
    let icon_size = if depth == 0 { 18.0 } else { 14.0 };
    icons::account(&p, pos2(rect.left() + 30.0 + indent, cy), icon_size, a.kind, a.group, t);

    // Columns: name, kind, note, amount. The note goes first when the screen is narrow.
    let right = rect.right() - 20.0;
    let buttons_left = right - AMOUNT_W - 60.0;
    let name_x = rect.left() + 48.0 + indent;
    let kind_x = rect.left() + (rect.width() * 0.34).clamp(250.0, 420.0);
    let note_x = kind_x + 150.0;
    let show_note = buttons_left - note_x > 90.0;
    let show_kind = buttons_left - kind_x > 110.0;
    let name_room = if show_kind { kind_x - name_x - 14.0 } else { buttons_left - name_x - 8.0 };

    let name = if a.unassigned { "Unassigned" } else { a.name.as_str() };
    let font = if group_row { theme::bold(13.5) } else { theme::body(if depth == 0 { 13.5 } else { 13.0 }) };
    let name_col = if a.closed {
        t.text_faint
    } else if a.unassigned {
        t.text_dim
    } else {
        t.text
    };
    p.text(pos2(name_x, cy), Align2::LEFT_CENTER, widgets::elide(ui, name, font.clone(), name_room.max(40.0)), font, name_col);
    let kind = if a.closed { format!("{} · closed", a.kind.label()) } else { a.kind.label().to_string() };
    if show_kind {
        p.text(pos2(kind_x, cy), Align2::LEFT_CENTER, kind.as_str(), theme::body(12.0), t.text_dim);
    }
    let note = note(a, children);
    if show_note && !note.is_empty() {
        let text = widgets::elide(ui, &note, theme::body(12.0), buttons_left - note_x - 12.0);
        p.text(pos2(note_x, cy), Align2::LEFT_CENTER, text, theme::body(12.0), t.text_faint);
    }

    let amount_font = if group_row || depth == 0 { theme::bold(13.5) } else { theme::body(13.0) };
    p.text(pos2(right, cy), Align2::RIGHT_CENTER, amount.format(), amount_font, amount_colour(a, amount, t));

    // Hover buttons left of the amount: add a category (groups), edit (everything but system accounts).
    let mut x = right - AMOUNT_W - 14.0;
    let mut hover_button = |g: Glyph, key: &str, tip: String| -> bool {
        let r = Rect::from_center_size(pos2(x, cy), vec2(26.0, 26.0));
        x -= 30.0;
        let b = ui.interact(r, id.with(key), Sense::click());
        widgets::a11y(&b, tip.clone());
        if b.hovered() {
            p.rect_filled(r, 6.0, t.border);
        }
        if hovered || b.hovered() {
            icons::glyph(&p, r.center(), 14.0, g, if b.hovered() { t.text } else { t.text_dim });
        }
        b.on_hover_text(tip).clicked()
    };
    let mut button_clicked = false;
    if a.system.is_none() && hover_button(Glyph::More, "edit", format!("Edit {name}")) {
        acts.push(Act::Run("ui.editAccount", json!({ "id": a.id.0 })));
        button_clicked = true;
    }
    if group_row && hover_button(Glyph::Plus, "add", format!("New category in {name}")) {
        acts.push(Act::Dialog("newAccount", json!({ "kind": "category", "parent": a.id.0 })));
        button_clicked = true;
    }

    widgets::a11y(&resp, format!("{name}, {kind}, {}", amount.format()));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() && !button_clicked {
        acts.push(Act::Show(a.id));
    }
}
