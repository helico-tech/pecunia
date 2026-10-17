//! "Is this one of your accounts?": counterparty IBANs that both receive money from you and send money back are likely
//! an own account the books do not know yet (savings, partner, joint).

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::money::Money;
use crate::tree::{AccountTree, normalize_iban};
use crate::types::{Txn, TxnId};

/// Each way must have happened at least this often.
const MIN_EACH_WAY: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnAccountCandidate {
    /// Without spaces, upper case.
    pub iban: String,
    /// The payee text seen most often for it.
    pub name: String,
    pub times_out: u32,
    pub times_in: u32,
    /// Both positive.
    pub total_out: Money,
    pub total_in: Money,
    pub last_date: NaiveDate,
    /// The transactions that would become transfers, lowest number first.
    pub txns: Vec<TxnId>,
}

impl OwnAccountCandidate {
    /// Its key in [`BookMeta::dismissed`](crate::book::BookMeta): "someone else's account".
    pub fn key(&self) -> String {
        format!("notmine:{}", self.iban)
    }
}

/// Counterparty IBANs of bank transactions that are not own accounts, with money going there and coming back at least
/// twice each; most transactions first. The caller leaves out IBANs the person already answered.
pub fn own_account_candidates(tree: &AccountTree, txns: &[&Txn]) -> Vec<OwnAccountCandidate> {
    let mut by_iban: BTreeMap<String, (OwnAccountCandidate, BTreeMap<&str, u32>)> = BTreeMap::new();
    for t in txns.iter().copied() {
        let (Some(iban), Some(source)) = (t.counterparty_iban.as_deref(), t.account) else { continue };
        let iban = normalize_iban(iban);
        if iban.is_empty() || tree.find_by_iban(&iban).is_some() {
            continue;
        }
        let amount =
            t.bank_amount.unwrap_or_else(|| t.postings.iter().filter(|p| tree.physical_of(p.account) == Some(source)).map(|p| p.amount).sum());
        if amount.is_zero() {
            continue;
        }
        let (c, names) = by_iban.entry(iban.clone()).or_insert_with(|| {
            let c = OwnAccountCandidate {
                iban,
                name: String::new(),
                times_out: 0,
                times_in: 0,
                total_out: Money::ZERO,
                total_in: Money::ZERO,
                last_date: t.date,
                txns: vec![],
            };
            (c, BTreeMap::new())
        });
        if amount.is_negative() {
            c.times_out = c.times_out.saturating_add(1);
            c.total_out -= amount;
        } else {
            c.times_in = c.times_in.saturating_add(1);
            c.total_in += amount;
        }
        c.last_date = c.last_date.max(t.date);
        c.txns.push(t.id);
        let n = names.entry(t.payee.as_str()).or_default();
        *n = n.saturating_add(1);
    }
    let mut out: Vec<OwnAccountCandidate> = by_iban
        .into_values()
        .filter(|(c, _)| c.times_out >= MIN_EACH_WAY && c.times_in >= MIN_EACH_WAY)
        .map(|(mut c, names)| {
            // The most common name; on a tie the first in order.
            c.name = names.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))).map(|n| n.0.to_string()).unwrap_or_default();
            c.txns.sort();
            c
        })
        .collect();
    let times = |c: &OwnAccountCandidate| c.times_out.saturating_add(c.times_in);
    out.sort_by(|a, b| times(b).cmp(&times(a)).then(a.iban.cmp(&b.iban)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn finds_ibans_money_goes_to_and_comes_back_from() {
        let with_iban = |mut t: Txn, iban: &str| -> Txn {
            t.counterparty_iban = Some(iban.into());
            t
        };
        let out = |id: u64, cents: i64| txn(id, d(2026, 3, id as u32), Some(BETAAL), "J. Bakker", &[(BETAAL_FREE, -cents), (GROCERIES, cents)]);
        let back = |id: u64, cents: i64| txn(id, d(2026, 4, id as u32), Some(BETAAL), "J. Bakker", &[(BETAAL_FREE, cents), (SALARY, -cents)]);
        let txns = vec![
            with_iban(out(1, 30_000), "NL55 INGB 0008 8101 23"),
            with_iban(out(2, 30_000), "NL55INGB0008810123"),
            with_iban(out(3, 30_000), "NL55INGB0008810123"),
            with_iban(back(4, 25_000), "NL55INGB0008810123"),
            with_iban(back(5, 20_000), "nl55ingb0008810123"),
            // Only one way: a shop.
            with_iban(out(6, 1_000), "NL19ABNA0213232323"),
            with_iban(out(7, 1_000), "NL19ABNA0213232323"),
            // An own account.
            with_iban(out(8, 1_000), "NL12INGB0001234567"),
            with_iban(back(9, 1_000), "NL12INGB0001234567"),
            with_iban(out(10, 1_000), "NL12INGB0001234567"),
            with_iban(back(11, 1_000), "NL12INGB0001234567"),
        ];
        let found = own_account_candidates(&tree(), &refs(&txns));
        assert_eq!(
            found,
            vec![OwnAccountCandidate {
                iban: "NL55INGB0008810123".into(),
                name: "J. Bakker".into(),
                times_out: 3,
                times_in: 2,
                total_out: Money(90_000),
                total_in: Money(45_000),
                last_date: d(2026, 4, 5),
                txns: (1..=5).map(TxnId).collect(),
            }]
        );
    }
}
