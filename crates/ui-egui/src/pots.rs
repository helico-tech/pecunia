//! The Pots screen: every account with money you have, its pots with balances and goals, and moving money between them.
//! The balances come from [`reports::net_worth`], cached while the book and the date stay the same.

use std::rc::Rc;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use pecunia_engine::model::reports::{self, GoalProgress, PhysicalBalance, PotBalance};
use pecunia_engine::model::{AccountId, AccountKind, Book, Group, Money, NaiveDate, Txn};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::theme::{self, Tokens};
use crate::{PecuniaApp, widgets};

/// Wider than this, the cards stand in two columns.
const WIDE: f32 = 900.0;
const GAP: f32 = 16.0;
const PAD: f32 = 20.0;
const HEAD_H: f32 = 64.0;
const POT_H: f32 = 44.0;
const GOAL_H: f32 = 70.0;
const FOOT_H: f32 = 40.0;

type Key = (u64, NaiveDate, usize);

thread_local! {
    static CACHE: std::cell::RefCell<Option<(Key, Rc<Vec<PhysicalBalance>>)>> = const { std::cell::RefCell::new(None) };
}

/// The open accounts with money you have, with their pots (Unassigned first).
fn accounts(app: &PecuniaApp) -> Rc<Vec<PhysicalBalance>> {
    let book = app.session.book();
    let today = app.session.today();
    let key: Key = (app.session.revision(), today, book as *const Book as usize);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_ref() {
            Some((k, d)) if *k == key => d.clone(),
            _ => {
                let tree = book.tree();
                let txns: Vec<&Txn> = book.txns().map(|t| t.as_ref()).collect();
                let open: Vec<AccountId> = tree.physical_accounts(Group::Have).into_iter().filter(|a| !a.closed).map(|a| a.id).collect();
                let have: Vec<PhysicalBalance> =
                    reports::net_worth(&tree, &txns, today).have.into_iter().filter(|l| open.contains(&l.account)).collect();
                let d = Rc::new(have);
                *c = Some((key, d.clone()));
                d
            }
        }
    })
}

/// What a click asks for; run after drawing.
enum Act {
    Run(&'static str, Json),
    Show(AccountId),
    Dialog(&'static str, Json),
}

pub fn show(app: &mut PecuniaApp, ui: &mut Ui, t: &Tokens) {
    let list = accounts(app);
    let mut acts: Vec<Act> = Vec::new();
    title_row(ui, &list, t, &mut acts);

    if list.is_empty() {
        crate::register::centered(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            icons::glyph(ui.painter(), r.center(), 28.0, Glyph::Jar, t.text_faint);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("No accounts with money yet").font(theme::bold(16.0)).color(t.text));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Add a current account, savings account or cash, then divide its money over pots.").size(13.0).color(t.text_dim),
            );
            ui.add_space(12.0);
            if widgets::primary_button(ui, "New account", t).clicked() {
                acts.push(Act::Dialog("newAccount", json!({})));
            }
        });
    } else {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(PAD as i8)).show(ui, |ui| {
                let w = ui.available_width();
                if w >= WIDE {
                    two_columns(ui, &list, w, t, &mut acts);
                } else {
                    for a in list.iter() {
                        account_card(ui, a, w, t, &mut acts);
                        ui.add_space(GAP);
                    }
                }
            });
        });
    }

    for a in acts {
        match a {
            Act::Run(id, p) => {
                app.run_any(id, p);
            }
            Act::Show(id) => app.show_account(id),
            Act::Dialog(name, p) => app.open_dialog(name, p),
        }
    }
}

fn title_row(ui: &mut Ui, list: &[PhysicalBalance], t: &Tokens, acts: &mut Vec<Act>) {
    let full_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(full_w, 52.0), Sense::hover());
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.label(egui::RichText::new("Pots").font(theme::bold(18.0)).color(t.text));
    // Phone layout: only the title and Move money; New account is in the menu.
    let compact = crate::is_compact(ui.ctx());
    if !list.is_empty() && !compact {
        let in_pots: Money = list.iter().map(|a| a.in_pots).sum();
        row.add_space(8.0);
        row.label(egui::RichText::new(format!("{} set aside in pots", in_pots.format())).size(12.5).color(t.text_dim));
    }
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !list.is_empty() && widgets::primary_button(ui, "Move money", t).on_hover_text("Move money from one pot to another").clicked() {
            acts.push(Act::Run("ui.potMove", json!({})));
        }
        if !compact && widgets::secondary_button(ui, "New account", t).clicked() {
            acts.push(Act::Dialog("newAccount", json!({})));
        }
    });
}

fn pot_height(p: &PotBalance) -> f32 {
    if p.goal.is_some() { GOAL_H } else { POT_H }
}

/// How tall a card is, to balance the two columns.
fn card_height(a: &PhysicalBalance) -> f32 {
    HEAD_H + a.pots.iter().map(pot_height).sum::<f32>() + FOOT_H + 34.0
}

