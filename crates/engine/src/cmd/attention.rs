//! Answers to Needs attention on the Overview: "Seen" on fixed-cost news, "Not mine" on an IBAN that looked like an own
//! account. The keys come from `report.overview` (`attention.*[].key`).

use pecunia_model::reports;
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "attention.dismiss", "Remove from Needs Attention", [], None,
            "key (an item's `key` from report.overview's attention). Stops showing that item in Needs attention.",
            always, dismiss),
        cmd!(Edit "attention.restore", "Show Answered Attention Items Again", ["Review"], None,
            "Shows every item in Needs attention that was marked seen or not mine.",
            has_dismissed, restore),
    ]
}

fn has_dismissed(s: &Session) -> std::result::Result<(), String> {
    if s.book().meta.dismissed.is_empty() { Err("Nothing in Needs attention was answered".into()) } else { Ok(()) }
}

fn dismiss(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "attention.dismiss";
    let key = req_string(C, p, "key")?;
    let open = reports::needs_attention(s.book(), s.today());
    if !open.keys().any(|k| k == key) {
        return Err(bad(C, "`key` is not an open item in Needs attention"));
    }
    s.book_mut().dismiss(&key)?;
    s.notify("Removed from Needs attention", true);
    Ok(json!({ "dismissed": key }))
}

fn restore(s: &mut Session, _: &Json) -> Result<Json> {
    let n = s.book_mut().restore_dismissed();
    s.notify(format!("{n} answered item{} in Needs attention again", if n == 1 { " is" } else { "s are" }), true);
    Ok(json!({ "restored": n }))
}
