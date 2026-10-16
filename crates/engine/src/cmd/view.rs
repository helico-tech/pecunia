use pecunia_model::register::Filter;
use pecunia_model::{ReviewKind, TxnStatus};
use serde_json::{Value as Json, json};

use super::*;
use crate::view::{ReportKind, Screen};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Plain "view.screen", "Go To", [], None, "screen: overview, review, transactions, pots, reports, accounts, rules or imports", always, screen),
        cmd!(Plain "view.overview", "Overview", ["View"], Some("⌘1"), "", always, |s, _| go(s, Screen::Overview)),
        cmd!(Plain "view.review", "Review", ["View"], Some("⌘2"), "", always, |s, _| go(s, Screen::Review)),
        cmd!(Plain "view.transactions", "Transactions", ["View"], Some("⌘3"), "", always, |s, _| go(s, Screen::Transactions)),
        cmd!(Plain "view.pots", "Pots", ["View"], Some("⌘4"), "", always, |s, _| go(s, Screen::Pots)),
        cmd!(Plain "view.reports", "Reports", ["View"], Some("⌘5"), "", always, |s, _| go(s, Screen::Reports)),
        cmd!(Plain "view.accounts", "Accounts and Categories", ["View"], Some("⌘6"), "", always, |s, _| go(s, Screen::Accounts)),
        cmd!(Plain "view.rules", "Payee Rules", ["View"], None, "", always, |s, _| go(s, Screen::Rules)),
        cmd!(Plain "view.imports", "Imports", ["View"], None, "", always, |s, _| go(s, Screen::Imports)),
        cmd!(Plain "view.filter", "Filter Transactions", [], None,
            "replace? (bool: start from no filter), text?, account? (account, pot, category or group; null clears), review? (bool), status? (auto|to_review|reviewed|null), from?, to? (dates), import? (number). Shows the Transactions screen.",
            always, filter),
        cmd!(Plain "view.clearFilter", "Clear Filters", ["View"], None, "", always, clear_filter),
        cmd!(Plain "view.reviewTab", "Review Tab", [], None, "tab: duplicate, transfer, suggested, unknown, or null for all", always, review_tab),
        cmd!(Plain "view.report", "Show Report", [], None,
            "report? (incomeExpenses|netWorth|cashFlow|fixedCosts|bankCheck), from?, to? (dates; null = this month). Shows the Reports screen.",
            always, report),
    ]
}

fn go(s: &mut Session, screen: Screen) -> Result<Json> {
    s.view.screen = screen;
    Ok(json!({ "screen": screen }))
}

fn screen(s: &mut Session, p: &Json) -> Result<Json> {
    let screen = str_param(p, "screen")
        .and_then(Screen::parse)
        .ok_or_else(|| bad("view.screen", "`screen` is overview, review, transactions, pots, reports, accounts, rules or imports"))?;
    go(s, screen)
}

fn filter(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "view.filter";
    let mut f = if bool_param(p, "replace").unwrap_or(false) { Filter::default() } else { s.view.filter.clone() };
    if let Some(t) = opt_string(C, p, "text")? {
        f.text = t.chars().take(200).collect();
    }
    if p.get("account").is_some() {
        f.account = account_param(s, C, p, "account")?;
    }
    if let Some(r) = bool_param(p, "review") {
        f.review = r;
    }
    match p.get("status") {
        None => {}
        Some(Json::Null) => f.status = None,
        Some(Json::String(t)) if t.trim().is_empty() => f.status = None,
        Some(Json::String(t)) => f.status = Some(TxnStatus::parse(t).ok_or_else(|| bad(C, "`status` is auto, to_review or reviewed"))?),
        Some(_) => return Err(bad(C, "`status` must be a string")),
    }
    if p.get("from").is_some() {
        f.from = date_param(s, C, p, "from")?;
    }
    if p.get("to").is_some() {
        f.to = date_param(s, C, p, "to")?;
    }
    match p.get("import") {
        None => {}
        Some(Json::Null) => f.import = None,
        Some(v) => f.import = Some(pecunia_model::ImportId(v.as_u64().ok_or_else(|| bad(C, "`import` must be a number"))?)),
    }
    s.view.filter = f;
    s.view.screen = Screen::Transactions;
    Ok(json!({ "filter": s.view.filter }))
}

fn clear_filter(s: &mut Session, _: &Json) -> Result<Json> {
    s.view.filter = Filter::default();
    Ok(json!({}))
}

fn review_tab(s: &mut Session, p: &Json) -> Result<Json> {
    s.view.review_tab = match p.get("tab") {
        None | Some(Json::Null) => None,
        Some(Json::String(t)) if t.trim().is_empty() || t.trim().eq_ignore_ascii_case("all") => None,
        Some(Json::String(t)) => {
            Some(ReviewKind::parse(t).ok_or_else(|| bad("view.reviewTab", "`tab` is duplicate, transfer, suggested or unknown"))?)
        }
        Some(_) => return Err(bad("view.reviewTab", "`tab` must be a string")),
    };
    s.view.screen = Screen::Review;
    Ok(json!({ "tab": s.view.review_tab }))
}

fn report(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "view.report";
    if let Some(r) = opt_string(C, p, "report")? {
        s.view.report = ReportKind::parse(&r).ok_or_else(|| bad(C, "`report` is incomeExpenses, netWorth, cashFlow, fixedCosts or bankCheck"))?;
    }
    if p.get("from").is_some() {
        s.view.from = date_param(s, C, p, "from")?;
    }
    if p.get("to").is_some() {
        s.view.to = date_param(s, C, p, "to")?;
    }
    if let (Some(a), Some(b)) = (s.view.from, s.view.to)
        && b < a
    {
        s.view.to = Some(a);
    }
    s.view.screen = Screen::Reports;
    Ok(json!({ "report": s.view.report, "from": s.view.from, "to": s.view.to }))
}
