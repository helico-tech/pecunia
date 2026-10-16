//! Ledger rules over plain data (ADR 0003): building and checking transactions, balances, transfer matching. Pure
//! functions; [`crate::Book`] calls them and stores the result. From budget.rs `ledger/`.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::dedup::MAX_DAYS;
use crate::money::Money;
use crate::tree::AccountTree;
use crate::types::{AccountId, AccountKind, Group, Posting, SystemAccount, Txn, TxnKind};

/// Why the books refuse a booking or an account. Every message is a sentence for people.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    #[error("Account {0} does not exist.")]
    AccountNotFound(AccountId),
    #[error("{name} has parts of its own; book on one of them instead.")]
    NotALeaf { name: String },
    #[error("A transaction needs at least two lines.")]
    TooFewPostings,
    #[error("A line of zero euros does nothing; leave it out.")]
    ZeroPosting,
    #[error("The lines do not add up to zero: they are {0} off.")]
    Unbalanced(Money),
    #[error("{name} is not a bank account, card, cash or loan.")]
    NotPhysical { name: String },
    #[error("{name} has no Unassigned pot.")]
    NoUnassignedPot { name: String },
    #[error("The book has no {} account.", .0.name())]
    SystemAccountMissing(SystemAccount),
    #[error("Money cannot go from {name} to itself; pick another account or category.")]
    SameAccount { name: String },
    #[error("A pot move stays within one account; to move money to another account, make a transfer.")]
    PotMoveAcrossAccounts,
    #[error("{name} is not a pot.")]
    NotAPot { name: String },
    #[error("The amount must be more than zero.")]
    NotPositive,
    #[error("This transaction does not belong to an account, so it has no side to keep.")]
    NoSourceAccount,
    #[error("The parts add up to {got}, but the transaction is {expected}.")]
    SplitDoesNotAddUp { expected: Money, got: Money },
    #[error("A {} cannot sit there: {reason}.", .kind.label().to_lowercase())]
    WrongPlace { kind: AccountKind, reason: &'static str },
    #[error("A {} does not belong in {}.", .kind.label().to_lowercase(), .group.label().to_lowercase())]
    WrongGroup { kind: AccountKind, group: Group },
    #[error("{name} needs exactly one Unassigned pot.")]
    UnassignedPotCount { name: String },
    #[error("The amount is too large.")]
    TooLarge,
}

fn posting(account: AccountId, amount: Money) -> Posting {
    Posting::new(account, amount)
}

/// The rules every transaction keeps: at least two lines, no zero lines, leaves only, a sum of zero; a pot move stays
/// among the pots of one account.
pub fn check_postings(tree: &AccountTree, kind: TxnKind, postings: &[Posting]) -> Result<(), LedgerError> {
    if postings.len() < 2 {
        return Err(LedgerError::TooFewPostings);
    }
    for p in postings {
        let a = tree.account(p.account)?;
        if !tree.takes_postings(a.id) {
            return Err(LedgerError::NotALeaf { name: a.name.clone() });
        }
        if p.amount.is_zero() {
            return Err(LedgerError::ZeroPosting);
        }
        if !p.amount.is_sane() {
            return Err(LedgerError::TooLarge);
        }
    }
    let sum: Money = postings.iter().map(|p| p.amount).sum();
    if !sum.is_zero() {
        return Err(LedgerError::Unbalanced(sum));
    }
    if kind == TxnKind::PotMove {
        let first = postings.first().and_then(|p| tree.physical_of(p.account));
        for p in postings {
            let a = tree.account(p.account)?;
            if a.kind != AccountKind::Pot {
                return Err(LedgerError::NotAPot { name: a.name.clone() });
            }
            if tree.physical_of(a.id) != first {
                return Err(LedgerError::PotMoveAcrossAccounts);
            }
        }
    }
    Ok(())
}