/// Cards in two columns, each card into the shorter one.
fn two_columns(ui: &mut Ui, list: &[PhysicalBalance], w: f32, t: &Tokens, acts: &mut Vec<Act>) {
    let col_w = ((w - GAP) / 2.0).floor();
    let mut columns: [Vec<&PhysicalBalance>; 2] = [Vec::new(), Vec::new()];
    let mut heights = [0.0f32; 2];
    for a in list {
        let i = usize::from(heights.get(1) < heights.first());
        if let (Some(c), Some(h)) = (columns.get_mut(i), heights.get_mut(i)) {
            c.push(a);
            *h += card_height(a) + GAP;
        }
    }
    let origin = ui.cursor().min;
    let mut bottom = origin.y;
    for (i, col) in columns.iter().enumerate() {
        let slot = Rect::from_min_size(pos2(origin.x + i as f32 * (col_w + GAP), origin.y), vec2(col_w, 100_000.0));
        let mut child =
            ui.new_child(egui::UiBuilder::new().id_salt(("pots-column", i)).max_rect(slot).layout(egui::Layout::top_down(egui::Align::Min)));
        for a in col {
            account_card(&mut child, a, col_w, t, acts);
            child.add_space(GAP);
        }
        bottom = bottom.max(child.min_rect().bottom());
    }
    ui.allocate_exact_size(vec2(w, bottom - origin.y), Sense::hover());
}

fn account_card(ui: &mut Ui, a: &PhysicalBalance, w: f32, t: &Tokens, acts: &mut Vec<Act>) {
    widgets::card(t).inner_margin(egui::Margin { left: 16, right: 16, top: 6, bottom: 6 }).show(ui, |ui| {
        ui.set_width(w - 34.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.push_id(("pots-card", a.account.0), |ui| {
            card_head(ui, a, t, acts);
            for p in &a.pots {
                pot_row(ui, p, t, acts);
            }
            card_foot(ui, a, t, acts);
        });
    });
}

fn card_head(ui: &mut Ui, a: &PhysicalBalance, t: &Tokens, acts: &mut Vec<Act>) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, HEAD_H), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect.expand2(vec2(8.0, -4.0)), 7.0, t.hover);
    }
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    let cy = rect.center().y;
    icons::account(p, pos2(rect.left() + 14.0, cy), 26.0, a.kind, Group::Have, t);
    let col = if a.balance.is_negative() { t.danger } else { t.text };
    let bg = p.layout_no_wrap(a.balance.format(), theme::bold(17.0), col);
    let bw = bg.size().x;
    p.galley(pos2(rect.right() - bw, cy - 9.0 - bg.size().y / 2.0), bg, col);
    let unassigned = a.pots.iter().find(|x| x.unassigned).map(|x| x.balance).unwrap_or(Money::ZERO);
    let sub = if a.pots.iter().any(|x| !x.unassigned) {
        format!("{} in pots · {} unassigned", a.in_pots.format(), unassigned.format())
    } else {
        "No pots yet".to_string()
    };
    let sw = p.layout_no_wrap(sub.clone(), theme::body(11.5), t.text_dim).size().x;
    let kw = p.layout_no_wrap(a.kind.label().to_string(), theme::body(11.5), t.text_dim).size().x;
    let name = widgets::elide(ui, &a.name, theme::bold(15.0), (rect.width() - 40.0 - bw - 16.0).max(40.0));
    let p = ui.painter();
    p.text(pos2(rect.left() + 38.0, cy - 9.0), Align2::LEFT_CENTER, name, theme::bold(15.0), t.text);
    if 38.0 + kw + 16.0 + sw <= rect.width() {
        p.text(pos2(rect.right(), cy + 10.0), Align2::RIGHT_CENTER, sub, theme::body(11.5), t.text_dim);
        p.text(pos2(rect.left() + 38.0, cy + 10.0), Align2::LEFT_CENTER, a.kind.label(), theme::body(11.5), t.text_dim);
    } else {
        // A narrow card (a phone): the pots line replaces the account kind.
        let sub = widgets::elide(ui, &sub, theme::body(11.5), rect.width() - 38.0);
        ui.painter().text(pos2(rect.left() + 38.0, cy + 10.0), Align2::LEFT_CENTER, sub, theme::body(11.5), t.text_dim);
    }
    widgets::a11y(&resp, &a.name);
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show its transactions").clicked() {
        acts.push(Act::Show(a.account));
    }
}

/// "€ 200,00 to go · € 25,00 per month until June 2027".
fn goal_text(g: &GoalProgress) -> String {
    if g.reached {
        return match g.reached_on {
            Some(d) => format!("Goal of {} reached on {}", g.goal.format(), d.format("%-d %b %Y")),
            None => format!("Goal of {} reached", g.goal.format()),
        };
    }
    let mut s = format!("{} to go", g.to_go.format());
    match (g.per_month, g.goal_date) {
        (Some(pm), Some(d)) => s.push_str(&format!(" · {} per month until {}", pm.format(), d.format("%B %Y"))),
        (None, Some(d)) => s.push_str(&format!(" · was due {}", d.format("%B %Y"))),
        _ => {}
    }
    s
}

