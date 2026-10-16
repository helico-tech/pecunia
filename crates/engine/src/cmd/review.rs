use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "review.acceptSuggestions", "Accept All Suggestions", ["Review"], None,
            "ids? (list of transaction numbers; default every suggestion in Review). Books each one on its top guess.",
            always, accept),
        cmd!(Edit "review.answerAllLike", "Same Answer for This Payee", [], None,
            "id? (default the selection), to (category or own account): answers every waiting transaction of the same payee",
            always, answer_all_like),
        cmd!(Edit "review.notATransfer", "Not a Transfer", [], None, "id? (default the selection)", always, not_a_transfer),
        cmd!(Edit "review.duplicate", "Decide About a Possible Duplicate", [], None,
            "id? (default the selection), keep (bool): true keeps both, false deletes this copy",
            always, duplicate),
    ]
}

fn accept(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "review.acceptSuggestions";
    let ids = match list_param(C, p, "ids")? {
        None => None,
        Some(v) => Some(v.iter().map(|x| resolve_txn(s, C, x, "ids")).collect::<Result<Vec<_>>>()?),
    };
    let n = s.book_mut().accept_suggestions(ids.as_deref())?;
    if n > 0 {
        s.notify(format!("Accepted {n} suggestion{}", if n == 1 { "" } else { "s" }), true);
    }
    Ok(json!({ "accepted": n }))
}

fn answer_all_like(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "review.answerAllLike";
    let id = target(s, C, p)?;
    let to = req_account(s, C, p, "to")?;
    let n = s.book_mut().answer_all_like(id, to)?;
    Ok(json!({ "answered": n }))
}

fn not_a_transfer(s: &mut Session, p: &Json) -> Result<Json> {
    let id = target(s, "review.notATransfer", p)?;
    s.book_mut().not_a_transfer(id)?;
    Ok(json!({ "id": id.0 }))
}

fn duplicate(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "review.duplicate";
    let id = target(s, C, p)?;
    let keep = bool_param(p, "keep").ok_or_else(|| bad(C, "missing `keep` (true keeps both, false deletes this copy)"))?;
    let original = s.book_mut().resolve_duplicate(id, keep)?;
    if !keep && s.selection == Some(id) {
        s.selection = original;
    }
    Ok(json!({ "id": id.0, "kept": keep }))
}
