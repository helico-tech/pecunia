//! Reports as queries. Amounts in the answers are integer cents; most amounts are shown amounts (what you have, owe,
//! spend or earn, positive).

use pecunia_model::reports::{self, month_start};
use pecunia_model::{NaiveDate, Txn};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Query "report.overview", "Overview", [], None,
            "date? (default today). Net worth, free to spend, this month and last month, the balances of the last year, and what needs attention.",
            always, overview),
        cmd!(Query "report.netWorth", "Net Worth", [], None, "date? (default today). What you have (with pots and goals) and owe.", always, net_worth),
        cmd!(Query "report.incomeExpenses", "Income and Spending", [], None,
            "from?, to? (dates; default this month). Income and spending per category and group, what is left over.",
            always, income_expenses),
        cmd!(Query "report.cashFlow", "Cash Flow", [], None, "from?, to? (default this month). In, out and balance per account.", always, cash_flow),
        cmd!(Query "report.monthEnds", "Balances Over Time", [], None, "from?, to? (default the last 12 months). Bank and cash at the end of each month.", always, month_ends),
        cmd!(Query "report.fixedCosts", "Fixed Costs", [], None, "date? (default today). Repeating costs, price changes, new and stopped ones.", always, fixed_costs),
        cmd!(Query "report.bankCheck", "Bank Check", [], None, "The books against the balances the bank stated in imported files.", always, bank_check),
        cmd!(Query "report.ownAccounts", "Own Accounts?", [], None, "IBANs money goes to and comes back from: maybe accounts of yours not in the book.", always, own_accounts),
    ]
}

fn txns(s: &Session) -> Vec<&Txn> {
    s.book().txns().map(|t| t.as_ref()).collect()
}

/// `from`/`to` params, else the view's period, else this month up to today.
fn period(s: &Session, cmd: &str, p: &Json) -> Result<(NaiveDate, NaiveDate)> {
    let today = s.today();
    let from = date_param(s, cmd, p, "from")?.or(s.view.from).unwrap_or_else(|| month_start(today));
    let to = date_param(s, cmd, p, "to")?.or(s.view.to).unwrap_or(today);
    if to < from {
        return Err(bad(cmd, "`to` is before `from`"));
    }
    Ok((from, to))
}

fn to_json<T: serde::Serialize>(v: &T) -> Json {
    serde_json::to_value(v).unwrap_or(Json::Null)
}

fn overview(s: &mut Session, p: &Json) -> Result<Json> {
    let today = date_param(s, "report.overview", p, "date")?.unwrap_or_else(|| s.today());
    let b = s.book();
    let tree = b.tree();
    let t = txns(s);
    let (lm_from, lm_to) = reports::last_full_month(today);
    let year_ago = today.checked_sub_months(chrono::Months::new(11)).map(month_start).unwrap_or(today);
    Ok(json!({
        "date": today,
        "netWorth": to_json(&reports::net_worth(&tree, &t, today)),
        "freeToSpend": to_json(&reports::free_to_spend(&tree, &t, today)),
        "thisMonth": to_json(&reports::income_and_expenses(&tree, &t, month_start(today), today)),
        "lastMonth": to_json(&reports::income_and_expenses(&tree, &t, lm_from, lm_to)),
        "monthEnds": to_json(&reports::bank_and_cash_over_time(&tree, &t, year_ago, today)),
        "attention": attention_json(&reports::needs_attention(b, today)),
    }))
}

/// Needs attention, each item that can be answered with its `key` (for `attention.dismiss`).
fn attention_json(a: &reports::NeedsAttention) -> Json {
    let mut j = to_json(a);
    let keys: [(&str, Vec<String>); 4] = [
        ("own_accounts", a.own_accounts.iter().map(|c| c.key()).collect()),
        ("price_changes", a.price_changes.iter().map(|c| c.key()).collect()),
        ("new_fixed_costs", a.new_fixed_costs.iter().map(|c| c.key()).collect()),
        ("stopped_fixed_costs", a.stopped_fixed_costs.iter().map(|c| c.key()).collect()),
    ];
    for (field, keys) in keys {
        if let Some(items) = j.get_mut(field).and_then(Json::as_array_mut) {
            for (item, key) in items.iter_mut().zip(keys) {
                if let Some(o) = item.as_object_mut() {
                    o.insert("key".into(), Json::String(key));
                }
            }
        }
    }
    j
}

fn net_worth(s: &mut Session, p: &Json) -> Result<Json> {
    let date = date_param(s, "report.netWorth", p, "date")?.unwrap_or_else(|| s.today());
    Ok(to_json(&reports::net_worth(&s.book().tree(), &txns(s), date)))
}

fn income_expenses(s: &mut Session, p: &Json) -> Result<Json> {
    let (from, to) = period(s, "report.incomeExpenses", p)?;
    Ok(to_json(&reports::income_and_expenses(&s.book().tree(), &txns(s), from, to)))
}

fn cash_flow(s: &mut Session, p: &Json) -> Result<Json> {
    let (from, to) = period(s, "report.cashFlow", p)?;
    Ok(to_json(&reports::cash_flow(&s.book().tree(), &txns(s), from, to)))
}

fn month_ends(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "report.monthEnds";
    let today = s.today();
    let from = date_param(s, C, p, "from")?.unwrap_or_else(|| today.checked_sub_months(chrono::Months::new(11)).map(month_start).unwrap_or(today));
    let to = date_param(s, C, p, "to")?.unwrap_or(today);
    Ok(json!({ "months": to_json(&reports::bank_and_cash_over_time(&s.book().tree(), &txns(s), from, to)) }))
}

fn fixed_costs(s: &mut Session, p: &Json) -> Result<Json> {
    let date = date_param(s, "report.fixedCosts", p, "date")?.unwrap_or_else(|| s.today());
    Ok(to_json(&reports::fixed_costs(&s.book().tree(), &txns(s), date)))
}

fn bank_check(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({ "accounts": to_json(&reports::bank_checks(&s.book().tree(), &txns(s), &s.book().meta.balances)) }))
}

fn own_accounts(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!({ "candidates": to_json(&reports::own_account_candidates(&s.book().tree(), &txns(s))) }))
}
