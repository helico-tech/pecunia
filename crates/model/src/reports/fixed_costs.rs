//! Fixed costs, found from payments that repeat: the same payee about once a month (direct debits mostly). Research
//! for budget.rs: half of the money going out, 23 of 55 creditors debit monthly.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use chrono::{Datelike, Days, Months, NaiveDate};
use serde::Serialize;

use crate::ledger::is_transfer;
use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::{AccountId, Txn};

/// Days between two payments that count as "a month later".
const MONTHLY: RangeInclusive<i64> = 25..=35;
/// A payment is late (stopped) this many days after its expected date.
const GRACE_DAYS: u64 = 7;
/// How close to "a year before" a payment must be to count as the same season.
const SEASON_DAYS: i64 = 10;
/// A price change is news for this many months.
const PRICE_CHANGE_MONTHS: u32 = 3;
/// A series this short that is all there is from a payee is a new fixed cost, not yet counted.
const NEW_BELOW: usize = 4;

/// Fixed costs at a date: what counts, plus what changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedCosts {
    pub date: NaiveDate,
    /// Biggest first.
    pub items: Vec<FixedCost>,
    pub total_per_month: Money,
    /// At today's prices.
    pub total_per_year: Money,
    pub price_changes: Vec<PriceChange>,
    /// Repeating for a short while only: ask the person.
    pub new: Vec<NewFixedCost>,
    /// Expected but not seen.
    pub stopped: Vec<StoppedCost>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Frequency {
    Monthly,
    /// Monthly for part of the year (local taxes in 8 terms).
    TimesAYear {
        times: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedCost {
    /// The grouping key.
    pub payee_norm: String,
    /// The payee as the bank wrote it, last time.
    pub name: String,
    pub frequency: Frequency,
    /// The last payment (all payments to the payee that day together).
    pub amount: Money,
    pub per_month: Money,
    pub per_year: Money,
    pub last_date: NaiveDate,
    pub next_date: NaiveDate,
    /// How many payments made up the last amount ("2 policies").
    pub parts: u32,
    /// Where the money went (categories, the mortgage), for the "what" line.
    pub counters: Vec<AccountId>,
    /// The physical accounts it was paid from.
    pub accounts: Vec<AccountId>,
}

/// A fixed cost that costs more (or less) since recently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceChange {
    pub payee_norm: String,
    pub name: String,
    pub old_amount: Money,
    pub new_amount: Money,
    pub since: NaiveDate,
    /// More (or, negative, less) a year at the new price.
    pub per_year_more: Money,
}

impl PriceChange {
    /// Its key in [`BookMeta::dismissed`](crate::book::BookMeta): this price from this date.
    pub fn key(&self) -> String {
        format!("price:{}:{}", self.since, self.payee_norm)
    }
}

impl NewFixedCost {
    pub fn key(&self) -> String {
        format!("new:{}", self.payee_norm)
    }
}

impl StoppedCost {
    /// This missed payment; a later one asks again.
    pub fn key(&self) -> String {
        format!("stopped:{}:{}", self.expected, self.payee_norm)
    }
}

/// A payment that repeated a few months in a row and never before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewFixedCost {
    pub payee_norm: String,
    pub name: String,
    pub amount: Money,
    pub day_of_month: u32,
    pub months_in_a_row: u32,
    pub last_date: NaiveDate,
    pub next_date: NaiveDate,
}

/// A fixed cost whose payment did not come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedCost {
    pub payee_norm: String,
    pub name: String,
    pub amount: Money,
    pub expected: NaiveDate,
    pub last_date: NaiveDate,
}

/// All payments to one payee on one day.
#[derive(Debug, Clone)]
struct Occurrence {
    date: NaiveDate,
    amount: Money,
    parts: u32,
    name: String,
    counters: BTreeSet<AccountId>,
    sources: BTreeSet<AccountId>,
}

impl Occurrence {
    fn new(date: NaiveDate) -> Occurrence {
        Occurrence { date, amount: Money::ZERO, parts: 0, name: String::new(), counters: BTreeSet::new(), sources: BTreeSet::new() }
    }
}

fn monthly(a: NaiveDate, b: NaiveDate) -> bool {
    MONTHLY.contains(&(b - a).num_days())
}