/// A bank row of `physical` (amount signed for that account): the account's own side (its Unassigned pot, or the
/// account itself in Owe) against the counter account, or *Not sorted yet* when nobody has classified it. A counter
/// that is a Have account means its Unassigned pot (a transfer).
pub fn book_bank_row(tree: &AccountTree, physical: AccountId, amount: Money, counter: Option<AccountId>) -> Result<Vec<Posting>, LedgerError> {
    let own = tree.own_leaf(physical)?;
    let counter = counter_leaf(tree, physical, counter)?;
    let postings = vec![posting(own, amount), posting(counter, -amount)];
    check_postings(tree, TxnKind::Bank, &postings)?;
    Ok(postings)
}

fn counter_leaf(tree: &AccountTree, source: AccountId, counter: Option<AccountId>) -> Result<AccountId, LedgerError> {
    let leaf = match counter {
        Some(c) => tree.resolve_leaf(c)?,
        None => tree.system(SystemAccount::NotSortedYet)?,
    };
    if tree.physical_of(leaf) == Some(source) {
        return Err(LedgerError::SameAccount { name: tree.name(source) });
    }
    Ok(leaf)
}

/// A transaction's postings split into its own side (on the physical account it came from, any of its pots) and the
/// counter side (everything else).
pub fn own_and_counter<'a>(tree: &AccountTree, txn: &'a Txn) -> Result<(Vec<&'a Posting>, Vec<&'a Posting>), LedgerError> {
    let source = txn.account.ok_or(LedgerError::NoSourceAccount)?;
    Ok(txn.postings.iter().partition(|p| tree.physical_of(p.account) == Some(source)))
}

/// The counter account of a transaction with one counter posting, if it has exactly one.
pub fn counter_of(tree: &AccountTree, txn: &Txn) -> Option<AccountId> {
    let (_, counter) = own_and_counter(tree, txn).ok()?;
    match counter.as_slice() {
        [one] => Some(one.account),
        _ => None,
    }
}

/// The transaction with one new counter account in place of its counter posting(s); the own side stays (also when it
/// is on another pot than Unassigned).
pub fn reclassify(tree: &AccountTree, txn: &Txn, counter: AccountId) -> Result<Vec<Posting>, LedgerError> {
    let (own, _) = own_and_counter(tree, txn)?;
    let own_total: Money = own.iter().map(|p| p.amount).sum();
    split(tree, txn, &[posting(counter, -own_total)])
}

/// The transaction with these parts as its counter side (a split: mortgage interest + repayment). The parts must add
/// up to the opposite of the own side; a part on a Have account goes to its Unassigned pot.
pub fn split(tree: &AccountTree, txn: &Txn, parts: &[Posting]) -> Result<Vec<Posting>, LedgerError> {
    let source = txn.account.ok_or(LedgerError::NoSourceAccount)?;
    let (own, _) = own_and_counter(tree, txn)?;
    let expected = -own.iter().map(|p| p.amount).sum::<Money>();
    let got: Money = parts.iter().map(|p| p.amount).sum();
    if got != expected {
        return Err(LedgerError::SplitDoesNotAddUp { expected, got });
    }
    let mut postings: Vec<Posting> = own.into_iter().cloned().collect();
    for part in parts {
        let leaf = counter_leaf(tree, source, Some(part.account))?;
        postings.push(Posting { account: leaf, ..part.clone() });
    }
    check_postings(tree, txn.kind, &postings)?;
    Ok(postings)
}

/// The transaction with its own side moved to another pot of the same account (the bank sees nothing of pots).
pub fn move_own_side(tree: &AccountTree, txn: &Txn, pot: AccountId) -> Result<Vec<Posting>, LedgerError> {
    let source = txn.account.ok_or(LedgerError::NoSourceAccount)?;
    let a = tree.account(pot)?;
    if a.kind != AccountKind::Pot {
        return Err(LedgerError::NotAPot { name: a.name.clone() });
    }
    if tree.physical_of(pot) != Some(source) {
        return Err(LedgerError::PotMoveAcrossAccounts);
    }
    let (own, counter) = own_and_counter(tree, txn)?;
    let own_total: Money = own.iter().map(|p| p.amount).sum();
    let mut postings = vec![posting(pot, own_total)];
    postings.extend(counter.into_iter().cloned());
    check_postings(tree, txn.kind, &postings)?;
    Ok(postings)
}

