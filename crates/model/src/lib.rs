#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! The Pecunia bookkeeping model (L0).
//!
//! A [`Book`] holds a household's accounts (a tree: money you have with its pots, money you owe, where money goes and
//! comes from), its transactions (postings that sum to zero) and its payee rules. All edits go through validated
//! methods on [`Book`]; every input is treated as hostile (length and amount caps, no panics). Dates and times are
//! passed in by the caller, so the model has no clock and does no I/O. The bookkeeping rules come from budget.rs
//! (ADR 0001, 0003); the shape (validated edits, `from_parts` repair, `diff` into changes) from OpenIssue.

mod book;
pub mod catalogue;
pub mod classify;
pub mod dedup;
mod edit;
mod ingest;
pub mod ledger;
mod money;
pub mod payees;
pub mod register;
pub mod reports;
mod review;
mod statement;
mod tree;
mod types;

pub use book::{Book, BookMeta, Change, diff};
pub use edit::{AccountPatch, NewAccount, NewTxn, TxnPatch};
pub use ingest::{ImportSource, IngestResult, row as bank_row};
pub use ledger::{Balances, LedgerError};
pub use money::Money;
pub use review::{ReviewItem, ReviewKind};
pub use statement::{CsvProfile, DetectedFormat, FileFormat, ParsedStatement, RawTransaction, SignMode, StatementBalance, StatementUnreadable};
pub use tree::{AccountTree, normalize_iban};
pub use types::*;

/// Re-exported so callers use the same date type.
pub use chrono::NaiveDate;

/// Limits on input sizes. Anything longer is rejected (names) or cut (texts).
pub mod limits {
    pub const NAME: usize = 100;
    pub const DESCRIPTION: usize = 1_000;
    pub const PAYEE: usize = 300;
    pub const TEXT: usize = 2_000;
    pub const IBAN: usize = 34;
    pub const KEY: usize = 220;
    pub const ACCOUNTS: usize = 10_000;
    pub const TXNS: usize = 500_000;
    pub const RULES: usize = 10_000;
    pub const POSTINGS: usize = 100;
    pub const PROFILES: usize = 200;
    pub const BALANCES: usize = 100_000;
    pub const IMPORTS: usize = 10_000;
    /// Rows in one bank file.
    pub const ROWS: usize = 200_000;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("{0} does not exist")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
}

pub type Result<T> = std::result::Result<T, ModelError>;

pub(crate) fn invalid(msg: impl Into<String>) -> ModelError {
    ModelError::Invalid(msg.into())
}

/// `2026-10-16` → the date, if it is a real one.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().filter(|d| (1900..=2200).contains(&chrono::Datelike::year(d)))
}

/// The UTC date of Unix milliseconds.
pub fn date_of(ms: i64) -> NaiveDate {
    chrono::DateTime::from_timestamp_millis(ms).map(|t| t.date_naive()).unwrap_or_default()
}

/// `16 Oct 2026`.
pub fn format_date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

#[cfg(test)]
pub(crate) mod testkit;
