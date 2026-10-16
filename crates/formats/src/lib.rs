#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Pecunia's bank-file parsers (L0): the CSV layouts of Dutch banks, CAMT.053 and MT940. Bytes in, rows out.
//!
//! Ported from budget.rs `budget-adapters/src/files`; its `StatementFormats` port became the plain functions
//! [`built_in_profiles`], [`detect`] and [`parse`]. Every file is hostile: a file that cannot be read is a
//! [`StatementUnreadable`] (or `None`), never a panic. Amounts are integer cents, capped at [`Money::MAX_INPUT`].
//! [`samples`] writes synthetic bank files for tests and the sample book.
//!
//! [`Money::MAX_INPUT`]: pecunia_model::Money::MAX_INPUT

mod camt053;
mod csv;
mod mt940;
pub mod samples;

use std::sync::LazyLock;

use pecunia_model::{CsvProfile, DetectedFormat, FileFormat, ParsedStatement, StatementUnreadable};
use regex::Regex;

pub use csv::CsvPreview;

/// The CSV layouts of the Dutch banks, in detection order.
pub fn built_in_profiles() -> Vec<CsvProfile> {
    csv::profiles::built_in()
}

/// Recognises CAMT.053, MT940 and the CSV layouts. CSV profiles are tried in order: the built-in ones first, then
/// `extra` (the profiles the person saved in the book). `None` when it is a CSV that no profile fits, so the person
/// maps the columns.
pub fn detect(content: &[u8], extra: &[CsvProfile]) -> Option<DetectedFormat> {
    let head = latin1(content.get(..4096).unwrap_or(content));
    let mt940_start = MT940_START.as_ref().is_some_and(|re| re.is_match(&head));
    if head.contains("camt.053") || head.contains("<BkToCstmrStmt") {
        Some(DetectedFormat::camt053())
    } else if mt940_start && head.contains(":61:") || head.contains(":25:") && head.contains(":60F:") {
        Some(DetectedFormat::mt940())
    } else {
        csv::detect(content, &all_profiles(extra)).map(|p| DetectedFormat::csv(&p.name))
    }
}

/// Reads `content` as `format`. A CSV profile is looked up by name: the built-in ones first, then `extra`. A file
/// that cannot be read is an error that names the line when it is known.
pub fn parse(content: &[u8], format: &DetectedFormat, extra: &[CsvProfile]) -> Result<ParsedStatement, StatementUnreadable> {
    match format.format {
        FileFormat::Camt053 => camt053::parse(content),
        FileFormat::Mt940 => mt940::parse(content),
        FileFormat::Csv => {
            let wanted = format.profile.as_deref();
            let profile = built_in_profiles()
                .into_iter()
                .chain(extra.iter().cloned())
                .find(|p| Some(p.name.as_str()) == wanted)
                .ok_or_else(|| StatementUnreadable::new(format!("unknown CSV profile '{}'", wanted.unwrap_or("null"))))?;
            csv::parse(&profile, content)
        }
    }
}

/// The first lines of a CSV, for a column-mapping dialog: the delimiter guessed, the header cells and up to `rows`
/// data rows (cells trimmed, as the parser reads them). `None` when the content does not read as a table of two or
/// more columns.
pub fn preview_csv(content: &[u8], rows: usize) -> Option<CsvPreview> {
    csv::preview(content, rows)
}

fn all_profiles(extra: &[CsvProfile]) -> Vec<CsvProfile> {
    let mut all = built_in_profiles();
    all.extend(extra.iter().cloned());
    all
}

/// The patterns are constants; one that failed to compile would mean a format is not recognised, never a crash.
pub(crate) fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

// Java's `\s` is ASCII only; Rust's is Unicode (it would also eat the NBSP of a Latin-1 file).
static MT940_START: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"(^|\n)[\t\n\x0B\x0C\r ]*(\{4:)?[\t\n\x0B\x0C\r ]*:20:"));

/// ISO-8859-1: every byte is the code point of the same number (not windows-1252).
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Splits on `\r\n`, `\n` and `\r`, and keeps a trailing empty line (Kotlin's `String.lines()`).
pub(crate) fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut bytes = text.bytes().enumerate().peekable();
    while let Some((i, b)) = bytes.next() {
        if b == b'\n' || b == b'\r' {
            // Line ends are ASCII, so `start` and `i` are character boundaries.
            out.push(text.get(start..i).unwrap_or(""));
            start = i + 1;
            if b == b'\r' && bytes.next_if(|&(_, next)| next == b'\n').is_some() {
                start = i + 2;
            }
        }
    }
    out.push(text.get(start..).unwrap_or(""));
    out
}

pub(crate) fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// `Some(s)` unless `s` is blank.
pub(crate) fn non_blank(s: impl Into<String>) -> Option<String> {
    let s = s.into();
    if is_blank(&s) { None } else { Some(s) }
}

/// Adds `value` unless already there, keeping the order.
pub(crate) fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// The first IBAN-looking token in `text`.
pub(crate) fn find_iban(text: &str) -> Option<String> {
    pecunia_model::payees::iban(text)
}

#[cfg(test)]
mod tests;
