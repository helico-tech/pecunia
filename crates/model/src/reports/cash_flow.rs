//! Cash flow per physical account, and bank and cash over time.

use chrono::NaiveDate;
use serde::Serialize;

use crate::ledger::Balances;
use crate::money::Money;
use crate::reports::{AccountAmount, month_end};
use crate::tree::AccountTree;
use crate::types::{AccountId, AccountKind, Group, Txn};

/// The most month ends [`bank_and_cash_over_time`] returns (a hundred years), whatever range it is asked for.
const MAX_MONTHS: usize = 1_200;

/// What came in and went out per physical account from `from` to `to` (both included).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CashFlow {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub accounts: Vec<AccountFlow>,
}

/// One physical account's flow. `money_in` and `money_out` are money into and out of the account, both positive
/// (paying off a card is money into it); pot moves stay inside and do not count. Opening and closing are shown
/// balances (what you have, or owe).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountFlow {
    pub account: AccountId,
    pub name: String,
    pub group: Group,
    pub opening: Money,
    pub money_in: Money,
    pub money_out: Money,
    pub closing: Money,
}

pub fn cash_flow(tree: &AccountTree, txns: &[&Txn], from: NaiveDate, to: NaiveDate) -> CashFlow {
    let opening = from.pred_opt().map(|day_before| Balances::at(txns.iter().copied(), day_before)).unwrap_or_default();
    let closing = Balances::at(txns.iter().copied(), to);
    let in_range: Vec<&Txn> = txns.iter().copied().filter(|t| t.date >= from && t.date <= to).collect();
    let accounts = tree
        .all_physical()
        .into_iter()
        .map(|a| {
            let (mut money_in, mut money_out) = (Money::ZERO, Money::ZERO);
            for t in &in_range {
                let here: Money = t.postings.iter().filter(|p| tree.physical_of(p.account) == Some(a.id)).map(|p| p.amount).sum();
                if here.is_positive() { money_in += here } else { money_out -= here }
            }
            AccountFlow {
                account: a.id,
                name: a.name.clone(),
                group: a.group,
                opening: opening.shown(tree, a.id),
                money_in,
                money_out,
                closing: closing.shown(tree, a.id),
            }
        })
        .collect();
    CashFlow { from, to, accounts }
}

/// The balance of bank and cash (current, savings and cash accounts) at one month end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonthEnd {
    /// The last day of the month, or `to` for the running month.
    pub date: NaiveDate,
    pub total: Money,
    pub accounts: Vec<AccountAmount>,
}

/// End-of-month balances of bank and cash from the month of `from` up to `to` (the last point is `to` itself when the
/// month is still running), for the "Bank and cash over time" chart. At most [`MAX_MONTHS`] points.
pub fn bank_and_cash_over_time(tree: &AccountTree, txns: &[&Txn], from: NaiveDate, to: NaiveDate) -> Vec<MonthEnd> {
    let accounts: Vec<_> = tree
        .physical_accounts(Group::Have)
        .into_iter()
        .filter(|a| matches!(a.kind, AccountKind::Current | AccountKind::Savings | AccountKind::Cash))
        .collect();
    let mut out = Vec::new();
    let mut at = month_end(from);
    while out.len() < MAX_MONTHS {
        let date = at.min(to);
        let b = Balances::at(txns.iter().copied(), date);
        let lines: Vec<AccountAmount> =
            accounts.iter().map(|a| AccountAmount { account: a.id, name: a.name.clone(), amount: b.shown(tree, a.id) }).collect();
        out.push(MonthEnd { date, total: lines.iter().map(|l| l.amount).sum(), accounts: lines });
        match at.succ_opt() {
            Some(next) if date < to => at = month_end(next),
            _ => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn books() -> Vec<Txn> {
        vec![
            txn(1, d(2026, 8, 1), None, "", &[(BETAAL_FREE, 100_000), (STARTING, -100_000)]),
            txn(2, d(2026, 9, 1), Some(BETAAL), "Werkgever", &[(BETAAL_FREE, 300_000), (SALARY, -300_000)]),
            txn(3, d(2026, 9, 2), Some(BETAAL), "Naar Spaar", &[(BETAAL_FREE, -20_000), (SPAAR_FREE, 20_000)]),
            txn(4, d(2026, 9, 3), None, "", &[(BETAAL_FREE, -5_000), (BILLS, 5_000)]),
            txn(5, d(2026, 9, 5), Some(CARD), "AH", &[(CARD, -6_418), (GROCERIES, 6_418)]),
            txn(6, d(2026, 10, 5), Some(BETAAL), "AH", &[(BETAAL_FREE, -1_000), (GROCERIES, 1_000)]),
        ]
    }

    #[test]
    fn flows_per_physical_account() {
        let txns = books();
        let cf = cash_flow(&tree(), &refs(&txns), d(2026, 9, 1), d(2026, 9, 30));
        let betaal = &cf.accounts[0];
        assert_eq!(betaal.name, "ING Betaalrekening");
        assert_eq!(
            (betaal.opening, betaal.money_in, betaal.money_out, betaal.closing),
            (Money(100_000), Money(300_000), Money(20_000), Money(380_000))
        );
        let card = cf.accounts.iter().find(|a| a.account == CARD).unwrap();
        assert_eq!((card.money_out, card.closing), (Money(6_418), Money(6_418)));
        assert!(cash_flow(&tree(), &refs(&txns), NaiveDate::MIN, NaiveDate::MIN).accounts.iter().all(|a| a.opening.is_zero()));
    }

    #[test]
    fn month_ends_of_bank_and_cash() {
        let txns = books();
        let points = bank_and_cash_over_time(&tree(), &refs(&txns), d(2026, 8, 15), d(2026, 10, 6));
        let dates: Vec<_> = points.iter().map(|p| p.date).collect();
        assert_eq!(dates, [d(2026, 8, 31), d(2026, 9, 30), d(2026, 10, 6)]);
        assert_eq!(points.iter().map(|p| p.total).collect::<Vec<_>>(), [Money(100_000), Money(400_000), Money(399_000)]);
        assert_eq!(bank_and_cash_over_time(&tree(), &[], d(2026, 10, 1), d(2026, 9, 1)).len(), 1, "a range backwards is one point");
        assert_eq!(bank_and_cash_over_time(&tree(), &[], NaiveDate::MIN, NaiveDate::MAX).len(), MAX_MONTHS);
    }
}