/// Moves money between two pots of one account; the bank sees nothing.
pub fn pot_move(tree: &AccountTree, from: AccountId, to: AccountId, amount: Money) -> Result<Vec<Posting>, LedgerError> {
    for id in [from, to] {
        let a = tree.account(id)?;
        if a.kind != AccountKind::Pot {
            return Err(LedgerError::NotAPot { name: a.name.clone() });
        }
    }
    if from == to {
        return Err(LedgerError::SameAccount { name: tree.name(from) });
    }
    if tree.physical_of(from) != tree.physical_of(to) {
        return Err(LedgerError::PotMoveAcrossAccounts);
    }
    if !amount.is_positive() {
        return Err(LedgerError::NotPositive);
    }
    let postings = vec![posting(from, -amount), posting(to, amount)];
    check_postings(tree, TxnKind::PotMove, &postings)?;
    Ok(postings)
}

/// The leaf a balance of a physical account or a pot is booked on, and the sign that turns a shown balance into a
/// posting.
fn balance_leaf(tree: &AccountTree, account: AccountId) -> Result<(AccountId, i64), LedgerError> {
    let a = tree.account(account)?;
    if tree.physical_of(account).is_none() {
        return Err(LedgerError::NotPhysical { name: a.name.clone() });
    }
    Ok((tree.resolve_leaf(account)?, a.group.display_sign()))
}

/// The opening balance of a physical account (on its Unassigned pot) or a pot, against *Starting balance*. `shown` is
/// the balance as people see it: what you have, or what you owe, both positive.
pub fn opening_balance(tree: &AccountTree, account: AccountId, shown: Money) -> Result<Vec<Posting>, LedgerError> {
    let (leaf, sign) = balance_leaf(tree, account)?;
    let equity = tree.system(SystemAccount::StartingBalance)?;
    let amount = Money(shown.cents().saturating_mul(sign));
    let postings = vec![posting(leaf, amount), posting(equity, -amount)];
    check_postings(tree, TxnKind::Opening, &postings)?;
    Ok(postings)
}

/// A new estimate of something's value (a house), against *Value changes*: the difference between the current and the
/// new shown balance.
pub fn revaluation(tree: &AccountTree, account: AccountId, current_shown: Money, new_shown: Money) -> Result<Vec<Posting>, LedgerError> {
    let (leaf, sign) = balance_leaf(tree, account)?;
    let equity = tree.system(SystemAccount::ValueChanges)?;
    let amount = Money((new_shown - current_shown).cents().saturating_mul(sign));
    let postings = vec![posting(leaf, amount), posting(equity, -amount)];
    check_postings(tree, TxnKind::Adjustment, &postings)?;
    Ok(postings)
}

/// Whether a transaction only moves money between own accounts (and pots): every posting is in Have or Owe.
pub fn is_transfer(tree: &AccountTree, txn: &Txn) -> bool {
    txn.postings.iter().all(|p| tree.get(p.account).is_some_and(|a| matches!(a.group, Group::Have | Group::Owe)))
}

/// The existing transaction a new bank row of `account` is the second copy of: booked from another own account, with
/// postings on `account` (any of its pots) that add up to the row's amount, dated at most [`MAX_DAYS`] apart. The
/// closest date wins, then the lowest id. `candidates` are transactions that have no key on `account` yet.
pub fn find_transfer_match<'a>(tree: &AccountTree, account: AccountId, date: NaiveDate, amount: Money, candidates: &[&'a Txn]) -> Option<&'a Txn> {
    if amount.is_zero() {
        return None;
    }
    candidates
        .iter()
        .copied()
        .filter(|t| t.account.is_some_and(|s| s != account))
        .filter(|t| (t.date - date).num_days().abs() <= MAX_DAYS)
        .filter(|t| {
            let here: Vec<_> = t.postings.iter().filter(|p| tree.physical_of(p.account) == Some(account)).collect();
            !here.is_empty() && here.iter().map(|p| p.amount).sum::<Money>() == amount
        })
        .min_by_key(|t| ((t.date - date).num_days().abs(), t.id))
}

/// Posting sums per leaf account, in posting sign (positive = money into the account). A physical account's balance is
/// the sum of its pots; a group's is the sum of its leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Balances(BTreeMap<AccountId, Money>);

