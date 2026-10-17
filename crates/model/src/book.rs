use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::ledger::{self, Balances};
use crate::money::Money;
use crate::statement::CsvProfile;
use crate::tree::{AccountTree, normalize_iban};
use crate::types::*;
use crate::{ModelError, Result, invalid, limits};

/// Everything about a book except its accounts, transactions and rules.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookMeta {
    pub name: String,
    /// A made-up book to try things out (the sample household). The apps say so on every screen.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub demo: bool,
    /// The numbers the next new account, transaction, rule and import get. Numbers are never reused.
    #[serde(default = "one")]
    pub next_account: u64,
    #[serde(default = "one")]
    pub next_txn: u64,
    #[serde(default = "one")]
    pub next_rule: u64,
    #[serde(default = "one")]
    pub next_import: u64,
    /// CSV layouts the person mapped, next to the built-in ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<CsvProfile>,
    /// Balances the bank stated in imported files, for the bank check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<StatedBalance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<ImportRecord>,
    /// Items in Needs attention the person answered ("Seen", "Not mine"), by their key
    /// (`reports::attention`). An older app may drop them; the items then show again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dismissed: Vec<String>,
}

fn one() -> u64 {
    1
}

/// A household's books. Cheap to clone: records are shared (`Arc`), so a clone is a snapshot for undo and for working
/// out what changed since the last save ([`diff`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Book {
    pub meta: BookMeta,
    pub(crate) accounts: BTreeMap<AccountId, Arc<Account>>,
    pub(crate) txns: BTreeMap<TxnId, Arc<Txn>>,
    pub(crate) rules: BTreeMap<RuleId, Arc<Rule>>,
}

/// One change to a book, as a store receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Meta(BookMeta),
    Account(Arc<Account>),
    DeleteAccount(AccountId),
    Txn(Arc<Txn>),
    DeleteTxn(TxnId),
    Rule(Arc<Rule>),
    DeleteRule(RuleId),
}

fn diff_map<K: Ord + Copy, V: PartialEq>(
    old: &BTreeMap<K, Arc<V>>,
    new: &BTreeMap<K, Arc<V>>,
    up: fn(Arc<V>) -> Change,
    del: fn(K) -> Change,
    out: &mut Vec<Change>,
) {
    for (id, rec) in new {
        match old.get(id) {
            Some(prev) if Arc::ptr_eq(prev, rec) || prev == rec => {}
            _ => out.push(up(rec.clone())),
        }
    }
    for id in old.keys() {
        if !new.contains_key(id) {
            out.push(del(*id));
        }
    }
}

/// What changed from `old` to `new`, record by record. Unchanged records share their `Arc`, so this is a pointer
/// comparison for most of them.
pub fn diff(old: &Book, new: &Book) -> Vec<Change> {
    let mut out = Vec::new();
    if old.meta != new.meta {
        out.push(Change::Meta(new.meta.clone()));
    }
    diff_map(&old.accounts, &new.accounts, Change::Account, Change::DeleteAccount, &mut out);
    diff_map(&old.txns, &new.txns, Change::Txn, Change::DeleteTxn, &mut out);
    diff_map(&old.rules, &new.rules, Change::Rule, Change::DeleteRule, &mut out);
    out
}

impl Book {
    /// An empty book with only the system accounts.
    pub fn new(name: &str) -> Result<Book> {
        let meta = BookMeta { name: clean_book_name(name)?, next_account: 1, next_txn: 1, next_rule: 1, next_import: 1, ..BookMeta::default() };
        let mut b = Book { meta, accounts: BTreeMap::new(), txns: BTreeMap::new(), rules: BTreeMap::new() };
        for key in SystemAccount::ALL {
            b.insert_system(key);
        }
        Ok(b)
    }

    /// A new book with the system accounts and the usual Dutch household categories ([`crate::catalogue`]).
    pub fn with_starter_categories(name: &str) -> Result<Book> {
        let mut b = Book::new(name)?;
        crate::catalogue::add_starter_categories(&mut b)?;
        Ok(b)
    }

