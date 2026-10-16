//! Choosing an account, pot or category: a pill that opens a searchable menu, grouped the way people think of money
//! (your accounts, then where money goes, then where it comes from).

use egui::{Align2, Sense, Ui, pos2, vec2};
use pecunia_engine::model::{Account, AccountId, AccountKind, Book, Group};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::widgets;

/// Which accounts a picker offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choices {
    /// Where a bank transaction's money went or came from: categories, and own accounts (a transfer).
    Counter,
    /// Where money can be taken from or put: own accounts, their pots, categories.
    Any,
    /// Own physical accounts only (import into, a pot's account).
    Physical,
    /// The pots of one account.
    PotsOf(AccountId),
    /// Category groups (where a new category goes).
    CategoryGroups,
    /// Categories only (rules answer with one).
    Categories,
}

/// The accounts a picker shows, with a heading for each run of them.
pub fn options(book: &Book, choices: Choices) -> Vec<(&'static str, Vec<(Account, usize)>)> {
    let tree = book.tree();
    let open = |a: &&Account| !a.closed && a.system.is_none();
    let mut own: Vec<(Account, usize)> = Vec::new();
    for a in tree.all_physical().into_iter().filter(open) {
        own.push((a.clone(), 0));
        if choices == Choices::Any {
            let pots = tree.pots(a.id);
            if pots.len() > 1 {
                own.extend(pots.into_iter().filter(|p| !p.unassigned && !p.closed).map(|p| (p.clone(), 1)));
            }
        }
    }
    let cats = |group: Group| -> Vec<(Account, usize)> {
        let mut v = Vec::new();
        for g in tree.roots(group).into_iter().filter(open) {
            v.push((g.clone(), 0));
            v.extend(tree.children(g.id).into_iter().filter(open).map(|c| (c.clone(), 1)));
        }
        v
    };
    match choices {
        Choices::Counter | Choices::Any => {
            vec![("YOUR ACCOUNTS", own), ("WHERE MONEY GOES", cats(Group::Goes)), ("WHERE MONEY COMES FROM", cats(Group::Comes))]
        }
        Choices::Physical => vec![("YOUR ACCOUNTS", tree.all_physical().into_iter().filter(open).map(|a| (a.clone(), 0)).collect())],
        Choices::PotsOf(acc) => vec![("POTS", tree.pots(acc).into_iter().filter(|p| !p.closed).map(|p| (p.clone(), 0)).collect())],
        Choices::CategoryGroups => {
            let groups = |g: Group| tree.roots(g).into_iter().filter(open).map(|a| (a.clone(), 0)).collect::<Vec<_>>();
            vec![("WHERE MONEY GOES", groups(Group::Goes)), ("WHERE MONEY COMES FROM", groups(Group::Comes))]
        }
        Choices::Categories => {
            let only = |g: Group| cats(g).into_iter().filter(|(a, _)| a.kind == AccountKind::Category).collect::<Vec<_>>();
            vec![("WHERE MONEY GOES", only(Group::Goes)), ("WHERE MONEY COMES FROM", only(Group::Comes))]
        }
    }
}

/// Whether an option can be picked (category groups are headings, except when picking a group).
fn pickable(a: &Account, choices: Choices) -> bool {
    match choices {
        Choices::CategoryGroups => a.kind == AccountKind::CategoryGroup,
        _ => a.kind != AccountKind::CategoryGroup,
    }
}

/// The menu: a search box and the options. Returns the chosen account.
pub fn menu(ui: &mut Ui, book: &Book, choices: Choices, current: Option<AccountId>, t: &Tokens) -> Option<AccountId> {
    let mut out = None;
    let search_id = ui.id().with("picker-search");
    let mut q: String = ui.data_mut(|d| d.get_temp(search_id).unwrap_or_default());
    ui.set_min_width(260.0);
    let te = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search…").desired_width(f32::INFINITY));
    if !te.has_focus() && q.is_empty() {
        te.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(search_id, q.clone()));
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    let hit = |a: &Account| words.iter().all(|w| a.name.to_lowercase().contains(w.as_str()));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let mut first: Option<AccountId> = None;
    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
        for (heading, list) in options(book, choices) {
            let shown: Vec<&(Account, usize)> = list
                .iter()
                .filter(|(a, _)| hit(a) || (!pickable(a, choices) && list.iter().any(|(c, _)| c.parent == Some(a.id) && hit(c))))
                .collect();
            if shown.is_empty() {
                continue;
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(heading).font(theme::bold(10.5)).color(t.text_faint));
            for (a, depth) in shown {
                if !pickable(a, choices) {
                    ui.label(egui::RichText::new(&a.name).size(12.0).color(t.text_dim));
                    continue;
                }
                if first.is_none() {
                    first = Some(a.id);
                }
                let indent = *depth as f32 * 14.0;
                let w = ui.available_width().max(220.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 5.0, t.hover);
                }
                icons::account(ui.painter(), pos2(rect.left() + 12.0 + indent, rect.center().y), 13.0, a.kind, a.group, t);
                let name = widgets::elide(ui, &a.name, theme::body(13.0), rect.width() - 60.0 - indent);
                ui.painter().text(pos2(rect.left() + 26.0 + indent, rect.center().y), Align2::LEFT_CENTER, name, theme::body(13.0), t.text);
                if current == Some(a.id) {
                    icons::glyph(ui.painter(), pos2(rect.right() - 12.0, rect.center().y), 11.0, Glyph::Check, t.accent);
                }
                widgets::a11y(&resp, &a.name);
                if resp.clicked() {
                    out = Some(a.id);
                }
            }
        }
        if first.is_none() {
            ui.label(egui::RichText::new("Nothing matches").color(t.text_faint));
        }
    });
    if enter && out.is_none() && !q.is_empty() {
        out = first;
    }
    if out.is_some() {
        ui.data_mut(|d| d.remove::<String>(search_id));
        ui.close();
    }
    out
}

/// An outlined pill showing the chosen account (or `placeholder`); clicking opens [`menu`]. Returns a new choice.
pub fn pill(ui: &mut Ui, book: &Book, choices: Choices, current: Option<AccountId>, placeholder: &str, t: &Tokens) -> Option<AccountId> {
    let a = current.and_then(|id| book.account(id)).cloned();
    let text = a.as_ref().map(|a| book.account_label(a.id)).unwrap_or_else(|| placeholder.to_string());
    let text = widgets::elide(ui, &text, theme::body(13.0), 230.0);
    let r = widgets::pill_outlined(
        ui,
        |p, c| match &a {
            Some(a) => icons::account(p, c, 12.0, a.kind, a.group, t),
            None => icons::glyph(p, c, 12.0, Glyph::ChevronDown, t.text_faint),
        },
        &text,
        a.is_none(),
        t,
    );
    let mut out = None;
    egui::Popup::menu(&r).frame(widgets::popup_frame(t)).width(300.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        out = menu(ui, book, choices, current, t);
    });
    out
}
