//! Reports as pure functions over the account tree and transactions (ADR 0003). Every result is a plain serializable
//! struct the engine returns as JSON. Amounts are shown amounts (what you have, owe, spend or earn, positive) unless a
//! field says otherwise. Dates (`today`, a range) are parameters: no clock. From budget.rs `reports/`.
//!
//! Transactions come in as `&[&Txn]`; from a [`crate::Book`]: `book.txns().map(|t| t.as_ref()).collect::<Vec<_>>()`.

mod attention;
mod bank_check;
mod cash_flow;
mod fixed_costs;
mod income;
mod net_worth;
mod own_accounts;

pub use attention::{NeedsAttention, ReviewCounts, needs_attention, review_counts};
pub use bank_check::{BankCheck, BankCheckStatus, bank_check, bank_checks};
pub use cash_flow::{AccountFlow, CashFlow, MonthEnd, bank_and_cash_over_time, cash_flow};
pub use fixed_costs::{FixedCost, FixedCosts, Frequency, NewFixedCost, PriceChange, StoppedCost, fixed_costs};
pub use income::{CategoryAmount, IncomeExpenses, income_and_expenses};
pub use net_worth::{FreeToSpend, GoalProgress, NetWorth, PhysicalBalance, PotBalance, free_to_spend, goal_progress, net_worth};
pub use own_accounts::{OwnAccountCandidate, own_account_candidates};

use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;

use crate::money::Money;
use crate::types::AccountId;

/// An account with an amount (a line in a report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountAmount {
    pub account: AccountId,
    pub name: String,
    pub amount: Money,
}

/// The first day of the month of `date`.
pub fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// The last day of the month of `date`.
pub fn month_end(date: NaiveDate) -> NaiveDate {
    month_start(date).checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(date)
}

/// The first and last day of a month; `None` for a month that does not exist.
pub fn month_range(year: i32, month: u32) -> Option<(NaiveDate, NaiveDate)> {
    let start = NaiveDate::from_ymd_opt(year, month, 1)?;
    Some((start, month_end(start)))
}

/// The first and last day of the month before `today`'s: the Overview's "September" in October.
pub fn last_full_month(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = month_start(today).checked_sub_months(Months::new(1)).unwrap_or(month_start(today));
    (first, month_end(first))
}

/// Months from the month of `from` to the month of `to` (12 from October to October next year).
pub fn months_between(from: NaiveDate, to: NaiveDate) -> i32 {
    (to.year() - from.year()) * 12 + to.month() as i32 - from.month() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::d;

    #[test]
    fn month_helpers() {
        assert_eq!(month_end(d(2026, 2, 10)), d(2026, 2, 28));
        assert_eq!(month_end(d(2026, 12, 31)), d(2026, 12, 31));
        assert_eq!(month_range(2026, 9), Some((d(2026, 9, 1), d(2026, 9, 30))));
        assert_eq!(month_range(2026, 13), None);
        assert_eq!(months_between(d(2026, 10, 6), d(2027, 6, 30)), 8);
        assert_eq!(last_full_month(d(2026, 10, 6)), (d(2026, 9, 1), d(2026, 9, 30)));
        assert_eq!(last_full_month(d(2026, 1, 31)), (d(2025, 12, 1), d(2025, 12, 31)));
        assert_eq!(month_end(NaiveDate::MAX), NaiveDate::MAX, "no panic at the end of time");
    }
}
