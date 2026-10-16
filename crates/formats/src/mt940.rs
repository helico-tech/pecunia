//! MT940 (SWIFT statement, Latin-1 text). From budget.rs `files/mt940.rs`.
//!
//! `:61:` is the statement line, `:86:` the details: up to 6 × 65 characters, wrapped at a fixed width even mid-word,
//! so continuation lines are joined without a space. The details come in the banks' own styles; this reads:
//! - structured fields: `/TRTP/…/NAME/…/IBAN/…/REMI/…` (ABN AMRO, Rabobank) and `/CNTP/iban/bic/name/…` (ING);
//! - labelled text: `SEPA … NAAM: … OMSCHRIJVING: … IBAN: …` (ABN AMRO direct debits and transfers);
//! - card payments and cash withdrawals: `BEA, …  MERCHANT,PAS123 …` / `GEA, …` (merchant = text before `,PAS`).
//!
//! Each statement's opening balance plus its lines must equal its closing balance; a mismatch becomes a warning.
//! MT940 has no stable transaction id, so dedup relies on the import hash.

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::NaiveDate;
use pecunia_model::{Money, ParsedStatement, RawTransaction, StatementBalance, StatementUnreadable};
use regex::{Captures, Regex};

use crate::{find_iban, is_blank, latin1, lines, non_blank, push_unique, re};

// Java regex classes are ASCII; spelled out so Rust's Unicode `\d`, `\w`, `\s` do not widen them.
static TAG: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"^:([0-9]{2}[A-Z]?):(.*)$"));
static LINE61: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"^([0-9]{6})([0-9]{4})?(R?[DC])([A-Z])?([0-9]+,[0-9]{0,2})([A-Z][A-Za-z0-9_]{3})?(.*)$"));
static BALANCE: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"^([DC])([0-9]{6})[A-Z]{3}([0-9]+,[0-9]{0,2})"));

/// Capture group `i`, or `""` when it did not take part.
fn group<'t>(c: &Captures<'t>, i: usize) -> &'t str {
    c.get(i).map_or("", |m| m.as_str())
}

pub(crate) fn parse(content: &[u8]) -> Result<ParsedStatement, StatementUnreadable> {
    let (Some(tag_re), Some(line61_re)) = (TAG.as_ref(), LINE61.as_ref()) else {
        return Err(StatementUnreadable::new("the MT940 reader is not available"));
    };
    let text = latin1(content);
    // (tag, value, line of the tag); continuation lines join the field before them.
    let mut fields: Vec<(String, String, usize)> = Vec::new();
    for (n, raw) in lines(&text).into_iter().enumerate() {
        let line = raw.trim_end_matches('\r');
        if let Some(c) = tag_re.captures(line) {
            fields.push((group(&c, 1).to_string(), group(&c, 2).to_string(), n + 1));
        } else if line.starts_with("-}") || line.starts_with('{') || is_blank(line) {
        } else if let Some((tag, value, _)) = fields.last_mut() {
            if tag != "86" {
                value.push('\n');
            }
            value.push_str(line);
        }
    }
    if !fields.iter().any(|(tag, _, _)| tag == "61") {
        return Err(StatementUnreadable::new("no :61: statement lines: this is not an MT940 file"));
    }
    let mut statement = ParsedStatement::default();
    let mut own: Option<String> = None;
    let mut opening: Option<Money> = None;
    let mut running = Money::ZERO;
    for (i, (tag, value, line_no)) in fields.iter().enumerate() {
        let at = |e: String| StatementUnreadable::at(*line_no, e);
        match tag.as_str() {
            "25" => {
                own = value.trim().split([' ', '/']).next().filter(|s| s.chars().count() > 8).map(str::to_string);
                if let Some(own) = &own {
                    push_unique(&mut statement.account_ibans, own.clone());
                }
            }
            "60F" | "60M" => {
                opening = balance(value).map_err(at)?.map(|b| b.amount);
                running = Money::ZERO;
            }
            "62F" | "62M" => {
                if let Some(b) = balance(value).map_err(at)? {
                    if let Some(start) = opening
                        && start + running != b.amount
                    {
                        statement.warnings.push(format!(
                            "statement ending {}: opening {} + lines {} ≠ closing {}",
                            b.date,
                            start.format(),
                            running.format(),
                            b.amount.format()
                        ));
                    }
                    if tag == "62F" {
                        statement.closing_balances.push(b);
                    }
                }
            }
            "61" => {
                let line = value.trim();
                let c =
                    line61_re.captures(line).ok_or_else(|| at(format!("unreadable :61: line '{}'", value.chars().take(40).collect::<String>())))?;
                let date = yymmdd(group(&c, 1)).map_err(at)?;
                let mut amount = Money::parse(group(&c, 5)).map_err(at)?;
                let dc = group(&c, 3);
                if dc.ends_with('D') {
                    amount = -amount;
                }
                if dc.starts_with('R') {
                    amount = -amount;
                }
                running += amount;
                let details = match fields.get(i + 1) {
                    Some((next, details, _)) if next == "86" => details.as_str(),
                    _ => "",
                };
                let d = read_details(details);
                statement.rows.push(RawTransaction {
                    date,
                    amount,
                    description: d.description,
                    counterparty_name: d.name,
                    counterparty_iban: d.iban.or_else(|| find_iban(details)),
                    external_id: None,
                    // budget.rs (and Kotlin before it) wrote a missing IBAN as "null"; kept so import hashes stay the same.
                    raw_line: format!("mt940|{}|{line}|{details}", own.as_deref().unwrap_or("null")),
                    own_iban: own.clone(),
                    who: None,
                });
            }
            _ => {}
        }
    }
    Ok(statement)
}