/// `date` plus `months`, or the last date there is.
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    date.checked_add_months(Months::new(months)).unwrap_or(NaiveDate::MAX)
}

/// `date` minus `months`, or the first date there is.
fn sub_months(date: NaiveDate, months: u32) -> NaiveDate {
    date.checked_sub_months(Months::new(months)).unwrap_or(NaiveDate::MIN)
}

/// Fixed costs as of `today`: per payee, payments out of own accounts (not transfers between them) on or before
/// `today`; a series of at least three payments about a month apart counts. Monthly when it covers the last year, else
/// "n× a year" with n the payments in the last year; a series that paused for the same season a year earlier is still
/// running (local taxes), one that is more than a week late is stopped.
pub fn fixed_costs(tree: &AccountTree, txns: &[&Txn], today: NaiveDate) -> FixedCosts {
    let mut by_payee: BTreeMap<&str, BTreeMap<NaiveDate, Occurrence>> = BTreeMap::new();
    for t in txns.iter().copied().filter(|t| t.date <= today && !t.payee_norm.is_empty()) {
        let Some(source) = t.account else { continue };
        if is_transfer(tree, t) {
            continue;
        }
        let (own, counter): (Vec<_>, Vec<_>) = t.postings.iter().partition(|p| tree.physical_of(p.account) == Some(source));
        let amount = t.bank_amount.unwrap_or_else(|| own.iter().map(|p| p.amount).sum());
        if !amount.is_negative() {
            continue;
        }
        let o = by_payee.entry(&t.payee_norm).or_default().entry(t.date).or_insert_with(|| Occurrence::new(t.date));
        o.amount -= amount;
        o.parts = o.parts.saturating_add(1);
        o.name.clone_from(&t.payee);
        o.counters.extend(counter.iter().map(|p| p.account));
        o.sources.insert(source);
    }

    let mut out = FixedCosts {
        date: today,
        items: vec![],
        total_per_month: Money::ZERO,
        total_per_year: Money::ZERO,
        price_changes: vec![],
        new: vec![],
        stopped: vec![],
    };
    let year_ago = sub_months(today, 12);
    for (payee, occ) in by_payee {
        let occ: Vec<Occurrence> = occ.into_values().collect();
        // The last run of payments about a month apart.
        let run_len = 1 + occ.windows(2).rev().take_while(|w| matches!(w, [a, b] if monthly(a.date, b.date))).count();
        let start = occ.len().saturating_sub(run_len);
        let run = occ.get(start..).unwrap_or_default();
        let (Some(first_ever), Some(first), Some(last)) = (occ.first(), run.first(), run.last()) else { continue };
        if run.len() < 3 {
            continue;
        }
        let mut next = add_months(last.date, 1);
        let mut paused = false;
        if next.checked_add_days(Days::new(GRACE_DAYS)).is_some_and(|late| today > late) {
            let season = sub_months(next, 12);
            let history_covers = season.checked_sub_days(Days::new(SEASON_DAYS as u64)).is_some_and(|s| first_ever.date < s);
            let paid_then = occ.iter().any(|o| (o.date - season).num_days().abs() <= SEASON_DAYS);
            if !history_covers || paid_then {
                out.stopped.push(StoppedCost {
                    payee_norm: payee.into(),
                    name: last.name.clone(),
                    amount: last.amount,
                    expected: next,
                    last_date: last.date,
                });
                continue;
            }
            paused = true;
            let first_this_year = occ.iter().find(|o| o.date > year_ago).map_or(next, |o| o.date);
            next = add_months(first_this_year, 12);
        }
        if start == 0 && run.len() < NEW_BELOW && !paused {
            out.new.push(NewFixedCost {
                payee_norm: payee.into(),
                name: last.name.clone(),
                amount: last.amount,
                day_of_month: last.date.day(),
                months_in_a_row: u32::try_from(run.len()).unwrap_or(u32::MAX),
                last_date: last.date,
                next_date: next,
            });
            continue;
        }
        let in_year: Vec<&Occurrence> = occ.iter().filter(|o| o.date > year_ago).collect();
        let covers_year = in_year.first().is_none_or(|o| o.date >= first.date);
        let times = i64::try_from(in_year.len()).unwrap_or(i64::MAX);
        let (frequency, per_month, per_year) = if (!paused && covers_year) || times >= 12 {
            (Frequency::Monthly, last.amount, last.amount.ratio(12, 1))
        } else {
            let k = times.max(1);
            (Frequency::TimesAYear { times: u32::try_from(k).unwrap_or(u32::MAX) }, last.amount.ratio(k, 12), last.amount.ratio(k, 1))
        };
        // The payment before the last price change, and the first one at the new price.
        let changed = run.iter().rposition(|o| o.amount != last.amount).and_then(|i| Some((run.get(i)?, run.get(i + 1)?)));
        if let Some((old, new)) = changed
            && new.date >= sub_months(today, PRICE_CHANGE_MONTHS)
        {
            let per_year_times = per_year.cents() / last.amount.cents().max(1);
            out.price_changes.push(PriceChange {
                payee_norm: payee.into(),
                name: last.name.clone(),
                old_amount: old.amount,
                new_amount: last.amount,
                since: new.date,
                per_year_more: (last.amount - old.amount).ratio(per_year_times, 1),
            });
        }
        out.items.push(FixedCost {
            payee_norm: payee.into(),
            name: last.name.clone(),
            frequency,
            amount: last.amount,
            per_month,
            per_year,
            last_date: last.date,
            next_date: next,
            parts: last.parts,
            counters: last.counters.iter().copied().collect(),
            accounts: last.sources.iter().copied().collect(),
        });
    }
    out.items.sort_by(|a, b| b.per_month.cmp(&a.per_month).then(a.name.cmp(&b.name)));
    out.total_per_month = out.items.iter().map(|i| i.per_month).sum();
    out.total_per_year = out.items.iter().map(|i| i.per_year).sum();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    /// Monthly payments to `payee` from Betaal to Groceries on `day`, for these (year, month, cents).
    fn series(id: &mut u64, payee: &str, day: u32, months: &[(i32, u32, i64)]) -> Vec<Txn> {
        months
            .iter()
            .map(|(y, m, c)| {
                *id += 1;
                txn(*id, d(*y, *m, day), Some(BETAAL), payee, &[(BETAAL_FREE, -c), (GROCERIES, *c)])
            })
            .collect()
    }

    fn year(from: (i32, u32), n: u32) -> Vec<(i32, u32)> {
        (0..n).map(|i| (from.0 + ((from.1 - 1 + i) / 12) as i32, (from.1 - 1 + i) % 12 + 1)).collect()
    }

    #[test]
    fn finds_monthly_and_part_of_the_year_costs() {
        let mut id = 0;
        let mut txns = vec![];
        let netflix: Vec<_> = year((2025, 10), 12).into_iter().map(|(y, m)| (y, m, if (y, m) >= (2026, 8) { 1399 } else { 1199 })).collect();
        txns.extend(series(&mut id, "NETFLIX", 17, &netflix));
        let taxes: Vec<_> = [(2025, 10)].into_iter().chain(year((2026, 3), 8)).map(|(y, m)| (y, m, 4800)).collect();
        txns.extend(series(&mut id, "GEMEENTE UTRECHT", 28, &taxes));
        // Two policies on the same day, from two accounts.
        for (y, m) in year((2025, 11), 12) {
            id += 2;
            txns.push(txn(id - 1, d(y, m, 4), Some(BETAAL), "ZILVEREN KRUIS", &[(BETAAL_FREE, -15620), (GROCERIES, 15620)]));
            txns.push(txn(id, d(y, m, 4), Some(CARD), "ZILVEREN KRUIS", &[(CARD, -15620), (GROCERIES, 15620)]));
        }
        // Weekly shopping and a monthly transfer are not fixed costs.
        for w in 0..30 {
            id += 1;
            txns.push(txn(id, d(2026, 3, 1) + Days::new(7 * w), Some(BETAAL), "AH", &[(BETAAL_FREE, -5000), (GROCERIES, 5000)]));
        }
        for (y, m) in year((2025, 10), 12) {
            id += 1;
            txns.push(txn(id, d(y, m, 2), Some(BETAAL), "SPAAR", &[(BETAAL_FREE, -20000), (SPAAR_FREE, 20000)]));
        }
        let txns = refs(&txns);

        let fc = fixed_costs(&tree(), &txns, d(2026, 10, 6));
        let names: Vec<_> = fc.items.iter().map(|i| i.payee_norm.as_str()).collect();
        assert_eq!(names, ["ZILVEREN KRUIS", "GEMEENTE UTRECHT", "NETFLIX"]);
        let zk = &fc.items[0];
        assert_eq!((zk.amount, zk.parts, zk.frequency, zk.next_date), (Money(31240), 2, Frequency::Monthly, d(2026, 11, 4)));
        assert_eq!(zk.accounts, vec![BETAAL, CARD]);
        assert_eq!(zk.counters, vec![GROCERIES]);
        let taxes = &fc.items[1];
        assert_eq!(taxes.frequency, Frequency::TimesAYear { times: 8 });
        assert_eq!((taxes.per_month, taxes.per_year, taxes.next_date), (Money(3200), Money(38400), d(2026, 10, 28)));
        assert_eq!(fc.items[2].per_month, Money(1399));
        assert_eq!(fc.total_per_month, Money(31240 + 3200 + 1399));
        assert_eq!(fc.total_per_year, Money(31240 * 12 + 38400 + 1399 * 12));
        assert_eq!(
            fc.price_changes,
            vec![PriceChange {
                payee_norm: "NETFLIX".into(),
                name: "NETFLIX".into(),
                old_amount: Money(1199),
                new_amount: Money(1399),
                since: d(2026, 8, 17),
                per_year_more: Money(2400),
            }]
        );
        assert!(fc.new.is_empty() && fc.stopped.is_empty());

        // In December the taxes pause, as they did a year earlier: still a fixed cost, next term in March.
        let dec = fixed_costs(&tree(), &txns, d(2026, 12, 10));
        let taxes = dec.items.iter().find(|i| i.payee_norm == "GEMEENTE UTRECHT").unwrap();
        assert_eq!((taxes.frequency, taxes.next_date), (Frequency::TimesAYear { times: 8 }, d(2027, 3, 28)));
        assert!(dec.price_changes.is_empty(), "news for three months only");
    }

    #[test]
    fn finds_new_and_stopped_ones() {
        let mut id = 0;
        let mut txns = series(&mut id, "BASIC FIT", 28, &[(2026, 7, 2999), (2026, 8, 2999), (2026, 9, 2999)]);
        let spotify: Vec<_> = year((2025, 10), 11).into_iter().map(|(y, m)| (y, m, 1099)).collect();
        txns.extend(series(&mut id, "SPOTIFY", 15, &spotify));
        let txns = refs(&txns);
        let fc = fixed_costs(&tree(), &txns, d(2026, 10, 6));
        assert!(fc.items.is_empty());
        assert_eq!(
            fc.new,
            vec![NewFixedCost {
                payee_norm: "BASIC FIT".into(),
                name: "BASIC FIT".into(),
                amount: Money(2999),
                day_of_month: 28,
                months_in_a_row: 3,
                last_date: d(2026, 9, 28),
                next_date: d(2026, 10, 28),
            }]
        );
        assert_eq!(
            fc.stopped,
            vec![StoppedCost {
                payee_norm: "SPOTIFY".into(),
                name: "SPOTIFY".into(),
                amount: Money(1099),
                expected: d(2026, 9, 15),
                last_date: d(2026, 8, 15),
            }]
        );
        assert!(fixed_costs(&tree(), &txns, d(2026, 9, 20)).stopped.is_empty(), "not late yet");
    }

    #[test]
    fn survives_the_ends_of_time() {
        let mut id = 0;
        let late = series(&mut id, "LATE", 1, &[(262_142, 10, 100), (262_142, 11, 100), (262_142, 12, 100)]);
        let early = series(&mut id, "EARLY", 1, &[(-262_143, 1, 100), (-262_143, 2, 100), (-262_143, 3, 100)]);
        let txns: Vec<Txn> = late.into_iter().chain(early).collect();
        let fc = fixed_costs(&tree(), &refs(&txns), NaiveDate::MAX);
        assert_eq!(fc.new.len() + fc.stopped.len() + fc.items.len(), 2);
    }
}
