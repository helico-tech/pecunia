//! Bank rows before dedup and classification, and the description of bank-file formats. From budget.rs `raw.rs` and
//! `files.rs`. The parsers live in `pecunia-formats`; CSV profiles a person maps are stored in the book.

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::types::SourceKind;

/// A transaction as a bank file delivers it. The amount is signed for the account it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawTransaction {
    pub date: NaiveDate,
    pub amount: Money,
    pub description: String,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    /// The source's own stable id, when it has one (CAMT.053, Rabobank's Volgnr).
    pub external_id: Option<String>,
    /// The row exactly as read, for the import hash.
    pub raw_line: String,
    /// The IBAN of the account this row belongs to, when the file says.
    pub own_iban: Option<String>,
    /// Card holder (ICS card files), information only.
    pub who: Option<String>,
}

impl RawTransaction {
    pub fn new(date: NaiveDate, amount: Money, description: impl Into<String>) -> Self {
        RawTransaction {
            date,
            amount,
            description: description.into(),
            counterparty_name: None,
            counterparty_iban: None,
            external_id: None,
            raw_line: String::new(),
            own_iban: None,
            who: None,
        }
    }
}

/// A balance the bank states in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementBalance {
    pub date: NaiveDate,
    pub amount: Money,
}

/// What a bank file holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedStatement {
    pub rows: Vec<RawTransaction>,
    /// The account's own IBAN(s) found in the file, to suggest the target account.
    pub account_ibans: Vec<String>,
    pub warnings: Vec<String>,
    pub closing_balances: Vec<StatementBalance>,
}

/// The file could not be read as the detected format; `line` (1-based) points at the problem when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementUnreadable {
    pub line: Option<usize>,
    pub reason: String,
}

impl fmt::Display for StatementUnreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.reason),
            None => f.write_str(&self.reason),
        }
    }
}

impl std::error::Error for StatementUnreadable {}

impl StatementUnreadable {
    pub fn new(reason: impl Into<String>) -> Self {
        StatementUnreadable { line: None, reason: reason.into() }
    }

    pub fn at(line: usize, reason: impl Into<String>) -> Self {
        StatementUnreadable { line: Some(line), reason: reason.into() }
    }
}

/// The kinds of bank file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FileFormat {
    Csv,
    Camt053,
    Mt940,
}

/// A recognised file format: CSV (with the name of the profile that fits), CAMT.053 or MT940.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedFormat {
    pub format: FileFormat,
    pub profile: Option<String>,
}

impl DetectedFormat {
    pub fn camt053() -> Self {
        DetectedFormat { format: FileFormat::Camt053, profile: None }
    }

    pub fn mt940() -> Self {
        DetectedFormat { format: FileFormat::Mt940, profile: None }
    }

    pub fn csv(profile: impl Into<String>) -> Self {
        DetectedFormat { format: FileFormat::Csv, profile: Some(profile.into()) }
    }

    pub fn source_kind(&self) -> SourceKind {
        match self.format {
            FileFormat::Csv => SourceKind::Csv,
            FileFormat::Camt053 => SourceKind::Camt053,
            FileFormat::Mt940 => SourceKind::Mt940,
        }
    }

    /// `CSV · ING (CSV)`, `CAMT.053`, `MT940`.
    pub fn label(&self) -> String {
        match (&self.format, &self.profile) {
            (FileFormat::Csv, Some(p)) => format!("CSV · {p}"),
            (FileFormat::Csv, None) => "CSV".into(),
            (FileFormat::Camt053, _) => "CAMT.053".into(),
            (FileFormat::Mt940, _) => "MT940".into(),
        }
    }
}

/// How the sign of an amount is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignMode {
    /// The amount carries its own sign: `-200,00`, `+15,00`.
    #[default]
    Signed,
    /// A separate column says which way money went; [`CsvProfile::debit_values`] mark outflows (`Af`, `D`, `Debet`).
    Indicator,
}

/// A bank's CSV layout, as data. Columns are header names when `has_header` is true, else zero-based positions
/// written `#3`. Built-in profiles cover the Dutch banks; a person's mapping is saved in the book as a new profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvProfile {
    pub name: String,
    pub bank: String,
    #[serde(default = "default_delimiter")]
    pub delimiter: char,
    #[serde(default = "default_true")]
    pub has_header: bool,
    /// Lines before the header (Knab writes "KNAB EXPORT" first).
    #[serde(default)]
    pub skip_lines: usize,
    pub date_column: String,
    /// `yyyyMMdd`, `yyyy-MM-dd` or `dd-MM-yyyy`.
    pub date_format: String,
    pub amount_column: String,
    #[serde(default = "default_true")]
    pub decimal_comma: bool,
    #[serde(default)]
    pub sign: SignMode,
    #[serde(default)]
    pub indicator_column: Option<String>,
    #[serde(default)]
    pub debit_values: Vec<String>,
    #[serde(default)]
    pub name_column: Option<String>,
    #[serde(default)]
    pub counter_iban_column: Option<String>,
    #[serde(default)]
    pub description_columns: Vec<String>,
    #[serde(default)]
    pub own_iban_column: Option<String>,
    /// A stable per-row id from the bank (Rabobank's Volgnr), used for exact dedup.
    #[serde(default)]
    pub external_id_column: Option<String>,
    /// Card holder or person (ICS card files).
    #[serde(default)]
    pub who_column: Option<String>,
    /// A WHATWG encoding label (`UTF-8`, `windows-1252`).
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

impl CsvProfile {
    /// A profile with the defaults: comma-separated, a header row, decimal comma, signed amounts, UTF-8.
    pub fn new(
        name: impl Into<String>,
        bank: impl Into<String>,
        date_column: impl Into<String>,
        date_format: impl Into<String>,
        amount_column: impl Into<String>,
    ) -> Self {
        CsvProfile {
            name: name.into(),
            bank: bank.into(),
            delimiter: default_delimiter(),
            has_header: true,
            skip_lines: 0,
            date_column: date_column.into(),
            date_format: date_format.into(),
            amount_column: amount_column.into(),
            decimal_comma: true,
            sign: SignMode::Signed,
            indicator_column: None,
            debit_values: Vec::new(),
            name_column: None,
            counter_iban_column: None,
            description_columns: Vec::new(),
            own_iban_column: None,
            external_id_column: None,
            who_column: None,
            encoding: default_encoding(),
        }
    }
}

fn default_delimiter() -> char {
    ','
}

fn default_true() -> bool {
    true
}

fn default_encoding() -> String {
    "UTF-8".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_names_the_line() {
        assert_eq!(StatementUnreadable::at(3, "bad date").to_string(), "line 3: bad date");
        assert_eq!(StatementUnreadable::new("the file is empty").to_string(), "the file is empty");
    }

    #[test]
    fn profiles_read_with_defaults() {
        let minimal = r#"{"name":"X","bank":"Y","date_column":"d","date_format":"yyyyMMdd","amount_column":"a"}"#;
        let profile: CsvProfile = serde_json::from_str(minimal).unwrap();
        assert_eq!(profile, CsvProfile::new("X", "Y", "d", "yyyyMMdd", "a"));
        assert_eq!(DetectedFormat::csv("ING (CSV)").label(), "CSV · ING (CSV)");
    }
}
