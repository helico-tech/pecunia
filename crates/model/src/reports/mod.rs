//! Reports as pure functions over the account tree and transactions (ADR 0003). Amounts are shown amounts (what you
//! have, owe, spend or earn, positive) unless a field says otherwise. From budget.rs `reports/`.

use chrono::{Datelike, Months, NaiveDate};

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
    }
}