/// A `:60F:`/`:62F:` balance: `C260102EUR84,50`. `None` when the field has another shape.
fn balance(text: &str) -> Result<Option<StatementBalance>, String> {
    let Some(c) = BALANCE.as_ref().and_then(|re| re.captures(text.trim())) else { return Ok(None) };
    let amount = Money::parse(group(&c, 3))?;
    Ok(Some(StatementBalance { date: yymmdd(group(&c, 2))?, amount: if group(&c, 1) == "D" { -amount } else { amount } }))
}

/// Six digits `yyMMdd`, in this century.
fn yymmdd(s: &str) -> Result<NaiveDate, String> {
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    NaiveDate::from_ymd_opt(2000 + n(0..2) as i32, n(2..4), n(4..6))
        .ok_or_else(|| format!("not a date: '{}'", s.chars().take(20).collect::<String>()))
}

/// Name, IBAN and description from the `:86:` details.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Details {
    pub name: Option<String>,
    pub iban: Option<String>,
    pub description: String,
}

const STRUCTURED_KEYS: [&str; 18] =
    ["TRTP", "CSID", "NAME", "MARF", "REMI", "IBAN", "BIC", "EREF", "ORDP", "BENM", "ULTC", "ULTD", "PURP", "RTRN", "ADDR", "CNTP", "SVCL", "BUSP"];
const LABELS: [&str; 7] = ["INCASSANT", "NAAM", "MACHTIGING", "OMSCHRIJVING", "IBAN", "BIC", "KENMERK"];

static STRUCTURED: LazyLock<Option<Regex>> = LazyLock::new(|| re(&format!("/({})/", STRUCTURED_KEYS.join("|"))));
// Kotlin had `(?<![A-Z])` in front, which the regex crate lacks; `labelled` checks the character before instead. No
// \b: wrapped lines are joined without a space, so a label can directly follow a digit ("…80000NAAM:").
static LABELLED: LazyLock<Option<Regex>> = LazyLock::new(|| re(&format!("({}):", LABELS.join("|"))));
static CARD: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"^(BEA|GEA),[\t\n\x0B\x0C\r ]*(?:[A-Z ]*?PAY|BETAALPAS|[A-Z]+)?[\t\n\x0B\x0C\r ]+(.+?),PAS[0-9]+"));

