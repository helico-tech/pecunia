//! The register: transactions as rows, filtered and in date order, with the money shown as an arrow (*ING → Groceries*)
//! and, for one account or pot, a running balance. Used by the app's list, the CLI and MCP.

use std::sync::Arc;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::book::Book;
use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::*;

/// Which transactions to show. Empty = all.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Filter {
    /// Words that must all appear in the payee, description, note, person, account names or amount.
    pub text: String,
    /// An account, pot, category or category group: transactions with money on it or below it.
    pub account: Option<AccountId>,
    /// Only transactions waiting in Review.
    pub review: bool,
    pub status: Option<TxnStatus>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub import: Option<ImportId>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }
}

/// One line of the register.
#[derive(Clone, Debug)]
pub struct Row {
    pub txn: Arc<Txn>,
    /// Where the money came from and went to, for people.
    pub from: String,
    pub to: String,
    /// The amount for the filtered account (money in positive; for a category, spent or earned positive), or for the
    /// household when no account is chosen (money out negative).
    pub amount: Money,
    /// The balance of the filtered account or pot after this row.
    pub balance: Option<Money>,
}

/// A short label for a leaf: an Unassigned pot is its account, another pot is `Account · Pot`.
pub fn leaf_label(tree: &AccountTree, id: AccountId) -> String {
    match tree.get(id) {
        Some(a) if a.unassigned => a.parent.map(|p| tree.name(p)).unwrap_or_else(|| a.name.clone()),
        Some(a) if a.kind == AccountKind::Pot => format!("{} · {}", a.parent.map(|p| tree.name(p)).unwrap_or_default(), a.name),
        Some(a) => a.name.clone(),
        None => format!("#{id}"),
    }
}

/// `(from, to)` for a transaction: the accounts money left and the accounts it went to.
pub fn arrow(tree: &AccountTree, t: &Txn) -> (String, String) {
    let side = |neg: bool| {
        let names: Vec<String> = t.postings.iter().filter(|p| p.amount.is_negative() == neg).map(|p| leaf_label(tree, p.account)).collect();
        match names.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [first, rest @ ..] => format!("{first} + {} more", rest.len()),
        }
    };
    (side(true), side(false))
}

/// The amount of a transaction for the household: what left or entered own accounts (Have and Owe), or the size of a
/// move between own accounts.
pub fn household_amount(tree: &AccountTree, t: &Txn) -> Money {
    let own: Money =
        t.postings.iter().filter(|p| tree.get(p.account).is_some_and(|a| matches!(a.group, Group::Have | Group::Owe))).map(|p| p.amount).sum();
    if own.is_zero() { t.size() } else { own }
}