    fn insert_system(&mut self, key: SystemAccount) -> AccountId {
        let id = self.take_account_id();
        let a = Account {
            id,
            group: Group::Equity,
            kind: AccountKind::System,
            parent: None,
            name: key.name().into(),
            description: String::new(),
            iban: None,
            unassigned: false,
            system: Some(key),
            goal: None,
            goal_date: None,
            sort: 0,
            closed: false,
        };
        self.accounts.insert(id, Arc::new(a));
        id
    }

    pub(crate) fn take_account_id(&mut self) -> AccountId {
        let id = AccountId(self.meta.next_account.max(1));
        self.meta.next_account = id.0.saturating_add(1);
        id
    }

    pub(crate) fn take_txn_id(&mut self) -> TxnId {
        let id = TxnId(self.meta.next_txn.max(1));
        self.meta.next_txn = id.0.saturating_add(1);
        id
    }

    pub(crate) fn take_rule_id(&mut self) -> RuleId {
        let id = RuleId(self.meta.next_rule.max(1));
        self.meta.next_rule = id.0.saturating_add(1);
        id
    }

    pub(crate) fn take_import_id(&mut self) -> ImportId {
        let id = ImportId(self.meta.next_import.max(1));
        self.meta.next_import = id.0.saturating_add(1);
        id
    }

