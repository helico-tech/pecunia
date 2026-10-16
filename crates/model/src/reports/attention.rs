//! "Needs attention" on the Overview: what waits in Review, own accounts to confirm, fixed-cost news, bank
//! differences.

use chrono::NaiveDate;
use serde::Serialize;

use crate::book::Book;
use crate::reports::{
    BankCheck, BankCheckStatus, NewFixedCost, OwnAccountCandidate, PriceChange, StoppedCost, bank_checks, fixed_costs, own_account_candidates,
};
use crate::review::ReviewKind;
use crate::types::Txn;

/// Transactions waiting in Review, per tab ([`Book::review_kind`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ReviewCounts {
    /// All of them.
    pub to_review: usize,
    /// Looks like a row already in the books.
    pub duplicates: usize,
    /// Probably a transfer between own accounts, to link.
    pub transfers: usize,
    /// With a guess to accept.
    pub suggested: usize,
    /// Nothing to suggest ("Where did this go?").
    pub unknown: usize,
}

pub fn review_counts(book: &Book) -> ReviewCounts {
    let mut c = ReviewCounts::default();
    for kind in book.txns().filter_map(|t| book.review_kind(t)) {
        c.to_review += 1;
        match kind {
            ReviewKind::Duplicate => c.duplicates += 1,
            ReviewKind::Transfer => c.transfers += 1,
            ReviewKind::Suggested => c.suggested += 1,
            ReviewKind::Unknown => c.unknown += 1,
        }
    }
    c
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedsAttention {
    pub review: ReviewCounts,
    pub own_accounts: Vec<OwnAccountCandidate>,
    pub price_changes: Vec<PriceChange>,
    pub new_fixed_costs: Vec<NewFixedCost>,
    pub stopped_fixed_costs: Vec<StoppedCost>,
    /// Bank checks that differ.
    pub bank_differences: Vec<BankCheck>,
}

impl NeedsAttention {
    /// Items in all, for the Overview's badge: transactions to review count as one.
    pub fn count(&self) -> usize {
        usize::from(self.review.to_review > 0)
            + self.own_accounts.len()
            + self.price_changes.len()
            + self.new_fixed_costs.len()
            + self.stopped_fixed_costs.len()
            + self.bank_differences.len()
    }
}

/// Everything the Overview may ask about, as of `today`. The caller drops what the person already answered (an IBAN
/// marked "someone else's", a price change marked as seen).
pub fn needs_attention(book: &Book, today: NaiveDate) -> NeedsAttention {
    let tree = book.tree();
    let txns: Vec<&Txn> = book.txns().map(|t| t.as_ref()).collect();
    let fc = fixed_costs(&tree, &txns, today);
    NeedsAttention {
        review: review_counts(book),
        own_accounts: own_account_candidates(&tree, &txns),
        price_changes: fc.price_changes,
        new_fixed_costs: fc.new,
        stopped_fixed_costs: fc.stopped,
        bank_differences: bank_checks(&tree, &txns, &book.meta.balances)
            .into_iter()
            .filter(|c| matches!(c.status, BankCheckStatus::Differs { .. }))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Money;
    use crate::testkit::*;
    use crate::types::*;

    #[test]
    fn counts_the_review_tabs() {
        let review = |t: Txn| Txn { status: TxnStatus::ToReview, ..t };
        let unsorted = |id: u64| txn(id, d(2026, 9, 30), Some(BETAAL), "x", &[(BETAAL_FREE, -100), (NOT_SORTED, 100)]);
        let guessed = Txn { guesses: vec![Guess { account: GROCERIES, probability: 0.7 }], ..review(unsorted(1)) };
        // Booked on a category but nobody confirmed it, and no guess: Review shows it as unknown.
        let assigned = review(txn(2, d(2026, 9, 20), Some(BETAAL), "x", &[(BETAAL_FREE, 100), (SALARY, -100)]));
        let transfer = Txn { transfer_likely: true, ..review(unsorted(3)) };
        let duplicate = Txn { duplicate_of: Some(TxnId(1)), ..review(unsorted(6)) };
        let mut b = book(vec![guessed, assigned, transfer, review(unsorted(4)), unsorted(5), duplicate]);
        assert_eq!(review_counts(&b), ReviewCounts { to_review: 5, duplicates: 1, transfers: 1, suggested: 1, unknown: 2 });
        assert_eq!(review_counts(&b).to_review, b.review_count());

        b.meta.balances = vec![StatedBalance { account: BETAAL, date: d(2026, 9, 30), amount: Money(-1), source: "a.sta".into() }];
        let na = needs_attention(&b, d(2026, 10, 6));
        assert_eq!(na.review.to_review, 5);
        assert!(na.own_accounts.is_empty() && na.price_changes.is_empty() && na.new_fixed_costs.is_empty() && na.stopped_fixed_costs.is_empty());
        assert_eq!(na.bank_differences.iter().map(|c| c.account).collect::<Vec<_>>(), [BETAAL]);
        assert_eq!(na.count(), 2, "review counts as one");
    }
}