fn pot_row(ui: &mut Ui, pot: &PotBalance, t: &Tokens, acts: &mut Vec<Act>) {
    let w = ui.available_width();
    let h = pot_height(pot);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    // Buttons on the right: move money (every pot) and edit (not Unassigned).
    let cy = if pot.goal.is_some() { rect.top() + 20.0 } else { rect.center().y };
    let buttons = Rect::from_min_max(pos2(rect.right() - 60.0, cy - 14.0), pos2(rect.right(), cy + 14.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::right_to_left(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 2.0;
    let edit = !pot.unassigned && widgets::icon_button(&mut child, Glyph::More, &format!("Edit {}", pot.name), t).clicked();
    let moved = widgets::icon_button(&mut child, Glyph::Swap, &format!("Move money from {}", pot.name), t).clicked();
    let hit = Rect::from_min_max(rect.min, pos2(rect.right() - 64.0, rect.bottom()));
    let resp = ui.interact(hit, ui.id().with(("pot", pot.account.0)), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(hit.expand2(vec2(8.0, -2.0)), 6.0, t.hover);
    }
    p.line_segment([pos2(rect.left() + 28.0, rect.bottom()), pos2(rect.right(), rect.bottom())], Stroke::new(1.0, t.border.gamma_multiply(0.7)));
    let name_col = if pot.unassigned { t.text_dim } else { t.text };
    if pot.unassigned {
        p.circle_stroke(pos2(rect.left() + 14.0, cy), 6.5, Stroke::new(1.4, t.border_strong));
    } else {
        icons::account(p, pos2(rect.left() + 14.0, cy), 15.0, AccountKind::Pot, Group::Have, t);
    }
    let amount_col = if pot.balance.is_negative() {
        t.danger
    } else if pot.balance.is_zero() {
        t.text_faint
    } else {
        t.text
    };
    let ag = p.layout_no_wrap(pot.balance.format(), theme::body(13.5), amount_col);
    let aw = ag.size().x;
    p.galley(pos2(hit.right() - 4.0 - aw, cy - ag.size().y / 2.0), ag, amount_col);
    let label = if pot.unassigned { "Unassigned · not in a pot".to_string() } else { pot.name.clone() };
    let name = widgets::elide(ui, &label, theme::body(13.5), (hit.width() - 40.0 - aw - 16.0).max(40.0));
    ui.painter().text(pos2(rect.left() + 32.0, cy), Align2::LEFT_CENTER, name, theme::body(13.5), name_col);

    if let Some(g) = &pot.goal {
        let bar_y = rect.top() + 40.0;
        let bar_w = hit.width() - 32.0 - 4.0;
        let track = Rect::from_min_size(pos2(rect.left() + 32.0, bar_y - 3.0), vec2(bar_w.max(10.0), 6.0));
        let col = if g.reached { t.positive } else { t.accent };
        let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(track));
        widgets::bar(&mut bar, track.width(), g.percent as f32 / 100.0, col, t);
        let text_col = if g.reached { t.positive } else { t.text_dim };
        let line = widgets::elide(ui, &goal_text(g), theme::body(11.5), bar_w - 50.0);
        let p = ui.painter();
        p.text(pos2(track.left(), bar_y + 15.0), Align2::LEFT_CENTER, line, theme::body(11.5), text_col);
        p.text(pos2(track.right(), bar_y + 15.0), Align2::RIGHT_CENTER, format!("{}%", g.percent.min(999)), theme::bold(11.5), text_col);
    }

    widgets::a11y(&resp, &pot.name);
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show its transactions").clicked() {
        acts.push(Act::Show(pot.account));
    }
    if moved {
        acts.push(Act::Run("ui.potMove", json!({ "from": pot.account.0 })));
    }
    if edit {
        acts.push(Act::Run("ui.editAccount", json!({ "id": pot.account.0 })));
    }
}

fn card_foot(ui: &mut Ui, a: &PhysicalBalance, t: &Tokens, acts: &mut Vec<Act>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), FOOT_H), Sense::click());
    let p = ui.painter();
    let col: Color32 = if resp.hovered() { t.text } else { t.text_dim };
    if resp.hovered() {
        p.rect_filled(rect.expand2(vec2(8.0, -6.0)), 6.0, t.hover);
    }
    icons::glyph(p, pos2(rect.left() + 14.0, rect.center().y), 12.0, Glyph::Plus, col);
    p.text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, "New pot", theme::body(13.0), col);
    widgets::a11y(&resp, format!("New pot in {}", a.name));
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        acts.push(Act::Dialog("newAccount", json!({ "kind": "pot", "parent": a.account.0 })));
    }
}
