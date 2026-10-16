//! The offline classifier (ADR 0004): own accounts → payee rules → memory → Review. Pure functions over the book.
//! From budget.rs `classify/pipeline.rs` and `classify/rules.rs`, without the model step.
//!
//! The answer is a counter account (ADR 0003): a category in `Goes`/`Comes`, or another own account for a transfer
//! (which always goes to Review).

use std::collections::{BTreeMap, HashMap};

use crate::book::Book;
use crate::ledger::own_and_counter;
use crate::payees;
use crate::tree::{AccountTree, normalize_iban};
use crate::types::*;

/// Memory answers only when at least this many similar confirmed transactions agree…
pub const MEMORY_MIN: usize = 2;
/// …for at least this share.
pub const MEMORY_AGREEMENT: f64 = 0.9;
/// How similar a past payee must be for memory.
pub const MEMORY_SIMILARITY: f64 = 0.9;
/// Two identical answers for one payee (and no different ones) make a rule.
pub const ANSWERS_FOR_RULE: u32 = 2;

/// The classifier's answer for one transaction. `counter` is set only when it was booked automatically (`Auto`);
/// otherwise `guesses` hold the best counter accounts for Review.
#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    pub counter: Option<AccountId>,
    pub status: TxnStatus,
    pub source: Option<ClassSource>,
    pub guesses: Vec<Guess>,
    pub transfer_likely: bool,
    /// For the detail pane: "Payee rule", "Similar past transactions", "The other account is one of yours".
    pub via: String,
    /// The rule that answered, to count its uses.
    pub rule: Option<RuleId>,
}

/// What the classifier looks at of one transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts<'a> {
    /// The physical account the bank row belongs to.
    pub account: Option<AccountId>,
    pub payee_norm: &'a str,
    pub counterparty_iban: Option<&'a str>,
}

impl<'a> From<&'a Txn> for Facts<'a> {
    fn from(t: &'a Txn) -> Self {
        Facts { account: t.account, payee_norm: &t.payee_norm, counterparty_iban: t.counterparty_iban.as_deref() }
    }
}

/// Everything the classifier knows about the household: the categories it may answer, the own accounts, the rules
/// and the confirmed history. Build once per batch.
pub struct Classifier {
    categories: Vec<AccountId>,
    own: Vec<(AccountId, String)>,
    rules: Vec<Rule>,
    /// Confirmed answers per payee, in first-seen order.
    by_payee: Vec<(String, Vec<AccountId>)>,
}

impl Classifier {
    pub fn new(book: &Book, tree: &AccountTree) -> Classifier {
        let categories = tree.categories().into_iter().filter(|(a, _)| !a.closed).map(|(a, _)| a.id).collect();
        let own =
            tree.all_physical().into_iter().filter(|a| !a.closed).filter_map(|a| a.iban.as_deref().map(|i| (a.id, normalize_iban(i)))).collect();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut by_payee: Vec<(String, Vec<AccountId>)> = Vec::new();
        for t in book.txns().filter(|t| t.status == TxnStatus::Reviewed && t.class_source == Some(ClassSource::User)) {
            let Some(counter) = counter_account(tree, t) else { continue };
            if tree.get(counter).is_none_or(|a| a.kind != AccountKind::Category) || t.payee_norm.is_empty() {
                continue;
            }
            let i = *index.entry(t.payee_norm.clone()).or_insert_with(|| {
                by_payee.push((t.payee_norm.clone(), Vec::new()));
                by_payee.len() - 1
            });
            if let Some((_, v)) = by_payee.get_mut(i) {
                v.push(counter);
            }
        }
        Classifier { categories, own, rules: book.rules().map(|r| (**r).clone()).collect(), by_payee }
    }

    fn is_category(&self, id: AccountId) -> bool {
        self.categories.contains(&id)
    }

    /// Past answers for payees at least `min` similar to `payee`.
    fn similar(&self, payee: &str, min: f64) -> Vec<AccountId> {
        if payee.is_empty() {
            return Vec::new();
        }
        self.by_payee.iter().filter(|(p, _)| payees::similarity(p, payee) >= min).flat_map(|(_, h)| h.iter().copied()).collect()
    }

