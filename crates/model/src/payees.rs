//! Turns noisy bank text into a stable payee name, compares payees, and hashes bank rows. From budget.rs `payees.rs`.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::statement::RawTransaction;

const NOISE_PREFIXES: [&str; 16] = [
    "BEA, APPLE PAY",
    "BEA, GOOGLE PAY",
    "BEA, BETAALPAS",
    "BEA,",
    "GEA, BETAALPAS",
    "GEA,",
    "BEA ",
    "GEA ",
    "SEPA INCASSO ALGEMEEN DOORLOPEND",
    "SEPA INCASSO",
    "SEPA OVERBOEKING",
    "IDEAL",
    "BETAALAUTOMAAT",
    "ONLINE BANKIEREN",
    "APPLE PAY",
    "GOOGLE PAY",
];

/// The patterns are constants; a pattern that failed to compile would only mean less cleaning, never a crash.
fn re(p: &str) -> Option<Regex> {
    Regex::new(p).ok()
}

static LEGAL_FORMS: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"\b(B\.?V\.?|N\.?V\.?|V\.?O\.?F\.?|GMBH|LTD|S\.?A\.?R\.?L\.?|INC)\b\.?"));
static IBAN: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"\b([A-Z]{2}[0-9]{2}[A-Z]{4}[0-9]{6,10})\b"));
static NOISE_TOKENS: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\b(PAS[0-9]*|NR:?[A-Za-z0-9_]*|TERM:?[A-Za-z0-9_]*|CT[A-Za-z0-9_]*[0-9][A-Za-z0-9_]*|TRANSACTIE:?[A-Za-z0-9_]*)\b"));
static DATE_TIME: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\b[0-9]{1,2}[.\-/][0-9]{1,2}[.\-/][0-9]{2,4}\b|\b[0-9]{1,2}:[0-9]{2}(:[0-9]{2})?\b|\b[0-9]{4}-[0-9]{2}-[0-9]{2}\b"));
static PUNCTUATION: LazyLock<Option<Regex>> = LazyLock::new(|| re(r#"[*_/\\,;:'"()\[\]#]"#));

/// Finds the first IBAN-looking token in `text` (spaces inside the IBAN allowed).
pub fn iban(text: &str) -> Option<String> {
    let re = IBAN.as_ref()?;
    let upper = text.to_uppercase();
    re.find(&upper.replace(' ', "")).or_else(|| re.find(&upper)).map(|m| m.as_str().to_string())
}

/// Normalised payee: upper case, no payment-method prefixes, terminal ids, dates, numbers, legal forms or punctuation.
/// "BEA, Apple Pay ALBERT HEIJN 1427,PAS123 NR:CT59X1" → "ALBERT HEIJN". Uses the counterparty name when there is one,
/// else the description.
pub fn normalize(counterparty_name: Option<&str>, description: &str) -> String {
    let base = counterparty_name.filter(|n| !n.trim().is_empty()).unwrap_or(description);
    let base: String = base.chars().take(500).collect();
    let mut s = base.to_uppercase().replace('\u{a0}', " ");
    for p in NOISE_PREFIXES {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
        }
    }
    for re in [&*IBAN, &*DATE_TIME, &*NOISE_TOKENS, &*LEGAL_FORMS, &*PUNCTUATION].into_iter().flatten() {
        s = re.replace_all(&s, " ").into_owned();
    }
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphabetic) && t.chars().filter(|c| c.is_numeric()).count() < 3)
        .map(|t| t.trim_matches(['.', '-']))
        .filter(|t| !t.is_empty())
        .take(4) // payee names rarely need more than four words
        .collect();
    if words.is_empty() { base.trim().to_uppercase().chars().take(40).collect() } else { words.join(" ") }
}

/// The words of a normalised payee (two characters or more), for similarity.
pub fn tokens(norm: &str) -> BTreeSet<String> {
    norm.to_uppercase().split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit())).filter(|t| t.len() >= 2).map(str::to_string).collect()
}

