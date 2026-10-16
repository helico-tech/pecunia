//! Dedup keys and fuzzy duplicate matching. From budget.rs `dedup.rs`.
//!
//! In order: (a) same account + external id, (b) same account + import hash, (c) fuzzy: same account, same amount,
//! dates at most [`MAX_DAYS`] apart, similar payee → a *possible* duplicate for the person, never dropped silently.

use chrono::NaiveDate;

use crate::money::Money;
use crate::payees;
use crate::types::{SourceKind, TxnId};

/// The widest gap in days between two copies of one bank row (fuzzy dedup and transfer matching).
pub const MAX_DAYS: i64 = 3;
/// The lowest payee similarity for a possible duplicate.
pub const MIN_SIMILARITY: f64 = 0.5;

/// Key for the source's own id: `ext:CAMT053:abc`.
pub fn external_key(source: SourceKind, id: &str) -> String {
    let id: String = id.chars().take(200).collect();
    format!("ext:{}:{id}", source.id().to_uppercase())
}

/// Key for an import hash: `hash:<hex>`.
pub fn hash_key(hash: &str) -> String {
    format!("hash:{hash}")
}

/// A stored transaction of the same account as the fuzzy matcher sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct DedupCandidate {
    pub id: TxnId,
    pub date: NaiveDate,
    /// The amount on the account (the bank amount).
    pub amount: Money,
    pub payee_norm: String,
    pub source: SourceKind,
}

/// A possible duplicate of a new row among the account's stored transactions from *another* source (the same source
/// has exact keys): same amount, dates at most [`MAX_DAYS`] apart, payee similarity at least [`MIN_SIMILARITY`]. The
/// closest date wins, then the more similar payee. One original absorbs one duplicate, so the caller drops a match
/// from the candidates once used.
pub fn find_possible_duplicate<'a>(
    date: NaiveDate,
    amount: Money,
    payee_norm: &str,
    source: SourceKind,
    candidates: &'a [DedupCandidate],
) -> Option<&'a DedupCandidate> {
    candidates
        .iter()
        .filter(|c| c.amount == amount && c.source != source)
        .map(|c| (c, (c.date - date).num_days().abs(), payees::similarity(&c.payee_norm, payee_norm)))
        .filter(|(_, days, sim)| *days <= MAX_DAYS && *sim >= MIN_SIMILARITY)
        .min_by(|(_, da, sa), (_, db, sb)| (*da as f64 - sa).total_cmp(&(*db as f64 - sb)))
        .map(|(c, _, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::d;

    #[test]
    fn keys() {
        assert_eq!(external_key(SourceKind::Camt053, "x"), "ext:CAMT053:x");
        assert_eq!(hash_key("abc"), "hash:abc");
    }

    #[test]
    fn fuzzy_dedup_matches_across_sources_within_three_days() {
        let mt940 = DedupCandidate {
            id: TxnId(1),
            date: d(2026, 9, 30),
            amount: Money(-5210),
            payee_norm: "ALBERT HEIJN UTRECHT".into(),
            source: SourceKind::Mt940,
        };
        let cands = [mt940];
        let find = |date, cents, payee, source| find_possible_duplicate(date, Money(cents), payee, source, &cands);
        assert!(find(d(2026, 9, 29), -5210, "ALBERT HEIJN", SourceKind::Csv).is_some());
        assert!(find(d(2026, 9, 20), -5210, "ALBERT HEIJN", SourceKind::Csv).is_none(), "too far apart");
        assert!(find(d(2026, 9, 29), -5211, "ALBERT HEIJN", SourceKind::Csv).is_none(), "other amount");
        assert!(find(d(2026, 9, 29), -5210, "JUMBO", SourceKind::Csv).is_none(), "other payee");
        assert!(find(d(2026, 9, 29), -5210, "ALBERT HEIJN", SourceKind::Mt940).is_none(), "same source");
    }

    #[test]
    fn fuzzy_dedup_prefers_the_closest_date() {
        let c =
            |id, day| DedupCandidate { id: TxnId(id), date: d(2026, 9, day), amount: Money(-1000), payee_norm: "NS".into(), source: SourceKind::Csv };
        let cands = [c(1, 27), c(2, 29), c(3, 30)];
        let hit = find_possible_duplicate(d(2026, 9, 29), Money(-1000), "NS", SourceKind::Mt940, &cands);
        assert_eq!(hit.map(|c| c.id), Some(TxnId(2)));
    }
}
