//! The command registry. Ids are `area.verb`: `txn.create`, `view.screen`, `edit.undo`. Params are JSON objects; each
//! spec's `params` documents them for people, the CLI and MCP. Amounts in params are euros (`12.50`, `"12,50"`,
//! `"€ 1.234,56"`); amounts in answers are integer cents (`-6418`);
//! dates are `YYYY-MM-DD`; accounts are a number, a name, an IBAN or a path like `ING / Holiday`.

mod account;
mod attention;
mod file;
mod import;
mod query;
mod report;
mod review;
mod rule;
mod txn;
mod view;

pub use file::new_book;
use pecunia_model::{AccountId, Money, NaiveDate, TxnId};
pub use query::{account_json, money_json, txn_json};
use serde::Serialize;
use serde_json::Value as Json;

use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &Json) -> Result<Json>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

/// How a command treats undo, the journal and the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Changes the book: undoable, journaled, saved.
    Edit,
    /// Changes the session (view, selection, open file) but is not undone.
    Plain,
    /// Reads only.
    Query,
}

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Transaction"]`. Empty = not in menus or the palette.
    pub menu: &'static [&'static str],
    /// Shortcut as shown to people (`⌘N`); the UI keymap binds it.
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub kind: Kind,
    pub enabled: Enabled,
    pub run: Run,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub kind: Kind,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            params: self.params,
            kind: self.kind,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

macro_rules! cmd {
    ($kind:ident $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec {
            id: $id,
            label: $label,
            menu: &[$($m),*],
            shortcut: $sc,
            params: $params,
            kind: $crate::cmd::Kind::$kind,
            enabled: $en,
            run: $run,
        }
    };
}
pub(crate) use cmd;

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(txn::specs());
        v.extend(review::specs());
        v.extend(import::specs());
        v.extend(account::specs());
        v.extend(rule::specs());
        v.extend(view::specs());
        v.extend(query::specs());
        v.extend(report::specs());
        v.extend(attention::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

// ---------------------------------------------------------------- enabled checks

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    match s.selection {
        Some(id) if s.book().txn(id).is_some() => Ok(()),
        _ => Err("no transaction is selected".into()),
    }
}

// ---------------------------------------------------------------- param helpers

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

pub(crate) fn str_param<'a>(p: &'a Json, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Json::as_str)
}

pub(crate) fn bool_param(p: &Json, key: &str) -> Option<bool> {
    p.get(key).and_then(Json::as_bool)
}

/// A string param that must be a string when present.
pub(crate) fn opt_string(cmd: &str, p: &Json, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a string"))),
    }
}

/// A required string param.
pub(crate) fn req_string(cmd: &str, p: &Json, key: &str) -> Result<String> {
    opt_string(cmd, p, key)?.filter(|s| !s.trim().is_empty()).ok_or_else(|| bad(cmd, format!("missing `{key}`")))
}

/// An account reference: a number, a name, an IBAN or a path (`ING / Holiday`).
pub(crate) fn resolve_account(s: &Session, cmd: &str, v: &Json, key: &str) -> Result<AccountId> {
    let found = match v {
        Json::Number(n) => n.as_u64().map(AccountId).filter(|id| s.book().account(*id).is_some()),
        Json::String(t) => s.book().resolve_account(t),
        _ => return Err(bad(cmd, format!("`{key}` must be an account number or name"))),
    };
    found.ok_or_else(|| bad(cmd, format!("no account {}", short(v))))
}

pub(crate) fn account_param(s: &Session, cmd: &str, p: &Json, key: &str) -> Result<Option<AccountId>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(t)) if t.trim().is_empty() => Ok(None),
        Some(v) => resolve_account(s, cmd, v, key).map(Some),
    }
}

pub(crate) fn req_account(s: &Session, cmd: &str, p: &Json, key: &str) -> Result<AccountId> {
    account_param(s, cmd, p, key)?.ok_or_else(|| bad(cmd, format!("missing `{key}` (an account number or name)")))
}

/// A transaction reference: a number or `#12`.
pub(crate) fn resolve_txn(s: &Session, cmd: &str, v: &Json, key: &str) -> Result<TxnId> {
    let n = match v {
        Json::Number(n) => n.as_u64(),
        Json::String(t) => t.trim().trim_start_matches('#').parse().ok(),
        _ => None,
    };
    n.map(TxnId).filter(|id| s.book().txn(*id).is_some()).ok_or_else(|| bad(cmd, format!("`{key}`: no transaction {}", short(v))))
}

/// The `id` param, or the selected transaction when it's missing.
pub(crate) fn target(s: &Session, cmd: &str, p: &Json) -> Result<TxnId> {
    match p.get("id") {
        None | Some(Json::Null) => {
            s.selection.filter(|id| s.book().txn(*id).is_some()).ok_or_else(|| bad(cmd, "missing `id` and no transaction is selected"))
        }
        Some(v) => resolve_txn(s, cmd, v, "id"),
    }
}

/// An amount in euros: a JSON number (`12.5`) or text (`"12,50"`, `"€ 1.234,56"`).
pub(crate) fn money_param(cmd: &str, p: &Json, key: &str) -> Result<Option<Money>> {
    let text = match p.get(key) {
        None | Some(Json::Null) => return Ok(None),
        Some(Json::Number(n)) => n.to_string(),
        Some(Json::String(s)) => s.clone(),
        Some(_) => return Err(bad(cmd, format!("`{key}` must be an amount in euros, like 12.50"))),
    };
    // A JSON number may come as `1e3`; plain decimals only.
    if text.contains(['e', 'E']) && !text.contains("EUR") {
        return Err(bad(cmd, format!("`{key}`: write the amount as a plain number, like 1000.00")));
    }
    Money::parse(&text).map(Some).map_err(|e| bad(cmd, format!("`{key}`: {e}")))
}

pub(crate) fn req_money(cmd: &str, p: &Json, key: &str) -> Result<Money> {
    money_param(cmd, p, key)?.ok_or_else(|| bad(cmd, format!("missing `{key}` (an amount in euros)")))
}

/// A date `YYYY-MM-DD`; `today` is accepted too.
pub(crate) fn date_param(s: &Session, cmd: &str, p: &Json, key: &str) -> Result<Option<NaiveDate>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(t)) if t.trim().is_empty() => Ok(None),
        Some(Json::String(t)) if t.trim().eq_ignore_ascii_case("today") => Ok(Some(s.today())),
        Some(Json::String(t)) => pecunia_model::parse_date(t).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` is not a date like 2026-10-16"))),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a date like 2026-10-16"))),
    }
}

/// A list param given as an array of strings or numbers, or one comma-separated string.
pub(crate) fn list_param(cmd: &str, p: &Json, key: &str) -> Result<Option<Vec<Json>>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| Json::String(x.into())).collect())),
        Some(Json::Array(a)) if a.len() > 10_000 => Err(bad(cmd, format!("`{key}` has too many entries"))),
        Some(Json::Array(a)) => Ok(Some(a.clone())),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a list"))),
    }
}

/// A value for an error message, cut to a readable length.
pub(crate) fn short(v: &Json) -> String {
    let s = match v {
        Json::String(s) => format!("{s:?}"),
        other => other.to_string(),
    };
    if s.chars().count() > 40 { format!("{}…", s.chars().take(40).collect::<String>()) } else { s }
}

/// The tail of a message after a new rule ran: ", and booked 3 waiting transactions", or nothing.
pub(crate) fn booked_text(booked: usize) -> String {
    match booked {
        0 => String::new(),
        1 => ", and booked 1 waiting transaction".into(),
        n => format!(", and booked {n} waiting transactions"),
    }
}