impl Balances {
    /// Balances at the end of `date` (postings on or before it).
    pub fn at<'a>(txns: impl IntoIterator<Item = &'a Txn>, date: NaiveDate) -> Balances {
        Self::of(txns.into_iter().filter(|t| t.date <= date))
    }

    /// What moved from `from` to `to`, both included.
    pub fn between<'a>(txns: impl IntoIterator<Item = &'a Txn>, from: NaiveDate, to: NaiveDate) -> Balances {
        Self::of(txns.into_iter().filter(|t| t.date >= from && t.date <= to))
    }

    /// The posting sums of these transactions.
    pub fn of<'a>(txns: impl IntoIterator<Item = &'a Txn>) -> Balances {
        let mut map: BTreeMap<AccountId, Money> = BTreeMap::new();
        for p in txns.into_iter().flat_map(|t| &t.postings) {
            *map.entry(p.account).or_default() += p.amount;
        }
        Balances(map)
    }

    /// The posting sum of one leaf.
    pub fn leaf(&self, id: AccountId) -> Money {
        self.0.get(&id).copied().unwrap_or_default()
    }

    /// The posting sum of an account and everything under it.
    pub fn total(&self, tree: &AccountTree, id: AccountId) -> Money {
        tree.leaves(id).into_iter().map(|l| self.leaf(l)).sum()
    }

    /// The balance as people see it: what you owe and what came in show positive.
    pub fn shown(&self, tree: &AccountTree, id: AccountId) -> Money {
        let sign = tree.get(id).map(|a| a.group.display_sign()).unwrap_or(1);
        Money(self.total(tree, id).cents().saturating_mul(sign))
    }

    /// Every leaf with a non-zero sum.
    pub fn iter(&self) -> impl Iterator<Item = (AccountId, Money)> + '_ {
        self.0.iter().filter(|(_, m)| !m.is_zero()).map(|(a, m)| (*a, *m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn books_a_bank_row_against_the_unassigned_pot() {
        let t = tree();
        assert_eq!(book_bank_row(&t, BETAAL, Money(-6418), Some(GROCERIES)).unwrap(), vec![p(BETAAL_FREE, -6418), p(GROCERIES, 6418)]);
        assert_eq!(book_bank_row(&t, BETAAL, Money(-4799), None).unwrap(), vec![p(BETAAL_FREE, -4799), p(NOT_SORTED, 4799)]);
        assert_eq!(book_bank_row(&t, CARD, Money(-1271), Some(GROCERIES)).unwrap(), vec![p(CARD, -1271), p(GROCERIES, 1271)]);
        assert_eq!(
            book_bank_row(&t, BETAAL, Money(-20000), Some(SPAAR)).unwrap(),
            vec![p(BETAAL_FREE, -20000), p(SPAAR_FREE, 20000)],
            "a transfer lands on the other account's Unassigned pot"
        );
        assert_eq!(book_bank_row(&t, BETAAL, Money(-100), Some(BILLS)), Err(LedgerError::SameAccount { name: "ING Betaalrekening".into() }));
        assert_eq!(book_bank_row(&t, GROCERIES, Money(-100), None), Err(LedgerError::NotPhysical { name: "Groceries".into() }));
        assert_eq!(book_bank_row(&t, BETAAL, Money(-100), Some(DAILY)), Err(LedgerError::NotALeaf { name: "Daily".into() }));
        assert_eq!(book_bank_row(&t, BETAAL, Money::ZERO, None), Err(LedgerError::ZeroPosting));
    }

    #[test]
    fn checks_postings() {
        let t = tree();
        assert_eq!(check_postings(&t, TxnKind::Manual, &[p(BETAAL_FREE, 5)]), Err(LedgerError::TooFewPostings));
        assert_eq!(check_postings(&t, TxnKind::Manual, &[p(BETAAL_FREE, 5), p(GROCERIES, -4)]), Err(LedgerError::Unbalanced(Money(1))));
        assert_eq!(
            check_postings(&t, TxnKind::Manual, &[p(BETAAL, 5), p(GROCERIES, -5)]),
            Err(LedgerError::NotALeaf { name: "ING Betaalrekening".into() })
        );
        assert_eq!(check_postings(&t, TxnKind::Manual, &[p(AccountId(99), 5), p(GROCERIES, -5)]), Err(LedgerError::AccountNotFound(AccountId(99))));
        assert_eq!(check_postings(&t, TxnKind::PotMove, &[p(BETAAL_FREE, -5), p(HOLIDAY, 5)]), Err(LedgerError::PotMoveAcrossAccounts));
        assert_eq!(
            check_postings(&t, TxnKind::PotMove, &[p(BETAAL_FREE, -5), p(GROCERIES, 5)]),
            Err(LedgerError::NotAPot { name: "Groceries".into() })
        );
        assert_eq!(check_postings(&t, TxnKind::Manual, &[p(BETAAL_FREE, i64::MAX), p(GROCERIES, -i64::MAX)]), Err(LedgerError::TooLarge));
    }

    #[test]
    fn reclassifies_and_splits_the_counter_side_only() {
        let t = tree();
        let bol = txn(1, d(2026, 9, 30), Some(BETAAL), "Bol.com", &[(BILLS, -4799), (NOT_SORTED, 4799)]);
        assert_eq!(reclassify(&t, &bol, GROCERIES).unwrap(), vec![p(BILLS, -4799), p(GROCERIES, 4799)], "pot kept");
        assert_eq!(counter_of(&t, &bol), Some(NOT_SORTED));

        let mortgage = txn(2, d(2026, 10, 2), Some(BETAAL), "ABN AMRO Hypotheken", &[(BETAAL_FREE, -124500), (NOT_SORTED, 124500)]);
        let parts = [p(INTEREST, 61230), p(MORTGAGE, 63270)];
        assert_eq!(split(&t, &mortgage, &parts).unwrap(), vec![p(BETAAL_FREE, -124500), p(INTEREST, 61230), p(MORTGAGE, 63270)]);
        assert_eq!(split(&t, &mortgage, &[p(INTEREST, 61230)]), Err(LedgerError::SplitDoesNotAddUp { expected: Money(124500), got: Money(61230) }));
        assert_eq!(reclassify(&t, &mortgage, BILLS), Err(LedgerError::SameAccount { name: "ING Betaalrekening".into() }));
        let manual = txn(3, d(2026, 10, 2), None, "", &[(WALLET_FREE, -500), (GROCERIES, 500)]);
        assert_eq!(reclassify(&t, &manual, GROCERIES), Err(LedgerError::NoSourceAccount));
        assert_eq!(move_own_side(&t, &bol, BETAAL_FREE).unwrap(), vec![p(BETAAL_FREE, -4799), p(NOT_SORTED, 4799)]);
        assert_eq!(move_own_side(&t, &bol, HOLIDAY), Err(LedgerError::PotMoveAcrossAccounts));
    }

    #[test]
    fn moves_money_between_pots_of_one_account() {
        let t = tree();
        assert_eq!(pot_move(&t, SPAAR_FREE, HOLIDAY, Money(25000)).unwrap(), vec![p(SPAAR_FREE, -25000), p(HOLIDAY, 25000)]);
        assert_eq!(pot_move(&t, BETAAL_FREE, HOLIDAY, Money(1)), Err(LedgerError::PotMoveAcrossAccounts));
        assert_eq!(pot_move(&t, SPAAR_FREE, HOLIDAY, Money(-1)), Err(LedgerError::NotPositive));
        assert_eq!(pot_move(&t, SPAAR_FREE, CARD, Money(1)), Err(LedgerError::NotAPot { name: "ABN AMRO Creditcard".into() }));
        assert!(matches!(pot_move(&t, HOLIDAY, HOLIDAY, Money(1)), Err(LedgerError::SameAccount { .. })));
    }

    #[test]
    fn opening_balances_and_revaluations_post_against_equity() {
        let t = tree();
        assert_eq!(opening_balance(&t, SPAAR, Money(1000)).unwrap(), vec![p(SPAAR_FREE, 1000), p(STARTING, -1000)]);
        assert_eq!(opening_balance(&t, HOLIDAY, Money(600)).unwrap(), vec![p(HOLIDAY, 600), p(STARTING, -600)]);
        assert_eq!(opening_balance(&t, MORTGAGE, Money(5000)).unwrap(), vec![p(MORTGAGE, -5000), p(STARTING, 5000)]);
        assert_eq!(opening_balance(&t, GROCERIES, Money(1)), Err(LedgerError::NotPhysical { name: "Groceries".into() }));
        assert_eq!(
            revaluation(&t, WALLET, Money(39_500_000), Money(41_000_000)).unwrap(),
            vec![p(WALLET_FREE, 1_500_000), p(VALUE_CHANGES, -1_500_000)]
        );
        assert_eq!(revaluation(&t, WALLET, Money(1), Money(1)), Err(LedgerError::ZeroPosting));
    }

    #[test]
    fn rolls_pots_up_into_their_account() {
        let t = tree();
        let txns = [
            txn(1, d(2026, 1, 1), None, "", &[(BETAAL_FREE, 100_000), (BILLS, 50_000), (STARTING, -150_000)]),
            txn(2, d(2026, 1, 5), Some(BETAAL), "AH", &[(BETAAL_FREE, -6418), (GROCERIES, 6418)]),
            txn(3, d(2026, 1, 9), Some(CARD), "Shop", &[(CARD, -2500), (GROCERIES, 2500)]),
        ];
        let at = Balances::at(&txns, d(2026, 1, 5));
        assert_eq!(at.total(&t, BETAAL), Money(143_582));
        assert_eq!(at.leaf(BILLS), Money(50_000));
        assert_eq!(at.shown(&t, CARD), Money::ZERO);
        let later = Balances::at(&txns, d(2026, 1, 31));
        assert_eq!(later.shown(&t, CARD), Money(2500), "owed shows positive");
        assert_eq!(later.shown(&t, DAILY), Money(8918));
        assert_eq!(Balances::between(&txns, d(2026, 1, 2), d(2026, 1, 9)).total(&t, GROCERIES), Money(8918));
        assert_eq!(later.iter().map(|(_, m)| m).sum::<Money>(), Money::ZERO);
    }

    #[test]
    fn matches_the_other_banks_copy_of_a_transfer() {
        let t = tree();
        let to_spaar = txn(1, d(2026, 10, 2), Some(BETAAL), "Naar Oranje Spaar", &[(BETAAL_FREE, -20000), (HOLIDAY, 20000)]);
        let groceries = txn(2, d(2026, 10, 2), Some(BETAAL), "AH", &[(BETAAL_FREE, -20000), (GROCERIES, 20000)]);
        let cands = [&groceries, &to_spaar];
        let hit = |acct, date, cents| find_transfer_match(&t, acct, date, Money(cents), &cands).map(|x| x.id.0);
        assert_eq!(hit(SPAAR, d(2026, 10, 3), 20000), Some(1), "the copy on Spaar, a day later, pot chosen");
        assert_eq!(hit(SPAAR, d(2026, 10, 6), 20000), None, "four days apart");
        assert_eq!(hit(SPAAR, d(2026, 10, 3), -20000), None, "not mirrored");
        assert_eq!(hit(BETAAL, d(2026, 10, 2), -20000), None, "booked from this account itself");
        assert_eq!(hit(CARD, d(2026, 10, 2), 20000), None, "no posting on the card");
    }

    #[test]
    fn prefers_the_closest_date() {
        let t = tree();
        let a = txn(5, d(2026, 9, 23), Some(BETAAL), "ICS", &[(BETAAL_FREE, -43215), (CARD, 43215)]);
        let b = txn(4, d(2026, 9, 25), Some(BETAAL), "ICS", &[(BETAAL_FREE, -43215), (CARD, 43215)]);
        let cands = [&a, &b];
        assert_eq!(find_transfer_match(&t, CARD, d(2026, 9, 24), Money(43215), &cands).map(|x| x.id.0), Some(4));
        assert_eq!(find_transfer_match(&t, CARD, d(2026, 9, 23), Money(43215), &cands).map(|x| x.id.0), Some(5));
    }
}