/// Whether `t` passes the filter. `leaves` are the leaves under the filter's account.
fn matches(book: &Book, tree: &AccountTree, f: &Filter, leaves: &[AccountId], words: &[String], t: &Txn) -> bool {
    if f.review && book.review_kind(t).is_none() {
        return false;
    }
    if f.status.is_some_and(|s| s != t.status) || f.import.is_some_and(|i| t.import != Some(i)) {
        return false;
    }
    if f.from.is_some_and(|d| t.date < d) || f.to.is_some_and(|d| t.date > d) {
        return false;
    }
    if f.account.is_some() && !t.postings.iter().any(|p| leaves.contains(&p.account)) {
        return false;
    }
    if words.is_empty() {
        return true;
    }
    let mut hay = format!("{} {} {} {} {}", t.payee, t.description, t.note, t.who.as_deref().unwrap_or(""), t.size().format_plain());
    for p in &t.postings {
        hay.push(' ');
        hay.push_str(&leaf_label(tree, p.account));
    }
    let hay = hay.to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// The filtered transactions, newest first.
pub fn rows(book: &Book, tree: &AccountTree, f: &Filter) -> Vec<Row> {
    let leaves = f.account.map(|a| tree.leaves(a)).unwrap_or_default();
    let words: Vec<String> = f.text.to_lowercase().split_whitespace().take(10).map(str::to_string).collect();
    let account = f.account.and_then(|a| tree.get(a));
    let category_sign = account.filter(|a| matches!(a.group, Group::Goes | Group::Comes | Group::Equity)).map(|a| a.group.display_sign());
    let running = account.is_some_and(|a| a.kind.is_physical() || a.kind == AccountKind::Pot);
    let mut txns: Vec<&Arc<Txn>> = book.txns().filter(|t| matches(book, tree, f, &leaves, &words, t)).collect();
    txns.sort_by_key(|t| (t.date, t.id));
    // The running balance starts from everything before the first shown row that the date filter left out.
    let mut balance = if running {
        let start = f.from.unwrap_or(NaiveDate::MIN);
        book.txns().filter(|t| t.date < start).flat_map(|t| &t.postings).filter(|p| leaves.contains(&p.account)).map(|p| p.amount).sum()
    } else {
        Money::ZERO
    };
    let complete = words.is_empty() && !f.review && f.status.is_none() && f.import.is_none() && f.to.is_none();
    let mut out: Vec<Row> = txns
        .into_iter()
        .map(|t| {
            let (from, to) = arrow(tree, t);
            let here: Money = t.postings.iter().filter(|p| leaves.contains(&p.account)).map(|p| p.amount).sum();
            let amount = match (f.account, category_sign) {
                (None, _) => household_amount(tree, t),
                (Some(_), Some(sign)) => Money(here.cents().saturating_mul(sign)),
                (Some(_), None) => here,
            };
            balance += here;
            let shown = account.map(|a| Money(balance.cents().saturating_mul(if a.group == Group::Owe { -1 } else { 1 })));
            Row { txn: t.clone(), from, to, amount, balance: if running && complete { shown } else { None } }
        })
        .collect();
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn rows_with_arrows_amounts_and_running_balances() {
        let b = book(vec![
            txn(1, d(2026, 1, 1), None, "Start", &[(BETAAL_FREE, 100_000), (STARTING, -100_000)]),
            txn(2, d(2026, 1, 5), Some(BETAAL), "Albert Heijn", &[(BETAAL_FREE, -6418), (GROCERIES, 6418)]),
            txn(3, d(2026, 1, 25), Some(BETAAL), "Werkgever", &[(BETAAL_FREE, 250_000), (SALARY, -250_000)]),
            txn(4, d(2026, 1, 26), None, "Move", &[(BETAAL_FREE, -50_000), (BILLS, 50_000)]),
        ]);
        let t = b.tree();
        let all = rows(&b, &t, &Filter::default());
        assert_eq!(all.iter().map(|r| r.txn.id.0).collect::<Vec<_>>(), vec![4, 3, 2, 1]);
        assert_eq!((all[2].from.as_str(), all[2].to.as_str(), all[2].amount), ("ING Betaalrekening", "Groceries", Money(-6418)));
        assert_eq!((all[1].from.as_str(), all[1].to.as_str()), ("Salary", "ING Betaalrekening"));
        assert_eq!((all[0].to.as_str(), all[0].amount), ("ING Betaalrekening · Bills buffer", Money(50_000)), "a pot move shows its size");

        let ing = rows(&b, &t, &Filter { account: Some(BETAAL), ..Filter::default() });
        assert_eq!(ing.first().and_then(|r| r.balance), Some(Money(343_582)));
        assert_eq!(ing.last().and_then(|r| r.balance), Some(Money(100_000)));
        let groceries = rows(&b, &t, &Filter { account: Some(DAILY), ..Filter::default() });
        assert_eq!(groceries.iter().map(|r| r.amount).collect::<Vec<_>>(), vec![Money(6418)], "spent shows positive");
        let salary = rows(&b, &t, &Filter { account: Some(SALARY), ..Filter::default() });
        assert_eq!(salary.first().map(|r| r.amount), Some(Money(250_000)), "earned shows positive");

        assert_eq!(rows(&b, &t, &Filter { text: "albert 64,18".into(), ..Filter::default() }).len(), 1);
        assert_eq!(rows(&b, &t, &Filter { text: "groceries".into(), ..Filter::default() }).len(), 1);
        let jan = Filter { from: Some(d(2026, 1, 2)), to: Some(d(2026, 1, 25)), ..Filter::default() };
        assert_eq!(rows(&b, &t, &jan).len(), 2);
        let from = rows(&b, &t, &Filter { account: Some(BETAAL), from: Some(d(2026, 1, 2)), ..Filter::default() });
        assert_eq!(from.last().and_then(|r| r.balance), Some(Money(93_582)), "starts from the balance before");
    }
}
