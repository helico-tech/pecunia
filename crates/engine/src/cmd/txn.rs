use pecunia_model::classify::RuleChange;
use pecunia_model::{NewTxn, Posting, TxnPatch};
use serde_json::{Value as Json, json};

use super::*;
use crate::UiRequest;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "txn.create", "New Transaction…", ["Transaction"], Some("⌘N"),
            "payee, from (account the money leaves), to (account or category it goes to), amount (euros), date? (default today), note?, who?. Without them the UI asks.",
            always, create),
        cmd!(Edit "txn.update", "Edit Transaction", [], None, "id? (default the selection), and any of: date, payee, note, who (null to clear)", always, update),
        cmd!(Edit "txn.answer", "Set Category", [], None,
            "id? (default the selection), to (a category, or another own account for a transfer), always? (bool: also make a payee rule)",
            always, answer),
        cmd!(Edit "txn.confirm", "Mark as Checked", ["Transaction"], Some("⌘↩"), "id? (default the selection). Keeps where it is booked.", always, confirm),
        cmd!(Edit "txn.split", "Split Transaction", [], None,
            "id? (default the selection), parts: [{account, amount, memo?}] that add up to the transaction (mortgage = interest + repayment)",
            always, split),
        cmd!(Edit "txn.moveToPot", "Pay From Pot", [], None, "id? (default the selection), pot (a pot of the same account)", always, move_to_pot),
        cmd!(Edit "txn.delete", "Delete Transaction", ["Transaction"], Some("⌘⌫"), "id? (default the selection)", always, delete),
        cmd!(Edit "pot.move", "Move Money Between Pots…", ["Transaction"], None,
            "from (pot or account), to (pot or account), amount (euros), date? (default today), note?. Both in one account.",
            always, pot_move),
        cmd!(Plain "txn.select", "Select Transaction", [], None, "id (a number), or null to clear", always, select),
        cmd!(Plain "edit.undo", "Undo", ["Edit"], Some("⌘Z"), "", can_undo, undo),
        cmd!(Plain "edit.redo", "Redo", ["Edit"], Some("⇧⌘Z"), "", can_redo, redo),
    ]
}

fn can_undo(s: &Session) -> std::result::Result<(), String> {
    if s.can_undo() { Ok(()) } else { Err("nothing to undo".into()) }
}

fn can_redo(s: &Session) -> std::result::Result<(), String> {
    if s.can_redo() { Ok(()) } else { Err("nothing to redo".into()) }
}

fn create(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txn.create";
    let from = account_param(s, C, p, "from")?;
    let to = account_param(s, C, p, "to")?;
    let amount = money_param(C, p, "amount")?;
    let (Some(from), Some(to), Some(amount)) = (from, to, amount) else {
        let mut defaults = p.clone();
        if !defaults.is_object() {
            defaults = json!({});
        }
        s.request(UiRequest::Dialog("newTxn".into(), defaults));
        return Ok(json!({ "dialog": "newTxn" }));
    };
    let date = date_param(s, C, p, "date")?.unwrap_or_else(|| s.today());
    let new = NewTxn {
        date,
        payee: opt_string(C, p, "payee")?.unwrap_or_default(),
        from,
        to,
        amount,
        note: opt_string(C, p, "note")?.unwrap_or_default(),
        who: opt_string(C, p, "who")?,
    };
    let now = s.clock();
    let id = s.book_mut().add_txn(new, now)?;
    if bool_param(p, "select").unwrap_or(true) {
        s.selection = Some(id);
    }
    Ok(json!({ "id": id.0 }))
}

fn update(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txn.update";
    let id = target(s, C, p)?;
    let who = match p.get("who") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(Json::String(w)) => Some(Some(w.clone())),
        Some(_) => return Err(bad(C, "`who` must be a string or null")),
    };
    let patch = TxnPatch { date: date_param(s, C, p, "date")?, payee: opt_string(C, p, "payee")?, note: opt_string(C, p, "note")?, who };
    let changed = s.book_mut().update_txn(id, patch)?;
    Ok(json!({ "id": id.0, "changed": changed }))
}

