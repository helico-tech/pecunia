//! "Needs attention" on the Overview: what waits in Review, own accounts to confirm, fixed-cost news, bank
//! differences.

use std::collections::HashSet;

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
    /// The keys of the items a person can answer ("Seen", "Not mine"), in no particular order.
    pub fn keys(&self) -> impl Iterator<Item = String> + '_ {
        let fixed = self.price_changes.iter().map(PriceChange::key);
        let fixed = fixed.chain(self.new_fixed_costs.iter().map(NewFixedCost::key)).chain(self.stopped_fixed_costs.iter().map(StoppedCost::key));
        fixed.chain(self.own_accounts.iter().map(OwnAccountCandidate::key))
    }

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

/// Everything the Overview may ask about, as of `today`, without what the person already answered (an IBAN marked
/// "someone else's", a price change marked as seen: [`BookMeta::dismissed`](crate::book::BookMeta)).
pub fn needs_attention(book: &Book, today: NaiveDate) -> NeedsAttention {
    let tree = book.tree();
    let txns: Vec<&Txn> = book.txns().map(|t| t.as_ref()).collect();
    let fc = fixed_costs(&tree, &txns, today);
    let dismissed: HashSet<&str> = book.meta.dismissed.iter().map(String::as_str).collect();
    let open = |key: String| !dismissed.contains(key.as_str());
    NeedsAttention {
        review: review_counts(book),
        own_accounts: own_account_candidates(&tree, &txns).into_iter().filter(|c| open(c.key())).collect(),
        price_changes: fc.price_changes.into_iter().filter(|c| open(c.key())).collect(),
        new_fixed_costs: fc.new.into_iter().filter(|c| open(c.key())).collect(),
        stopped_fixed_costs: fc.stopped.into_iter().filter(|c| open(c.key())).collect(),
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

    #[test]
    fn answered_items_stop_asking() {
        let monthly =
            |id: u64, payee: &str, y: i32, m: u32, day: u32| txn(id, d(y, m, day), Some(BETAAL), payee, &[(BETAAL_FREE, -2999), (GROCERIES, 2999)]);
        let mut txns: Vec<Txn> = (7..=9).map(|m| monthly(u64::from(m), "BASIC FIT", 2026, m, 28)).collect();
        txns.extend((1..=11).map(|i| {
            let (y, m) = if i <= 3 { (2025, 9 + i) } else { (2026, i - 3) };
            monthly(100 + u64::from(i), "SPOTIFY", y, m, 15)
        }));
        let mut b = book(txns);
        let today = d(2026, 10, 6);
        let na = needs_attention(&b, today);
        assert_eq!((na.new_fixed_costs.len(), na.stopped_fixed_costs.len()), (1, 1));
        let new = na.new_fixed_costs[0].key();
        assert_eq!(new, "new:BASIC FIT");
        assert!(na.keys().any(|k| k == new));

        assert_eq!(b.dismiss(&new), Ok(true));
        assert_eq!(b.dismiss(&new), Ok(false), "once is enough");
        assert!(b.dismiss("").is_err());
        assert!(b.dismiss(&"x".repeat(crate::limits::DISMISSED_KEY + 1)).is_err());
        let na = needs_attention(&b, today);
        assert_eq!((na.new_fixed_costs.len(), na.stopped_fixed_costs.len()), (0, 1));
        assert_eq!(b.restore_dismissed(), 1);
        assert_eq!(needs_attention(&b, today).new_fixed_costs.len(), 1);

        // A file with junk in the list: empty, too long and repeated keys are dropped.
        let mut meta = b.meta.clone();
        meta.dismissed = vec![new.clone(), String::new(), "y".repeat(5_000), new.clone()];
        let (accounts, txns, rules) = (b.accounts().map(|a| (**a).clone()).collect(), b.txns().map(|t| (**t).clone()).collect(), vec![]);
        let (b2, _) = Book::from_parts(meta, accounts, txns, rules);
        assert_eq!(b2.meta.dismissed, vec![new]);
    }
}
