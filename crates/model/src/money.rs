//! Euros as whole cents. From budget.rs `money.rs`, with saturating arithmetic: amounts are capped on
//! input ([`Money::MAX_INPUT`]), so sums never come near the limits, and nothing panics if they did.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// An amount of euros as whole cents; outflows are negative. On disk it is a plain number of cents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Money = Money(0);
    /// The largest amount accepted from people, files and commands: one hundred billion euros.
    pub const MAX_INPUT: i64 = 10_000_000_000_000;

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn euros(euros: i64, cents: i64) -> Money {
        Money(euros.saturating_mul(100).saturating_add(if euros < 0 { -cents } else { cents }))
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn abs(self) -> Money {
        Money(self.0.saturating_abs())
    }

    /// Whether the amount is within what people and files may enter.
    pub fn is_sane(self) -> bool {
        self.0.unsigned_abs() <= Self::MAX_INPUT as u64
    }

    /// `self × n / d`, rounded half away from zero; 0 when `d` is 0.
    pub fn ratio(self, n: i64, d: i64) -> Money {
        if d == 0 {
            return Money::ZERO;
        }
        let x = self.0 as i128 * n as i128;
        let d = d as i128;
        let q = (x.abs() + d.abs() / 2) / d.abs();
        let q = if (x < 0) != (d < 0) { -q } else { q };
        Money(q.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
    }

    /// Dutch notation: `€ 1.234,56`, `−€ 23,40` (true minus sign).
    pub fn format(self) -> String {
        let a = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "−" } else { "" };
        format!("{sign}€ {},{:02}", group_thousands(a / 100), a % 100)
    }

    /// Like [`Money::format`] without the currency sign: `1.234,56`, `−23,40`.
    pub fn format_plain(self) -> String {
        let a = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "−" } else { "" };
        format!("{sign}{},{:02}", group_thousands(a / 100), a % 100)
    }

    /// Whole euros only, for charts and tiles: `€ 1.235`.
    pub fn format_euros(self) -> String {
        let rounded = self.ratio(1, 100).0;
        let sign = if rounded < 0 { "−" } else { "" };
        format!("{sign}€ {}", group_thousands(rounded.unsigned_abs()))
    }

    /// Plain decimal with a dot, no separators: `-1234.56` (JSON and CSV for programs).
    pub fn to_decimal(self) -> String {
        let a = self.0.unsigned_abs();
        format!("{}{}.{:02}", if self.0 < 0 { "-" } else { "" }, a / 100, a % 100)
    }

    /// Parses bank and people notation: `1.234,56`, `-1234,56`, `1234.56`, `+15,00`, `€ 3,50`, `(12,00)`. The last
    /// `.` or `,` followed by one or two digits is the decimal mark; other separators are thousands marks. Amounts
    /// beyond [`Money::MAX_INPUT`] are refused.
    pub fn parse(text: &str) -> Result<Money, String> {
        let shown: String = text.chars().take(40).collect();
        let mut s: String = text
            .trim()
            .replace('€', "")
            .replace("EUR", "")
            .chars()
            .filter(|c| *c != ' ' && *c != '\u{a0}' && *c != '\'')
            .map(|c| if c == '−' { '-' } else { c })
            .collect();
        if s.is_empty() {
            return Err("empty amount".into());
        }
        if s.len() > 40 {
            return Err(format!("not an amount: '{shown}'"));
        }
        let negative = s.starts_with('-') || (s.starts_with('(') && s.ends_with(')'));
        s = s.trim_start_matches(['+', '-', '(']).trim_end_matches(')').to_string();
        let (int_part, frac_part) = match s.rfind([',', '.']) {
            Some(i) if (1..=2).contains(&(s.len() - i - 1)) => (s.get(..i).unwrap_or(""), s.get(i + 1..).unwrap_or("")),
            _ => (s.as_str(), ""),
        };
        let digits: String = int_part.chars().filter(|c| *c != '.' && *c != ',').collect();
        let ok = digits.chars().all(|c| c.is_ascii_digit())
            && frac_part.chars().all(|c| c.is_ascii_digit())
            && (!digits.is_empty() || !frac_part.is_empty());
        if !ok {
            return Err(format!("not an amount: '{shown}'"));
        }
        let whole: i64 = if digits.is_empty() { 0 } else { digits.parse().map_err(|_| format!("too large: '{shown}'"))? };
        let frac: i64 = format!("{frac_part:0<2}").parse().map_err(|_| format!("not an amount: '{shown}'"))?;
        let cents = whole.checked_mul(100).and_then(|c| c.checked_add(frac)).ok_or(format!("too large: '{shown}'"))?;
        let m = Money(if negative { -cents } else { cents });
        if !m.is_sane() {
            return Err(format!("too large: '{shown}'"));
        }
        Ok(m)
    }
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0.saturating_add(rhs.0))
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0.saturating_sub(rhs.0))
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Money {
        Money(self.0.saturating_neg())
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        *self = *self + rhs;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Money) {
        *self = *self - rhs;
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, Add::add)
    }
}

impl<'a> Sum<&'a Money> for Money {
    fn sum<I: Iterator<Item = &'a Money>>(iter: I) -> Money {
        iter.copied().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dutch() {
        assert_eq!(Money(123456).format(), "€ 1.234,56");
        assert_eq!(Money(-2340).format(), "−€ 23,40");
        assert_eq!(Money(5).format(), "€ 0,05");
        assert_eq!(Money(21584347).format(), "€ 215.843,47");
        assert_eq!(Money(-123456).to_decimal(), "-1234.56");
        assert_eq!(Money(123456).format_euros(), "€ 1.235");
        assert_eq!(Money(-2340).format_plain(), "−23,40");
    }

    #[test]
    fn parses_bank_notation() {
        for (text, cents) in [
            ("1.234,56", 123456),
            ("-1234,56", -123456),
            ("1234.56", 123456),
            ("+15,00", 1500),
            ("€ 3,50", 350),
            ("(12,00)", -1200),
            ("1,234.5", 123450),
            ("12", 1200),
            ("−7,1", -710),
        ] {
            assert_eq!(Money::parse(text), Ok(Money(cents)), "{text}");
        }
        assert!(Money::parse("").is_err());
        assert!(Money::parse("abc").is_err());
        assert!(Money::parse("99999999999999999999").is_err());
        assert!(Money::parse("200000000000").is_err(), "beyond the input cap");
    }

    #[test]
    fn never_overflows() {
        assert_eq!(Money(i64::MAX) + Money(1), Money(i64::MAX));
        assert_eq!(-Money(i64::MIN), Money(i64::MAX));
        assert_eq!(Money(i64::MIN).abs(), Money(i64::MAX));
        assert_eq!(Money(1000).ratio(1, 3), Money(333));
        assert_eq!(Money(-1000).ratio(2, 3), Money(-667));
        assert_eq!(Money(5).ratio(1, 0), Money::ZERO);
    }
}