    pub fn classify(&self, tx: &Facts) -> Classified {
        let review = |guesses: Vec<Guess>, via: &str| Classified {
            counter: None,
            status: TxnStatus::ToReview,
            source: None,
            guesses,
            transfer_likely: false,
            via: via.into(),
            rule: None,
        };
        if let Some(iban) = tx.counterparty_iban.map(normalize_iban)
            && let Some((own, _)) = self.own.iter().find(|(_, i)| *i == iban)
            && Some(*own) != tx.account
        {
            let mut r = review(vec![Guess { account: *own, probability: 1.0 }], "The other account is one of yours");
            r.transfer_likely = true;
            return r;
        }
        let iban = tx.counterparty_iban.map(normalize_iban);
        if let Some(rule) = self.rules.iter().find(|r| !r.mixed && r.matches(tx.payee_norm, iban.as_deref()) && self.is_category(r.account)) {
            return Classified {
                counter: Some(rule.account),
                status: TxnStatus::Auto,
                source: Some(ClassSource::Rule),
                guesses: vec![Guess { account: rule.account, probability: 1.0 }],
                transfer_likely: false,
                via: if rule.origin == RuleOrigin::Manual { "Your payee rule".into() } else { "Learned payee rule".into() },
                rule: Some(rule.id),
            };
        }
        let similar = self.similar(tx.payee_norm, MEMORY_SIMILARITY);
        let guesses: Vec<Guess> = votes(&similar).into_iter().filter(|g| self.is_category(g.account)).collect();
        if similar.len() >= MEMORY_MIN
            && let Some(top) = guesses.first()
            && top.probability >= MEMORY_AGREEMENT
        {
            return Classified {
                counter: Some(top.account),
                status: TxnStatus::Auto,
                source: Some(ClassSource::Memory),
                guesses: guesses.into_iter().take(2).collect(),
                transfer_likely: false,
                via: "Similar past transactions".into(),
                rule: None,
            };
        }
        let loose: Vec<Guess> = votes(&self.similar(tx.payee_norm, 0.5)).into_iter().filter(|g| self.is_category(g.account)).take(2).collect();
        let via = if loose.is_empty() { "New payee" } else { "Earlier answers for similar payees" };
        review(loose, via)
    }
}

