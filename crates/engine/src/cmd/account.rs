use pecunia_model::{AccountKind, AccountPatch, Group, NewAccount, catalogue};
use serde_json::{Value as Json, json};

use super::*;
use crate::UiRequest;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "account.create", "New Account…", ["Account"], None,
            "kind (current|savings|cash|property|credit_card|loan|pot|category_group|category), name, parent? (a pot's account, a category's group), group? (goes|comes, for a category group), iban?, description?, goal? (euros, pots), goalDate?, balance? (euros: the starting balance), balanceDate? (default today). Without kind and name the UI asks.",
            always, create),
        cmd!(Edit "account.update", "Edit Account", [], None,
            "id (number or name), and any of: name, description, iban (null clears), goal (null clears), goalDate, parent (move a category to another group), sort, closed (bool)",
            always, update),
        cmd!(Edit "account.delete", "Delete Account", [], None, "id. Only accounts without transactions; close an account to keep its history.", always, delete),
        cmd!(Edit "account.setBalance", "Set Starting Balance", [], None,
            "id (an account or pot), amount (euros: what you have or owe, positive; 0 removes it), date? (default today)",
            always, set_balance),
        cmd!(Edit "account.setValue", "Set a New Value", [], None, "id (e.g. the house), amount (euros), date? (default today)", always, set_value),
        cmd!(Edit "category.add", "Add a Usual Category", [], None, "name (from the list of usual household categories; see `categories.catalogue`)", always, add_category),
    ]
}

fn create(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "account.create";
    let kind = match opt_string(C, p, "kind")? {
        Some(k) => Some(
            AccountKind::parse(&k)
                .ok_or_else(|| bad(C, "`kind` is current, savings, cash, property, credit_card, loan, pot, category_group or category"))?,
        ),
        None => None,
    };
    let name = opt_string(C, p, "name")?.filter(|n| !n.trim().is_empty());
    let (Some(kind), Some(name)) = (kind, name) else {
        s.request(UiRequest::Dialog("newAccount".into(), if p.is_object() { p.clone() } else { json!({}) }));
        return Ok(json!({ "dialog": "newAccount" }));
    };
    let group = match opt_string(C, p, "group")? {
        Some(g) => Some(Group::parse(&g).ok_or_else(|| bad(C, "`group` is goes or comes"))?),
        None => None,
    };
    let new = NewAccount {
        kind: Some(kind),
        name,
        group,
        parent: account_param(s, C, p, "parent")?,
        description: opt_string(C, p, "description")?.unwrap_or_default(),
        iban: opt_string(C, p, "iban")?,
        goal: money_param(C, p, "goal")?,
        goal_date: date_param(s, C, p, "goalDate")?,
    };
    let id = s.book_mut().create_account(new)?;
    if let Some(balance) = money_param(C, p, "balance")?.filter(|b| !b.is_zero()) {
        let date = date_param(s, C, p, "balanceDate")?.unwrap_or_else(|| s.today());
        let now = s.clock();
        s.book_mut().set_starting_balance(id, balance, date, now)?;
    }
    Ok(json!({ "id": id.0, "name": s.book().account_label(id) }))
}

fn update(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "account.update";
    let id = req_account(s, C, p, "id")?;
    let iban = match p.get("iban") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(Json::String(i)) if i.trim().is_empty() => Some(None),
        Some(Json::String(i)) => Some(Some(i.clone())),
        Some(_) => return Err(bad(C, "`iban` must be a string or null")),
    };
    let goal = match p.get("goal") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(_) => Some(money_param(C, p, "goal")?.filter(|g| !g.is_zero())),
    };
    let goal_date = match p.get("goalDate") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(_) => Some(date_param(s, C, p, "goalDate")?),
    };
    let sort = match p.get("sort") {
        None | Some(Json::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| bad(C, "`sort` must be a number"))?.clamp(-1_000_000, 1_000_000) as i32),
    };
    let patch = AccountPatch {
        name: opt_string(C, p, "name")?,
        description: opt_string(C, p, "description")?,
        iban,
        goal,
        goal_date,
        parent: account_param(s, C, p, "parent")?,
        sort,
        closed: bool_param(p, "closed"),
    };
    let changed = s.book_mut().update_account(id, patch)?;
    Ok(json!({ "id": id.0, "changed": changed }))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let id = req_account(s, "account.delete", p, "id")?;
    let name = s.book().account_label(id);
    s.book_mut().delete_account(id)?;
    s.notify(format!("Deleted {name}"), true);
    Ok(json!({ "deleted": name }))
}

fn set_balance(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "account.setBalance";
    let id = req_account(s, C, p, "id")?;
    let amount = req_money(C, p, "amount")?;
    let date = date_param(s, C, p, "date")?.unwrap_or_else(|| s.today());
    let now = s.clock();
    let txn = s.book_mut().set_starting_balance(id, amount, date, now)?;
    Ok(json!({ "id": id.0, "transaction": txn.map(|t| t.0) }))
}

fn set_value(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "account.setValue";
    let id = req_account(s, C, p, "id")?;
    let amount = req_money(C, p, "amount")?;
    let date = date_param(s, C, p, "date")?.unwrap_or_else(|| s.today());
    let now = s.clock();
    let txn = s.book_mut().set_value(id, amount, date, now)?;
    Ok(json!({ "id": id.0, "transaction": txn.map(|t| t.0) }))
}

fn add_category(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "category.add";
    let name = req_string(C, p, "name")?;
    let entry = catalogue::entry(&name)
        .ok_or_else(|| bad(C, format!("{} is not a usual category; use account.create", short(&Json::String(name.clone())))))?;
    let id = catalogue::add_entry(s.book_mut(), entry)?;
    Ok(json!({ "id": id.0, "name": entry.name, "group": entry.group_name }))
}
