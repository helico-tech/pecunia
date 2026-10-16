use pecunia_model::RuleId;
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "rule.create", "New Payee Rule", [], None,
            "payee? (the start of the payee name) and/or iban?, to (a category). Waiting transactions that match are booked at once.",
            always, create),
        cmd!(Edit "rule.update", "Edit Payee Rule", [], None, "id (number), and any of: payee, iban (null clears), to", always, update),
        cmd!(Edit "rule.delete", "Delete Payee Rule", [], None, "id (number)", always, delete),
    ]
}

fn rule_id(s: &Session, cmd: &str, p: &Json) -> Result<RuleId> {
    let id = p.get("id").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().trim_start_matches('#').parse().ok())));
    id.map(RuleId).filter(|id| s.book().rule(*id).is_some()).ok_or_else(|| bad(cmd, "missing or unknown `id` (a rule number)"))
}

fn create(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "rule.create";
    let to = req_account(s, C, p, "to")?;
    let payee = opt_string(C, p, "payee")?;
    let iban = opt_string(C, p, "iban")?;
    let (id, applied) = s.book_mut().create_rule(payee.as_deref(), iban.as_deref(), to)?;
    if applied > 0 {
        s.notify(format!("The new rule booked {applied} waiting transaction{}", if applied == 1 { "" } else { "s" }), true);
    }
    Ok(json!({ "id": id.0, "applied": applied }))
}

fn update(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "rule.update";
    let id = rule_id(s, C, p)?;
    let payee = opt_string(C, p, "payee")?;
    let iban = match p.get("iban") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(Json::String(i)) => Some(Some(i.clone())),
        Some(_) => return Err(bad(C, "`iban` must be a string or null")),
    };
    let to = account_param(s, C, p, "to")?;
    let changed = s.book_mut().update_rule(id, payee.as_deref(), iban.as_ref().map(|i| i.as_deref()), to)?;
    Ok(json!({ "id": id.0, "changed": changed }))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let id = rule_id(s, "rule.delete", p)?;
    s.book_mut().delete_rule(id)?;
    s.notify("Deleted the payee rule", true);
    Ok(json!({ "deleted": id.0 }))
}