/// Similarity 0..1: token overlap (Jaccard), 1 when equal, 0.9 when one name's words contain the other's.
pub fn similarity(a: &str, b: &str) -> f64 {
    if a.trim().is_empty() || b.trim().is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    if ta.is_superset(&tb) || tb.is_superset(&ta) {
        return 0.9;
    }
    ta.intersection(&tb).count() as f64 / ta.union(&tb).count() as f64
}

/// Hex SHA-256 of `text`.
pub fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Import hashes for one file: a hash of the raw row as read, plus its occurrence number among identical rows in the
/// same file. Two identical coffees on one day stay two transactions, and importing the file again gives the same
/// hashes.
pub fn import_hashes(rows: &[RawTransaction]) -> Vec<String> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    rows.iter()
        .map(|r| {
            let line = if r.raw_line.is_empty() {
                let opt = |o: &Option<String>| o.clone().unwrap_or_else(|| "null".into());
                format!("{}|{}|{}|{}|{}", r.date, r.amount.cents(), opt(&r.counterparty_name), opt(&r.counterparty_iban), r.description)
            } else {
                r.raw_line.clone()
            };
            let n = seen.entry(line.clone()).or_default();
            *n = n.saturating_add(1);
            sha256(&format!("{line}#{n}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::money::Money;

    #[test]
    fn normalises_dutch_bank_descriptions() {
        for (raw, expected) in [
            ("BEA, Apple Pay ALBERT HEIJN 1427,PAS123 NR:CT59X1, 03.10.26/18:42 UTRECHT", "ALBERT HEIJN UTRECHT"),
            ("BEA, Apple Pay KRUIDVAT 7731,PAS123 NR:CT59X1 02.10.26/12:10 UTRECHT", "KRUIDVAT UTRECHT"),
            ("BOL.COM B.V.", "BOL.COM"),
            ("SEPA Incasso algemeen doorlopend Vattenfall Klantenservice N.V.", "VATTENFALL KLANTENSERVICE"),
        ] {
            assert_eq!(normalize(None, raw), expected, "{raw}");
        }
        let zk = normalize(Some("Zilveren Kruis Zorgverzekeringen N.V."), "premie");
        assert_eq!(zk.split(' ').take(2).collect::<Vec<_>>().join(" "), "ZILVEREN KRUIS");
    }

    #[test]
    fn finds_ibans() {
        assert_eq!(iban("NL98RABO0123456789 J. de Vries Huur garagebox okt").as_deref(), Some("NL98RABO0123456789"));
        assert_eq!(iban("Albert Heijn"), None);
    }

    #[test]
    fn similarity_of_payees() {
        assert_eq!(similarity("ALBERT HEIJN", "ALBERT HEIJN"), 1.0);
        assert!(similarity("ALBERT HEIJN UTRECHT", "ALBERT HEIJN") >= 0.9);
        assert!(similarity("JUMBO", "ALBERT HEIJN") < 0.5);
        assert_eq!(similarity("", "JUMBO"), 0.0);
        assert!((similarity("JUMBO UTRECHT", "JUMBO AMSTERDAM") - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn identical_rows_in_one_file_get_different_stable_hashes() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let mut coffee = RawTransaction::new(d, Money(-350), "COFFEE");
        coffee.raw_line = "20261001;Coffee;-3,50".into();
        let rows = [coffee.clone(), coffee];
        let first = import_hashes(&rows);
        assert_ne!(first[0], first[1]);
        assert_eq!(first, import_hashes(&rows));
        assert_eq!(first[0], sha256("20261001;Coffee;-3,50#1"));
        let bare = RawTransaction::new(d, Money(-350), "COFFEE");
        assert_eq!(import_hashes(&[bare]), vec![sha256("2026-10-01|-350|null|null|COFFEE#1")]);
        assert_eq!(sha256("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