fn answer(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txn.answer";
    let id = target(s, C, p)?;
    let to =
        account_param(s, C, p, "to")?.or(account_param(s, C, p, "category")?).ok_or_else(|| bad(C, "missing `to` (a category or own account)"))?;
    let always = bool_param(p, "always").unwrap_or(false);
    let change = s.book_mut().answer(id, to, always)?;
    let rule = match change {
        RuleChange::Create { payee_prefix, .. } => {
            let booked = s.book_mut().apply_rules();
            s.notify(format!("New payee rule: {payee_prefix} → {}{}", s.book().account_label(to), booked_text(booked)), true);
            json!({ "created": payee_prefix, "booked": booked })
        }
        RuleChange::MarkMixed(r) => json!({ "mixed": r.0 }),
        RuleChange::None => Json::Null,
    };
    Ok(json!({ "id": id.0, "to": s.book().account_label(to), "rule": rule }))
}

fn confirm(s: &mut Session, p: &Json) -> Result<Json> {
    let id = target(s, "txn.confirm", p)?;
    let changed = s.book_mut().confirm(id)?;
    Ok(json!({ "id": id.0, "changed": changed }))
}

fn split(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txn.split";
    let id = target(s, C, p)?;
    let parts = match p.get("parts") {
        Some(Json::Array(a)) if a.len() <= pecunia_model::limits::POSTINGS => a.clone(),
        Some(Json::Array(_)) => return Err(bad(C, "too many parts")),
        _ => return Err(bad(C, "missing `parts`: a list of {account, amount}")),
    };
    let mut postings = Vec::new();
    for part in &parts {
        let account = req_account(s, C, part, "account")?;
        let amount = req_money(C, part, "amount")?;
        let memo = opt_string(C, part, "memo")?.unwrap_or_default().chars().take(pecunia_model::limits::NAME).collect();
        postings.push(Posting { account, amount, memo });
    }
    s.book_mut().split_txn(id, &postings)?;
    Ok(json!({ "id": id.0, "parts": postings.len() }))
}

fn move_to_pot(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txn.moveToPot";
    let id = target(s, C, p)?;
    let pot = req_account(s, C, p, "pot")?;
    s.book_mut().move_to_pot(id, pot)?;
    Ok(json!({ "id": id.0, "pot": s.book().account_label(pot) }))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let id = target(s, "txn.delete", p)?;
    let t = s.book_mut().delete_txn(id)?;
    if s.selection == Some(id) {
        s.selection = None;
    }
    s.notify(format!("Deleted {} of {}", t.payee, t.size().format()), true);
    Ok(json!({ "deleted": id.0 }))
}

fn pot_move(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pot.move";
    let from = account_param(s, C, p, "from")?;
    let to = account_param(s, C, p, "to")?;
    let amount = money_param(C, p, "amount")?;
    let (Some(from), Some(to), Some(amount)) = (from, to, amount) else {
        s.request(UiRequest::Dialog("potMove".into(), if p.is_object() { p.clone() } else { json!({}) }));
        return Ok(json!({ "dialog": "potMove" }));
    };
    let date = date_param(s, C, p, "date")?.unwrap_or_else(|| s.today());
    let note = opt_string(C, p, "note")?.unwrap_or_default();
    let now = s.clock();
    let id = s.book_mut().pot_move(from, to, amount, date, &note, now)?;
    Ok(json!({ "id": id.0 }))
}

fn select(s: &mut Session, p: &Json) -> Result<Json> {
    s.selection = match p.get("id") {
        None | Some(Json::Null) => None,
        Some(v) => Some(resolve_txn(s, "txn.select", v, "id")?),
    };
    Ok(json!({ "selected": s.selection.map(|id| id.0) }))
}

fn undo(s: &mut Session, _: &Json) -> Result<Json> {
    let label = s.undo_step().ok_or_else(|| bad("edit.undo", "nothing to undo"))?;
    Ok(json!({ "undone": label }))
}

fn redo(s: &mut Session, _: &Json) -> Result<Json> {
    let label = s.redo_step().ok_or_else(|| bad("edit.redo", "nothing to redo"))?;
    Ok(json!({ "redone": label }))
}
