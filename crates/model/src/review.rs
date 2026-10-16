//! Review (budget.rs `review.rs`): transactions waiting for a person, and the answers that work on many at once.
//! Single answers are [`Book::answer`].

use std::collections::BTreeMap;

use serde::Serialize;

use crate::book::Book;
use crate::classify::counter_account;
use crate::types::*;
use crate::{Result, invalid};

/// The tab an item is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    /// Looks like a row already in the books from another file.
    Duplicate,
    /// Probably money between own accounts.
    Transfer,
    /// The classifier has a guess.
    Suggested,
    /// Nobody knows yet.
    Unknown,
}

impl ReviewKind {
    pub const ALL: [ReviewKind; 4] = [ReviewKind::Duplicate, ReviewKind::Transfer, ReviewKind::Suggested, ReviewKind::Unknown];

    pub fn id(self) -> &'static str {
        match self {
            ReviewKind::Duplicate => "duplicate",
            ReviewKind::Transfer => "transfer",
            ReviewKind::Suggested => "suggested",
            ReviewKind::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ReviewKind::Duplicate => "Possible duplicates",
            ReviewKind::Transfer => "Transfers",
            ReviewKind::Suggested => "Suggestions",
            ReviewKind::Unknown => "Unknown",
        }
    }

    pub fn parse(s: &str) -> Option<ReviewKind> {
        ReviewKind::ALL.into_iter().find(|k| k.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// One transaction waiting in Review.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    pub id: TxnId,
    pub kind: ReviewKind,
    /// "Earlier: Kids 4×, Gifts 3×": the person's past answers for this payee.
    pub history: Vec<(AccountId, u32)>,
}

impl Book {
    /// What kind of Review item a transaction is, or `None` when it needs nobody.
    pub fn review_kind(&self, t: &Txn) -> Option<ReviewKind> {
        if t.duplicate_of.is_some() {
            return Some(ReviewKind::Duplicate);
        }
        if t.status != TxnStatus::ToReview {
            return None;
        }
        Some(if t.transfer_likely {
            ReviewKind::Transfer
        } else if t.guesses.is_empty() {
            ReviewKind::Unknown
        } else {
            ReviewKind::Suggested
        })
    }

    /// Transactions waiting for a person, newest first.
    pub fn review(&self) -> Vec<ReviewItem> {
        let tree = self.tree();
        let mut answers: BTreeMap<&str, BTreeMap<AccountId, u32>> = BTreeMap::new();
        for t in self.txns().filter(|t| t.status == TxnStatus::Reviewed && t.class_source == Some(ClassSource::User) && !t.payee_norm.is_empty()) {
            if let Some(c) = counter_account(&tree, t) {
                *answers.entry(t.payee_norm.as_str()).or_default().entry(c).or_default() += 1;
            }
        }
        let mut items: Vec<(chrono::NaiveDate, ReviewItem)> = self
            .txns()
            .filter_map(|t| {
                let kind = self.review_kind(t)?;
                let mut history: Vec<(AccountId, u32)> =
                    answers.get(t.payee_norm.as_str()).map(|m| m.iter().map(|(a, n)| (*a, *n)).collect()).unwrap_or_default();
                history.sort_by_key(|(a, n)| (std::cmp::Reverse(*n), *a));
                history.truncate(3);
                Some((t.date, ReviewItem { id: t.id, kind, history }))
            })
            .collect();
        items.sort_by(|(da, a), (db, b)| db.cmp(da).then(b.id.cmp(&a.id)));
        items.into_iter().map(|(_, i)| i).collect()
    }

    /// How many transactions wait in Review.
    pub fn review_count(&self) -> usize {
        self.txns().filter(|t| self.review_kind(t).is_some()).count()
    }

    /// Accepts the top guess of suggested transactions (`ids`, or all of them). Returns how many were booked.
    pub fn accept_suggestions(&mut self, ids: Option<&[TxnId]>) -> Result<usize> {
        let picks: Vec<(TxnId, AccountId)> = self
            .txns()
            .filter(|t| ids.is_none_or(|ids| ids.contains(&t.id)))
            .filter(|t| self.review_kind(t) == Some(ReviewKind::Suggested))
            .filter_map(|t| t.guesses.first().map(|g| (t.id, g.account)))
            .collect();
        if picks.is_empty() && ids.is_some() {
            return Err(invalid("none of these has a suggestion to accept"));
        }
        let mut n = 0;
        for (id, account) in picks {
            if self.answer(id, account, false).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Answers every waiting transaction with the same payee as `id` the same way. Returns how many.
    pub fn answer_all_like(&mut self, id: TxnId, counter: AccountId) -> Result<usize> {
        let payee = self.txn(id).map(|t| t.payee_norm.clone()).ok_or_else(|| crate::ModelError::NotFound(format!("transaction {id}")))?;
        if payee.is_empty() {
            return Err(invalid("this transaction has no payee to match others by"));
        }
        let same: Vec<TxnId> = self
            .txns()
            .filter(|t| t.payee_norm == payee && matches!(self.review_kind(t), Some(ReviewKind::Suggested | ReviewKind::Unknown)))
            .map(|t| t.id)
            .collect();
        let mut n = 0;
        for t in same {
            self.answer(t, counter, false)?;
            n += 1;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn waiting(id: u64, payee: &str, guess: Option<AccountId>) -> Txn {
        Txn {
            status: TxnStatus::ToReview,
            guesses: guess.map(|g| vec![Guess { account: g, probability: 0.6 }]).unwrap_or_default(),
            ..txn(id, d(2026, 9, id as u32), Some(BETAAL), payee, &[(BETAAL_FREE, -100), (NOT_SORTED, 100)])
        }
    }

    #[test]
    fn sorts_waiting_transactions_into_tabs() {
        let mut b = book(vec![
            waiting(1, "AH", Some(GROCERIES)),
            waiting(2, "NEW", None),
            Txn { transfer_likely: true, ..waiting(3, "SPAAR", Some(SPAAR)) },
            Txn { duplicate_of: Some(TxnId(1)), ..waiting(4, "AH", None) },
            txn(5, d(2026, 9, 5), Some(BETAAL), "DONE", &[(BETAAL_FREE, -1), (GROCERIES, 1)]),
        ]);
        let kinds: Vec<(u64, ReviewKind)> = b.review().iter().map(|i| (i.id.0, i.kind)).collect();
        assert_eq!(kinds, vec![(4, ReviewKind::Duplicate), (3, ReviewKind::Transfer), (2, ReviewKind::Unknown), (1, ReviewKind::Suggested)]);
        assert_eq!(b.review_count(), 4);
        assert_eq!(b.accept_suggestions(None).unwrap(), 1);
        assert_eq!(b.txn(TxnId(1)).unwrap().status, TxnStatus::Reviewed);
        assert_eq!(b.answer_all_like(TxnId(2), GROCERIES).unwrap(), 1);
        assert_eq!(b.review_count(), 2);
        assert_eq!(b.validate(), Ok(()));
    }
}