/// Reads the `:86:` details in whichever style the bank used.
pub(crate) fn read_details(raw: &str) -> Details {
    let text = raw.trim();
    if text.starts_with('/') && STRUCTURED.as_ref().is_some_and(|re| re.is_match(text)) {
        read_structured(text)
    } else if text.starts_with("BEA,") || text.starts_with("GEA,") {
        let name = CARD.as_ref().and_then(|re| re.captures(text)).map(|c| squash(group(&c, 2)));
        Details { name, iban: None, description: squash(text) }
    } else if !labelled(text).is_empty() {
        read_labelled(text)
    } else if text.starts_with("ABN AMRO BANK") {
        // The bank's own lines (fees, notes) carry no name field: the bank is the counterparty.
        Details { name: Some("ABN AMRO BANK N.V.".into()), iban: None, description: squash(text) }
    } else {
        Details { name: None, iban: None, description: squash(text) }
    }
}

/// `(start, end, key)` of each match, and the value after each key up to the next one (first occurrence wins).
fn values_between<'t>(text: &'t str, matches: &[(usize, usize, &'t str)]) -> HashMap<&'t str, &'t str> {
    let mut values = HashMap::new();
    for (i, &(_, end, key)) in matches.iter().enumerate() {
        let stop = matches.get(i + 1).map_or(text.len(), |m| m.0);
        values.entry(key).or_insert(text.get(end..stop).unwrap_or(""));
    }
    values
}

fn read_structured(text: &str) -> Details {
    let matches: Vec<_> = STRUCTURED
        .as_ref()
        .map(|re| {
            re.captures_iter(text).filter_map(|c| Some((c.get(0)?, c.get(1)?))).map(|(all, key)| (all.start(), all.end(), key.as_str())).collect()
        })
        .unwrap_or_default();
    let values: HashMap<&str, String> =
        values_between(text, &matches).into_iter().map(|(k, v)| (k, v.trim_matches('/').trim().to_string())).collect();
    // ING: /CNTP/iban/bic/name/…
    if let Some(cntp) = values.get("CNTP") {
        let parts: Vec<&str> = cntp.split('/').collect();
        let remi = values.get("REMI").map(|r| {
            let r = r.strip_prefix("USTD//").unwrap_or(r);
            r.strip_prefix("USTD/").unwrap_or(r)
        });
        return Details {
            name: parts.get(2).filter(|p| !is_blank(p)).map(|p| squash(p)),
            iban: parts.first().filter(|p| !is_blank(p)).map(|p| p.to_string()),
            description: squash(remi.unwrap_or(text)),
        };
    }
    let kind = values.get("TRTP").map(|t| squash(t));
    let remi = values.get("REMI").map(|r| squash(r)).unwrap_or_default();
    let description = non_blank(remi).into_iter().chain(kind).collect::<Vec<_>>().join(" · ");
    Details {
        name: values.get("NAME").map(|n| squash(n)),
        iban: values.get("IBAN").map(|i| i.replace(' ', "")),
        description: if is_blank(&description) { squash(text) } else { description },
    }
}

/// The label matches not preceded by a capital (Kotlin's `(?<![A-Z])`): after a miss, search again one further.
fn labelled(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let Some(re) = LABELLED.as_ref() else { return out };
    let mut from = 0;
    while let Some(c) = re.captures_at(text, from) {
        let (Some(all), Some(label)) = (c.get(0), c.get(1)) else { break };
        if text.get(..all.start()).and_then(|before| before.chars().next_back()).is_some_and(|ch| ch.is_ascii_uppercase()) {
            from = all.start() + 1; // labels start with an ASCII letter, so this is a character boundary
        } else {
            out.push((all.start(), all.end(), label.as_str()));
            from = all.end();
        }
    }
    out
}

fn read_labelled(text: &str) -> Details {
    let matches = labelled(text);
    let values: HashMap<&str, String> = values_between(text, &matches).into_iter().map(|(k, v)| (k, squash(v))).collect();
    let kind = squash(text.get(..matches.first().map_or(text.len(), |m| m.0)).unwrap_or(""));
    let description =
        [values.get("OMSCHRIJVING").cloned().and_then(non_blank), non_blank(kind)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    Details {
        name: values.get("NAAM").cloned().and_then(non_blank),
        iban: values.get("IBAN").and_then(|i| non_blank(i.replace(' ', ""))),
        description: if is_blank(&description) { squash(text) } else { description },
    }
}

/// Runs of ASCII white space become one space; then trimmed.
fn squash(s: &str) -> String {
    s.split(['\t', '\n', '\x0B', '\x0C', '\r', ' ']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ").trim().to_string()
}