    /// Builds a book from untrusted parts (a file) and repairs whatever breaks the model's rules: duplicate ids,
    /// accounts in the wrong place, Have accounts without exactly one Unassigned pot, postings on accounts that can't
    /// take them, unbalanced transactions, dangling references, oversized text, counters behind the data. Never fails;
    /// returns the book and one line per repair.
    pub fn from_parts(meta: BookMeta, accounts: Vec<Account>, txns: Vec<Txn>, rules: Vec<Rule>) -> (Book, Vec<String>) {
        let mut notes = Vec::new();
        let name = clean_book_name(&meta.name).unwrap_or_else(|_| {
            notes.push("the book's name was invalid; reset".to_string());
            "Household".into()
        });
        let mut b = Book {
            meta: BookMeta { name, profiles: Vec::new(), balances: Vec::new(), imports: Vec::new(), dismissed: Vec::new(), ..meta.clone() },
            accounts: BTreeMap::new(),
            txns: BTreeMap::new(),
            rules: BTreeMap::new(),
        };

        // Accounts: unique ids, clean text, one per system key.
        let mut ibans: BTreeSet<String> = BTreeSet::new();
        let mut system_seen: BTreeSet<SystemAccount> = BTreeSet::new();
        for mut a in accounts {
            if b.accounts.len() >= limits::ACCOUNTS {
                notes.push(format!("more than {} accounts; the rest were dropped", limits::ACCOUNTS));
                break;
            }
            if a.id.0 == 0 || b.accounts.contains_key(&a.id) {
                notes.push(format!("duplicate or zero account number {} dropped", a.id.0));
                continue;
            }
            a.name = truncate(a.name.split_whitespace().collect::<Vec<_>>().join(" ").as_str(), limits::NAME);
            if a.name.is_empty() {
                a.name = format!("Account {}", a.id.0);
            }
            a.description = truncate(&a.description, limits::DESCRIPTION);
            a.iban = a.iban.as_deref().map(normalize_iban).filter(|i| !i.is_empty() && i.len() <= limits::IBAN);
            if let Some(i) = &a.iban
                && !ibans.insert(i.clone())
            {
                notes.push(format!("account {} had an IBAN another account has; cleared", a.id.0));
                a.iban = None;
            }
            if let Some(k) = a.system
                && !system_seen.insert(k)
            {
                notes.push(format!("account {} was a second {} account; it is a plain one now", a.id.0, k.name()));
                a.system = None;
            }
            if a.goal.is_some_and(|g| !g.is_positive() || !g.is_sane()) {
                a.goal = None;
            }
            b.accounts.insert(a.id, Arc::new(a));
        }
        let max_account = b.accounts.keys().map(|id| id.0).max().unwrap_or(0);
        b.meta.next_account = meta.next_account.max(max_account.saturating_add(1)).max(1);
        for key in SystemAccount::ALL {
            if !system_seen.contains(&key) {
                notes.push(format!("the {} account was missing; added", key.name()));
                b.insert_system(key);
            }
        }
        b.repair_placement(&mut notes);

        // Transactions: unique ids, clean text, postings on accounts that take them, balanced.
        let tree = b.tree();
        let not_sorted = tree.system(SystemAccount::NotSortedYet).ok();
        let max_txn = txns.iter().map(|t| t.id.0).max().unwrap_or(0);
        let mut seen_txn: BTreeSet<TxnId> = BTreeSet::new();
        for mut t in txns {
            if b.txns.len() >= limits::TXNS {
                notes.push(format!("more than {} transactions; the rest were dropped", limits::TXNS));
                break;
            }
            if t.id.0 == 0 || !seen_txn.insert(t.id) {
                notes.push(format!("duplicate or zero transaction number {} dropped", t.id.0));
                continue;
            }
            t.payee = truncate(t.payee.trim(), limits::PAYEE);
            t.payee_norm = truncate(t.payee_norm.trim(), limits::PAYEE);
            t.description = truncate(&t.description, limits::TEXT);
            t.note = truncate(&t.note, limits::TEXT);
            t.via = truncate(&t.via, limits::NAME);
            t.who = t.who.as_deref().map(|w| truncate(w.trim(), limits::NAME)).filter(|w| !w.is_empty());
            t.counterparty_iban = t.counterparty_iban.as_deref().map(normalize_iban).filter(|i| !i.is_empty() && i.len() <= limits::IBAN);
            if t.account.is_some_and(|a| !tree.get(a).is_some_and(|x| x.kind.is_physical())) {
                notes.push(format!("transaction {} pointed at an account that is not a bank account; cleared", t.id.0));
                t.account = None;
            }
            if t.bank_amount.is_some_and(|m| !m.is_sane()) || t.account.is_none() {
                t.bank_amount = t.bank_amount.filter(|m| m.is_sane() && t.account.is_some());
            }
            t.guesses.retain(|g| tree.get(g.account).is_some() && g.probability.is_finite());
            t.guesses.truncate(5);
            for g in &mut t.guesses {
                g.probability = g.probability.clamp(0.0, 1.0);
            }
            let mut keys_seen = BTreeSet::new();
            t.keys.retain(|k| tree.get(k.account).is_some() && k.key.len() <= limits::KEY && keys_seen.insert(k.clone()));
            if t.duplicate_of.is_some_and(|d| d == t.id || d.0 > max_txn || d.0 == 0) {
                t.duplicate_of = None;
            }
            if let Some(n) = repair_postings(&tree, not_sorted, &mut t) {
                notes.push(n);
            }
            if t.postings.len() < 2 {
                notes.push(format!("transaction {} had no money in it; dropped", t.id.0));
                continue;
            }
            b.txns.insert(t.id, Arc::new(t));
        }
        // Duplicate links must point at transactions that survived.
        let ids: BTreeSet<TxnId> = b.txns.keys().copied().collect();
        for t in b.txns.values_mut() {
            if t.duplicate_of.is_some_and(|d| !ids.contains(&d)) {
                Arc::make_mut(t).duplicate_of = None;
            }
        }
        let max_txn = b.txns.keys().map(|id| id.0).max().unwrap_or(0);
        b.meta.next_txn = meta.next_txn.max(max_txn.saturating_add(1)).max(1);

        // Rules: unique ids, something to match, an answer that takes postings.
        for mut r in rules {
            if b.rules.len() >= limits::RULES {
                notes.push(format!("more than {} rules; the rest were dropped", limits::RULES));
                break;
            }
            if r.id.0 == 0 || b.rules.contains_key(&r.id) {
                notes.push(format!("duplicate or zero rule number {} dropped", r.id.0));
                continue;
            }
            r.payee_prefix = r.payee_prefix.as_deref().map(|p| truncate(p.trim(), limits::PAYEE)).filter(|p| !p.is_empty());
            r.iban = r.iban.as_deref().map(normalize_iban).filter(|i| !i.is_empty() && i.len() <= limits::IBAN);
            if r.payee_prefix.is_none() && r.iban.is_none() {
                notes.push(format!("rule {} matched nothing; dropped", r.id.0));
                continue;
            }
            if tree.resolve_leaf(r.account).is_err() {
                notes.push(format!("rule {} answered with an account that can't take money; dropped", r.id.0));
                continue;
            }
            b.rules.insert(r.id, Arc::new(r));
        }
        let max_rule = b.rules.keys().map(|id| id.0).max().unwrap_or(0);
        b.meta.next_rule = meta.next_rule.max(max_rule.saturating_add(1)).max(1);

        // Meta lists.
        let mut names = BTreeSet::new();
        for p in meta.profiles.into_iter().take(limits::PROFILES) {
            if !p.name.trim().is_empty() && p.name.len() <= limits::NAME && names.insert(p.name.to_lowercase()) {
                b.meta.profiles.push(p);
            }
        }
        b.meta.balances = meta
            .balances
            .into_iter()
            .filter(|s| tree.get(s.account).is_some_and(|a| a.kind.is_physical()) && s.amount.is_sane())
            .take(limits::BALANCES)
            .collect();
        b.meta.imports = meta.imports.into_iter().take(limits::IMPORTS).collect();
        for i in &mut b.meta.imports {
            i.file = truncate(&i.file, limits::NAME * 2);
            i.format = truncate(&i.format, limits::NAME);
        }
        let max_import = b.meta.imports.iter().map(|i| i.id.0).max().unwrap_or(0);
        b.meta.next_import = meta.next_import.max(max_import.saturating_add(1)).max(1);
        let mut seen = BTreeSet::new();
        b.meta.dismissed = meta
            .dismissed
            .into_iter()
            .filter(|k| !k.is_empty() && k.len() <= limits::DISMISSED_KEY && seen.insert(k.clone()))
            .take(limits::DISMISSED)
            .collect();
        (b, notes)
    }