/// Share of each counter account in `history`, most votes first (ties: first seen first).
fn votes(history: &[AccountId]) -> Vec<Guess> {
    let mut counts: Vec<(AccountId, usize)> = Vec::new();
    for c in history {
        match counts.iter_mut().find(|(a, _)| a == c) {
            Some((_, n)) => *n += 1,
            None => counts.push((*c, 1)),
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.into_iter().map(|(account, n)| Guess { account, probability: n as f64 / history.len().max(1) as f64 }).collect()
}

/// The one counter account of a bank transaction (a category, or the physical account on the other side of a
/// transfer); `None` for splits.
pub fn counter_account(tree: &AccountTree, t: &Txn) -> Option<AccountId> {
    let (_, counter) = own_and_counter(tree, t).ok()?;
    match counter.as_slice() {
        [one] => Some(tree.physical_of(one.account).unwrap_or(one.account)),
        _ => None,
    }
}

/// What to do with the rules after a person answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleChange {
    Create { payee_prefix: String, account: AccountId },
    MarkMixed(RuleId),
    None,
}

/// Learning from answers: `answers` counts the person's answers per counter account for `payee_norm`; `existing` is
/// the rule for that payee, if any. Two identical answers make a rule; a later different answer on a learned rule
/// marks it mixed (Bol.com: sometimes Kids, sometimes Gifts).
pub fn after_answer(payee_norm: &str, answers: &BTreeMap<AccountId, u32>, existing: Option<&Rule>) -> RuleChange {
    if payee_norm.trim().is_empty() {
        return RuleChange::None;
    }
    if let Some(rule) = existing {
        let others: u32 = answers.iter().filter(|(a, _)| **a != rule.account).map(|(_, n)| n).sum();
        return if rule.origin == RuleOrigin::Learned && !rule.mixed && others > 0 { RuleChange::MarkMixed(rule.id) } else { RuleChange::None };
    }
    match answers.iter().collect::<Vec<_>>().as_slice() {
        [(account, count)] if **count >= ANSWERS_FOR_RULE => RuleChange::Create { payee_prefix: payee_norm.to_string(), account: **account },
        _ => RuleChange::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn answered(id: u64, payee: &str, to: AccountId) -> Txn {
        Txn { class_source: Some(ClassSource::User), ..txn(id, d(2026, 9, 1), Some(BETAAL), payee, &[(BETAAL_FREE, -1000), (to, 1000)]) }
    }

    #[test]
    fn own_iban_means_a_likely_transfer() {
        let b = book(vec![]);
        let c = Classifier::new(&b, &b.tree());
        let r = c.classify(&Facts { account: Some(BETAAL), payee_norm: "ORANJE SPAAR", counterparty_iban: Some("nl34 ingb 0007 6543 21") });
        assert!(r.transfer_likely);
        assert_eq!(r.status, TxnStatus::ToReview);
        assert_eq!(r.guesses.first().map(|g| g.account), Some(SPAAR));
        let own = c.classify(&Facts { account: Some(SPAAR), payee_norm: "X", counterparty_iban: Some("NL34INGB0007654321") });
        assert!(!own.transfer_likely, "its own IBAN is not a transfer");
    }

    #[test]
    fn rules_then_memory_then_review() {
        let mut b = book(vec![
            answered(1, "JUMBO", GROCERIES),
            answered(2, "JUMBO", GROCERIES),
            answered(3, "BOL.COM", GROCERIES),
            answered(4, "BOL.COM", INTEREST),
        ]);
        b.rules.insert(
            RuleId(1),
            std::sync::Arc::new(Rule {
                id: RuleId(1),
                payee_prefix: Some("ALBERT HEIJN".into()),
                iban: None,
                account: GROCERIES,
                origin: RuleOrigin::Manual,
                mixed: false,
                uses: 0,
            }),
        );
        let c = Classifier::new(&b, &b.tree());
        let f = |p| Facts { account: Some(BETAAL), payee_norm: p, counterparty_iban: None };
        let rule = c.classify(&f("ALBERT HEIJN UTRECHT"));
        assert_eq!((rule.counter, rule.source, rule.rule), (Some(GROCERIES), Some(ClassSource::Rule), Some(RuleId(1))));
        assert_eq!(c.classify(&f("ALBERT HEIJNX")).counter, None, "a prefix ends at a word");
        let memory = c.classify(&f("JUMBO"));
        assert_eq!((memory.counter, memory.source, memory.status), (Some(GROCERIES), Some(ClassSource::Memory), TxnStatus::Auto));
        let mixed = c.classify(&f("BOL.COM"));
        assert_eq!(mixed.status, TxnStatus::ToReview, "the past disagrees");
        assert_eq!(mixed.guesses.len(), 2);
        let new = c.classify(&f("SOMETHING NEW"));
        assert!(new.guesses.is_empty());
        assert_eq!(new.via, "New payee");
    }

    #[test]
    fn learns_rules_from_answers() {
        let one = BTreeMap::from([(GROCERIES, 1)]);
        let two = BTreeMap::from([(GROCERIES, 2)]);
        let split = BTreeMap::from([(GROCERIES, 2), (INTEREST, 1)]);
        assert_eq!(after_answer("JUMBO", &one, None), RuleChange::None);
        assert_eq!(after_answer("JUMBO", &two, None), RuleChange::Create { payee_prefix: "JUMBO".into(), account: GROCERIES });
        assert_eq!(after_answer("JUMBO", &split, None), RuleChange::None);
        assert_eq!(after_answer("", &two, None), RuleChange::None);
        let learned = Rule {
            id: RuleId(7),
            payee_prefix: Some("JUMBO".into()),
            iban: None,
            account: GROCERIES,
            origin: RuleOrigin::Learned,
            mixed: false,
            uses: 3,
        };
        assert_eq!(after_answer("JUMBO", &split, Some(&learned)), RuleChange::MarkMixed(RuleId(7)));
        let manual = Rule { origin: RuleOrigin::Manual, ..learned };
        assert_eq!(after_answer("JUMBO", &split, Some(&manual)), RuleChange::None, "people's own rules stay");
    }
}
