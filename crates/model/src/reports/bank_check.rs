//! Bank check (reconciliation): the books' balance of a physical account against the balances the bank stated in
//! imported files (MT940 `:62F:`, CAMT `CLBD`), kept in [`crate::BookMeta::balances`].

use chrono::NaiveDate;
use serde::Serialize;

use crate::ledger::Balances;
use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::{AccountId, StatedBalance, Txn};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BankCheck {
    pub account: AccountId,
    pub name: String,
    #[serde(flatten)]
    pub status: BankCheckStatus,
}

/// Stated balances are signed like bank rows (a card debt is negative); so is `by`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BankCheckStatus {
    /// The bank never stated a balance.
    NoStatement,
    /// The books match the latest stated balance.
    InSync { date: NaiveDate, balance: Money },
    /// The books differ from the latest stated balance by `by` (books − bank), since the first stated balance after
    /// the last one that matched.
    Differs { by: Money, since: NaiveDate, date: NaiveDate, stated: Money, books: Money },
}

/// Checks one physical account against the stated balances (any order; those of other accounts are skipped).
pub fn bank_check(tree: &AccountTree, txns: &[&Txn], account: AccountId, stated: &[StatedBalance]) -> BankCheck {
    let mut stated: Vec<&StatedBalance> = stated.iter().filter(|s| s.account == account).collect();
    stated.sort_by_key(|s| s.date);
    let books = |date| Balances::at(txns.iter().copied(), date).total(tree, account);
    let status = match stated.last() {
        None => BankCheckStatus::NoStatement,
        Some(last) if books(last.date) == last.amount => BankCheckStatus::InSync { date: last.date, balance: last.amount },
        Some(last) => {
            let first_off = stated.iter().rposition(|s| books(s.date) == s.amount).map_or(0, |i| i + 1);
            let at_last = books(last.date);
            BankCheckStatus::Differs {
                by: at_last - last.amount,
                since: stated.get(first_off).map_or(last.date, |s| s.date),
                date: last.date,
                stated: last.amount,
                books: at_last,
            }
        }
    };
    BankCheck { account, name: tree.name(account), status }
}

/// Bank checks of every physical account the bank stated a balance for, in display order.
pub fn bank_checks(tree: &AccountTree, txns: &[&Txn], stated: &[StatedBalance]) -> Vec<BankCheck> {
    tree.all_physical().into_iter().filter(|a| stated.iter().any(|s| s.account == a.id)).map(|a| bank_check(tree, txns, a.id, stated)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn sb(account: AccountId, date: NaiveDate, cents: i64) -> StatedBalance {
        StatedBalance { account, date, amount: Money(cents), source: "test.sta".into() }
    }

    #[test]
    fn in_sync_or_differs_since_a_date() {
        let t = tree();
        let txns = [
            txn(1, d(2026, 9, 1), None, "", &[(BETAAL_FREE, 100_000), (STARTING, -100_000)]),
            txn(2, d(2026, 9, 21), Some(BETAAL), "AH", &[(BETAAL_FREE, -1_271), (GROCERIES, 1_271)]),
            txn(3, d(2026, 9, 21), Some(CARD), "Shop", &[(CARD, -43_215), (GROCERIES, 43_215)]),
        ];
        let txns = refs(&txns);
        let ok = bank_check(&t, &txns, BETAAL, &[sb(BETAAL, d(2026, 9, 30), 98_729), sb(BETAAL, d(2026, 9, 15), 100_000)]);
        assert_eq!(ok.status, BankCheckStatus::InSync { date: d(2026, 9, 30), balance: Money(98_729) });

        // The bank stated a balance before the books had the row, then caught up: in sync again.
        let caught_up = [sb(BETAAL, d(2026, 9, 15), 100_000), sb(BETAAL, d(2026, 9, 20), 98_729), sb(BETAAL, d(2026, 9, 30), 98_729)];
        assert!(matches!(bank_check(&t, &txns, BETAAL, &caught_up).status, BankCheckStatus::InSync { .. }));
        let off = [sb(BETAAL, d(2026, 9, 15), 100_000), sb(BETAAL, d(2026, 9, 20), 98_729), sb(BETAAL, d(2026, 9, 30), 97_458)];
        match bank_check(&t, &txns, BETAAL, &off).status {
            BankCheckStatus::Differs { by, since, .. } => assert_eq!((by, since), (Money(1_271), d(2026, 9, 20))),
            s => panic!("{s:?}"),
        }
        let card = bank_check(&t, &txns, CARD, &[sb(CARD, d(2026, 9, 30), -43_215)]);
        assert!(matches!(card.status, BankCheckStatus::InSync { .. }), "a card debt is stated negative");
        assert_eq!(bank_check(&t, &txns, BETAAL, &[]).status, BankCheckStatus::NoStatement);
        assert_eq!(bank_check(&t, &txns, BETAAL, &[sb(CARD, d(2026, 9, 30), 1)]).status, BankCheckStatus::NoStatement, "another account's");

        let all = bank_checks(&t, &txns, &[sb(CARD, d(2026, 9, 30), -43_215), sb(BETAAL, d(2026, 9, 30), 98_000)]);
        assert_eq!(all.iter().map(|c| c.account).collect::<Vec<_>>(), [BETAAL, CARD], "only accounts with a stated balance");
        assert!(matches!(all[0].status, BankCheckStatus::Differs { by: Money(729), .. }));
    }

    #[test]
    fn serializes_flat_with_a_status_tag() {
        let c = BankCheck { account: BETAAL, name: "ING".into(), status: BankCheckStatus::InSync { date: d(2026, 9, 30), balance: Money(5) } };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json, serde_json::json!({"account": 1, "name": "ING", "status": "in_sync", "date": "2026-09-30", "balance": 5}));
    }
}