    /// Puts every account where the tree allows it (ADR 0003): physical accounts at the root of their group, pots under
    /// a Have account, categories under a category group of the same group, one Unassigned pot per Have account.
    fn repair_placement(&mut self, notes: &mut Vec<String>) {
        let snapshot: BTreeMap<AccountId, Arc<Account>> = self.accounts.clone();
        let parent_ok = |p: Option<AccountId>, f: &dyn Fn(&Account) -> bool| p.and_then(|p| snapshot.get(&p)).is_some_and(|pa| f(pa));
        let mut fixes: Vec<(AccountId, Account, String)> = Vec::new();
        for a in snapshot.values() {
            let mut n = Account::clone(a);
            let mut why = None;
            if n.system.is_some() || n.kind == AccountKind::System {
                n.kind = AccountKind::System;
                n.group = Group::Equity;
                n.parent = None;
            } else if let Some(g) = n.kind.physical_group() {
                n.group = g;
                n.parent = None;
            } else if n.kind == AccountKind::Pot {
                n.group = Group::Have;
                if !parent_ok(n.parent, &|p| p.group == Group::Have && p.kind.is_physical() && p.system.is_none()) {
                    n.kind = AccountKind::Current;
                    n.parent = None;
                    n.unassigned = false;
                    why = Some("a pot without its account; it is an account of its own now");
                }
            } else if n.kind == AccountKind::CategoryGroup {
                if !matches!(n.group, Group::Goes | Group::Comes) {
                    n.group = Group::Goes;
                }
                n.parent = None;
            } else if n.kind == AccountKind::Category {
                if !matches!(n.group, Group::Goes | Group::Comes) {
                    n.group = Group::Goes;
                }
                let group = n.group;
                if !parent_ok(n.parent, &|p| p.kind == AccountKind::CategoryGroup && p.group == group) {
                    n.parent = None; // attached to an "Other" group below
                    why = Some("a category outside a category group; moved to Other");
                }
            }
            if n.unassigned && n.kind != AccountKind::Pot {
                n.unassigned = false;
            }
            if n != **a {
                fixes.push((a.id, n, why.map(str::to_string).unwrap_or_else(|| "an account in the wrong place; moved".into())));
            }
        }
        for (id, n, why) in fixes {
            notes.push(format!("account {}: {why}", id.0));
            self.accounts.insert(id, Arc::new(n));
        }
        // Categories without a group go under "Other" in their group.
        for group in [Group::Goes, Group::Comes] {
            let homeless: Vec<AccountId> =
                self.accounts.values().filter(|a| a.kind == AccountKind::Category && a.group == group && a.parent.is_none()).map(|a| a.id).collect();
            if homeless.is_empty() {
                continue;
            }
            let other = match self.accounts.values().find(|a| a.kind == AccountKind::CategoryGroup && a.group == group && a.name == "Other") {
                Some(o) => o.id,
                None => {
                    let id = self.take_account_id();
                    let o = Account { id, group, kind: AccountKind::CategoryGroup, parent: None, name: "Other".into(), ..blank_account() };
                    self.accounts.insert(id, Arc::new(o));
                    id
                }
            };
            for h in homeless {
                if let Some(a) = self.accounts.get_mut(&h) {
                    Arc::make_mut(a).parent = Some(other);
                }
            }
        }
        // Exactly one Unassigned pot per Have account.
        let physical: Vec<AccountId> = self.accounts.values().filter(|a| a.group == Group::Have && a.kind.is_physical()).map(|a| a.id).collect();
        for p in physical {
            let pots: Vec<AccountId> = self.accounts.values().filter(|a| a.parent == Some(p) && a.unassigned).map(|a| a.id).collect();
            match pots.split_first() {
                None => {
                    notes.push(format!("account {} had no Unassigned pot; added", p.0));
                    self.add_unassigned_pot(p);
                }
                Some((_, extra)) => {
                    for e in extra {
                        notes.push(format!("account {} had a second Unassigned pot; it is a plain pot now", e.0));
                        if let Some(a) = self.accounts.get_mut(e) {
                            Arc::make_mut(a).unassigned = false;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn add_unassigned_pot(&mut self, physical: AccountId) -> AccountId {
        let id = self.take_account_id();
        let pot = Account {
            id,
            group: Group::Have,
            kind: AccountKind::Pot,
            parent: Some(physical),
            name: "Unassigned".into(),
            unassigned: true,
            ..blank_account()
        };
        self.accounts.insert(id, Arc::new(pot));
        id
    }

    // ------------------------------------------------------------------ reading

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// The account tree. Built on each call; accounts are shared, so it is cheap.
    pub fn tree(&self) -> AccountTree {
        AccountTree::new(self.accounts.values().cloned())
    }

    pub fn accounts(&self) -> impl Iterator<Item = &Arc<Account>> {
        self.accounts.values()
    }

    pub fn account(&self, id: AccountId) -> Option<&Account> {
        self.accounts.get(&id).map(|a| a.as_ref())
    }

    pub fn txns(&self) -> impl DoubleEndedIterator<Item = &Arc<Txn>> {
        self.txns.values()
    }

    pub fn txn(&self, id: TxnId) -> Option<&Txn> {
        self.txns.get(&id).map(|t| t.as_ref())
    }

    pub fn txn_count(&self) -> usize {
        self.txns.len()
    }

    pub fn rules(&self) -> impl Iterator<Item = &Arc<Rule>> {
        self.rules.values()
    }

    pub fn rule(&self, id: RuleId) -> Option<&Rule> {
        self.rules.get(&id).map(|r| r.as_ref())
    }

    /// The system account for `key` (every repaired book has one).
    pub fn system(&self, key: SystemAccount) -> Result<AccountId> {
        self.accounts.values().find(|a| a.system == Some(key)).map(|a| a.id).ok_or_else(|| ModelError::NotFound(key.name().into()))
    }

    /// Balances at the end of `date`.
    pub fn balances_at(&self, date: NaiveDate) -> Balances {
        Balances::at(self.txns.values().map(|t| t.as_ref()), date)
    }

    /// Balances over all transactions.
    pub fn balances(&self) -> Balances {
        Balances::of(self.txns.values().map(|t| t.as_ref()))
    }

    /// The first and last transaction dates, if there are any.
    pub fn date_range(&self) -> Option<(NaiveDate, NaiveDate)> {
        let first = self.txns.values().map(|t| t.date).min()?;
        let last = self.txns.values().map(|t| t.date).max()?;
        Some((first, last))
    }

    /// Finds an account by number (`12`, `#12`), IBAN, path (`ING / Holiday`) or name (any case). A name that several
    /// accounts share prefers an open account outside Equity; an Unassigned pot needs its path.
    pub fn resolve_account(&self, s: &str) -> Option<AccountId> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let num = s.strip_prefix('#').unwrap_or(s);
        if let Ok(n) = num.parse::<u64>() {
            return self.accounts.contains_key(&AccountId(n)).then_some(AccountId(n));
        }
        if let Some(a) = self.accounts.values().find(|a| a.iban.as_deref().is_some_and(|i| i == normalize_iban(s))) {
            return Some(a.id);
        }
        if let Some((parent, child)) = s.split_once('/') {
            let parent = self.resolve_account(parent)?;
            return self.accounts.values().find(|a| a.parent == Some(parent) && a.name.eq_ignore_ascii_case(child.trim())).map(|a| a.id);
        }
        let mut hits: Vec<&Arc<Account>> = self.accounts.values().filter(|a| a.name.eq_ignore_ascii_case(s)).collect();
        hits.sort_by_key(|a| (a.closed, a.group == Group::Equity, a.unassigned, a.id));
        hits.first().map(|a| a.id)
    }

    /// `ING Betaalrekening`, `Oranje Spaar / Holiday` for a pot, `Daily / Groceries` for a category only when the name
    /// is not unique.
    pub fn account_label(&self, id: AccountId) -> String {
        let Some(a) = self.account(id) else { return format!("#{id}") };
        match a.parent.and_then(|p| self.account(p)) {
            Some(p) if a.kind == AccountKind::Pot => format!("{} / {}", p.name, a.name),
            _ => a.name.clone(),
        }
    }

    // ------------------------------------------------------------------ store support

    /// Replays changes a store already validated. Does not check the model's rules; run the result through
    /// [`Book::from_parts`] if unsure.
    pub fn apply(&mut self, changes: &[Change]) {
        for c in changes {
            match c {
                Change::Meta(m) => self.meta = m.clone(),
                Change::Account(a) => {
                    self.accounts.insert(a.id, a.clone());
                }
                Change::DeleteAccount(id) => {
                    self.accounts.remove(id);
                }
                Change::Txn(t) => {
                    self.txns.insert(t.id, t.clone());
                }
                Change::DeleteTxn(id) => {
                    self.txns.remove(id);
                }
                Change::Rule(r) => {
                    self.rules.insert(r.id, r.clone());
                }
                Change::DeleteRule(id) => {
                    self.rules.remove(id);
                }
            }
        }
    }

    /// The parts, for a store to write and for `from_parts` round trips.
    pub fn parts(&self) -> (&BookMeta, Vec<&Account>, Vec<&Txn>, Vec<&Rule>) {
        (
            &self.meta,
            self.accounts.values().map(|a| a.as_ref()).collect(),
            self.txns.values().map(|t| t.as_ref()).collect(),
            self.rules.values().map(|r| r.as_ref()).collect(),
        )
    }

    /// Checks the rules a stored book must keep; tests run it after every command.
    pub fn validate(&self) -> std::result::Result<(), String> {
        let tree = self.tree();
        tree.check().map_err(|e| e.to_string())?;
        for t in self.txns.values() {
            ledger::check_postings(&tree, if t.kind == TxnKind::PotMove { TxnKind::PotMove } else { TxnKind::Manual }, &t.postings)
                .map_err(|e| format!("transaction {}: {e}", t.id))?;
        }
        Ok(())
    }
}

/// Moves postings to accounts that take them and balances the transaction against *Not sorted yet*. Returns a note
/// when it changed something.
fn repair_postings(tree: &AccountTree, not_sorted: Option<AccountId>, t: &mut Txn) -> Option<String> {
    let before = t.postings.clone();
    t.postings.truncate(limits::POSTINGS);
    t.postings.retain(|p| !p.amount.is_zero() && p.amount.is_sane());
    for p in &mut t.postings {
        p.memo = truncate(&p.memo, limits::NAME);
        if !tree.takes_postings(p.account) {
            let to = tree
                .get(p.account)
                .filter(|a| a.group == Group::Have && a.kind.is_physical())
                .and_then(|a| tree.unassigned_pot(a.id))
                .map(|u| u.id)
                .or(not_sorted);
            if let Some(to) = to {
                p.account = to;
            }
        }
    }
    let sum: Money = t.postings.iter().map(|p| p.amount).sum();
    if !sum.is_zero()
        && let Some(ns) = not_sorted
    {
        t.postings.push(Posting::new(ns, -sum));
    }
    if t.kind == TxnKind::PotMove && ledger::check_postings(tree, TxnKind::PotMove, &t.postings).is_err() {
        t.kind = TxnKind::Manual;
    }
    (t.postings != before).then(|| format!("transaction {} had postings that did not add up or sat on the wrong accounts; repaired", t.id.0))
}

pub(crate) fn blank_account() -> Account {
    Account {
        id: AccountId(0),
        group: Group::Goes,
        kind: AccountKind::Category,
        parent: None,
        name: String::new(),
        description: String::new(),
        iban: None,
        unassigned: false,
        system: None,
        goal: None,
        goal_date: None,
        sort: 0,
        closed: false,
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub(crate) fn clean_book_name(s: &str) -> Result<String> {
    let n = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err(invalid("the book needs a name"));
    }
    if n.chars().count() > limits::NAME {
        return Err(invalid(format!("the book's name is at most {} characters", limits::NAME)));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn new_books_have_system_accounts() {
        let b = Book::new("Home").unwrap();
        assert_eq!(b.accounts().count(), 3);
        assert!(b.system(SystemAccount::NotSortedYet).is_ok());
        assert!(b.validate().is_ok());
        assert!(Book::new("  ").is_err());
    }

    #[test]
    fn from_parts_repairs_hostile_data() {
        let mut accounts = accounts();
        accounts.retain(|a| a.id != NOT_SORTED && a.id != SPAAR_FREE);
        accounts.push(acc(AccountId(1), Group::Goes, AccountKind::Category, None, "dupe"));
        accounts.push(acc(AccountId(50), Group::Owe, AccountKind::Category, Some(BETAAL), "Lost"));
        accounts.push(acc(AccountId(51), Group::Have, AccountKind::Pot, Some(GROCERIES), "Orphan pot"));
        let txns = vec![
            txn(1, d(2026, 1, 1), Some(BETAAL), "AH", &[(BETAAL, -500), (GROCERIES, 400)]),
            txn(1, d(2026, 1, 1), None, "dupe", &[(GROCERIES, 1), (SALARY, -1)]),
            txn(2, d(2026, 1, 2), Some(GROCERIES), "x", &[(DAILY, 0)]),
            Txn { duplicate_of: Some(TxnId(99)), ..txn(3, d(2026, 1, 3), None, "", &[(AccountId(999), 700), (GROCERIES, -700)]) },
        ];
        let rules = vec![
            Rule {
                id: RuleId(1),
                payee_prefix: Some("AH".into()),
                iban: None,
                account: GROCERIES,
                origin: RuleOrigin::Learned,
                mixed: false,
                uses: 0,
            },
            Rule { id: RuleId(2), payee_prefix: None, iban: None, account: GROCERIES, origin: RuleOrigin::Manual, mixed: false, uses: 0 },
            Rule { id: RuleId(3), payee_prefix: Some("X".into()), iban: None, account: DAILY, origin: RuleOrigin::Manual, mixed: false, uses: 0 },
        ];
        let meta = BookMeta { name: "".into(), next_txn: 1, ..BookMeta::default() };
        let (b, notes) = Book::from_parts(meta, accounts, txns, rules);
        assert!(!notes.is_empty());
        assert_eq!(b.validate(), Ok(()), "{notes:#?}");
        assert_eq!(b.name(), "Household");
        assert!(b.system(SystemAccount::NotSortedYet).is_ok(), "a missing system account comes back");
        assert!(b.tree().unassigned_pot(SPAAR).is_some(), "a Have account gets its Unassigned pot back");
        assert_eq!(b.txn_count(), 2, "the dupe and the empty one are gone");
        assert!(b.txn(TxnId(1)).unwrap().is_balanced());
        assert_eq!(b.txn(TxnId(3)).unwrap().duplicate_of, None);
        assert_eq!(b.rules().count(), 1);
        assert!(b.meta.next_txn > 3 && b.meta.next_account > 51);
        let lost = b.account(AccountId(50)).unwrap();
        assert_eq!((lost.group, lost.parent.and_then(|p| b.account(p)).map(|p| p.name.as_str())), (Group::Goes, Some("Other")));
        assert_eq!(b.account(AccountId(51)).unwrap().kind, AccountKind::Current);
    }

    #[test]
    fn diff_and_apply_round_trip() {
        let old = book(vec![txn(1, d(2026, 1, 1), Some(BETAAL), "AH", &[(BETAAL_FREE, -500), (GROCERIES, 500)])]);
        let mut new = old.clone();
        new.meta.name = "Renamed".into();
        new.txns.remove(&TxnId(1));
        new.rules.insert(
            RuleId(1),
            Arc::new(Rule {
                id: RuleId(1),
                payee_prefix: Some("AH".into()),
                iban: None,
                account: GROCERIES,
                origin: RuleOrigin::Manual,
                mixed: false,
                uses: 0,
            }),
        );
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 3);
        let mut replay = old.clone();
        replay.apply(&changes);
        assert_eq!(replay, new);
        assert!(diff(&new, &new).is_empty());
    }

    #[test]
    fn resolves_accounts_by_number_name_path_and_iban() {
        let b = book(vec![]);
        assert_eq!(b.resolve_account("21"), Some(GROCERIES));
        assert_eq!(b.resolve_account("#4"), Some(SPAAR));
        assert_eq!(b.resolve_account("groceries"), Some(GROCERIES));
        assert_eq!(b.resolve_account("ING Oranje Spaar / holiday"), Some(HOLIDAY));
        assert_eq!(b.resolve_account("Wallet/Unassigned"), Some(WALLET_FREE));
        assert_eq!(b.resolve_account("NL12 INGB 0001 2345 67"), Some(BETAAL));
        assert_eq!(b.resolve_account("nope"), None);
        assert_eq!(b.resolve_account("999"), None);
        assert_eq!(b.account_label(HOLIDAY), "ING Oranje Spaar / Holiday");
    }
}
