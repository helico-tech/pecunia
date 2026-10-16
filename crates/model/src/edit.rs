//! Validated edits on a [`Book`]. Every method checks its input, changes nothing when it fails, and keeps the rules of
//! ADR 0003 (a valid tree, balanced transactions on leaves).

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::NaiveDate;

use crate::book::{Book, blank_account, truncate};
use crate::classify::{self, RuleChange};
use crate::ledger::{self, LedgerError};
use crate::money::Money;
use crate::statement::CsvProfile;
use crate::tree::normalize_iban;
use crate::types::*;
use crate::{ModelError, Result, invalid, limits};

/// The fields of a new account. Only `kind` and `name` are always needed; a pot or category needs a `parent`, a
/// category group a `group` (goes or comes).
#[derive(Clone, Debug, Default)]
pub struct NewAccount {
    pub kind: Option<AccountKind>,
    pub name: String,
    pub group: Option<Group>,
    pub parent: Option<AccountId>,
    pub description: String,
    pub iban: Option<String>,
    pub goal: Option<Money>,
    pub goal_date: Option<NaiveDate>,
}

/// A change to some fields of an account. `None` leaves a field as it is; `Some(None)` clears it.
#[derive(Clone, Debug, Default)]
pub struct AccountPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub iban: Option<Option<String>>,
    pub goal: Option<Option<Money>>,
    pub goal_date: Option<Option<NaiveDate>>,
    /// Moves a category to another category group (of the same group).
    pub parent: Option<AccountId>,
    pub sort: Option<i32>,
    pub closed: Option<bool>,
}

/// A transaction entered by hand: `amount` goes from `from` to `to` (a physical account means its Unassigned pot).
#[derive(Clone, Debug)]
pub struct NewTxn {
    pub date: NaiveDate,
    pub payee: String,
    pub from: AccountId,
    pub to: AccountId,
    pub amount: Money,
    pub note: String,
    pub who: Option<String>,
}

/// A change to the descriptive fields of a transaction. `who: Some(None)` clears it.
#[derive(Clone, Debug, Default)]
pub struct TxnPatch {
    pub date: Option<NaiveDate>,
    pub payee: Option<String>,
    pub note: Option<String>,
    pub who: Option<Option<String>>,
}

fn not_found(what: &str, id: impl std::fmt::Display) -> ModelError {
    ModelError::NotFound(format!("{what} {id}"))
}

impl Book {
    // ------------------------------------------------------------------ the book

    pub fn rename(&mut self, name: &str) -> Result<()> {
        self.meta.name = crate::book::clean_book_name(name)?;
        Ok(())
    }

    // ------------------------------------------------------------------ accounts

    /// Adds an account. A physical account in Money you have gets its Unassigned pot.
    pub fn create_account(&mut self, new: NewAccount) -> Result<AccountId> {
        if self.accounts.len() >= limits::ACCOUNTS {
            return Err(invalid(format!("a book holds at most {} accounts", limits::ACCOUNTS)));
        }
        let kind = new.kind.ok_or_else(|| invalid("say what kind of account this is"))?;
        let tree = self.tree();
        let parent = match new.parent {
            Some(p) => Some(tree.account(p)?.clone()),
            None => None,
        };
        let group = match kind {
            AccountKind::System => return Err(invalid("system accounts are made by the app")),
            AccountKind::Pot => Group::Have,
            AccountKind::Category => parent.as_ref().map(|p| p.group).ok_or_else(|| invalid("a category needs a category group to sit in"))?,
            AccountKind::CategoryGroup => match new.group {
                Some(g @ (Group::Goes | Group::Comes)) => g,
                _ => return Err(invalid("a category group is in goes (spending) or comes (income)")),
            },
            k => k.physical_group().unwrap_or(Group::Have),
        };
        tree.check_placement(group, kind, new.parent)?;
        if parent.as_ref().is_some_and(|p| p.closed) {
            return Err(invalid("that account is closed; open it again first"));
        }
        let name = clean_name(&new.name)?;
        self.check_unique_name(&name, group, new.parent, None)?;
        let iban = self.clean_iban(new.iban.as_deref(), None)?;
        if iban.is_some() && !kind.is_physical() {
            return Err(invalid("only bank accounts and cards have an IBAN"));
        }
        let (goal, goal_date) = clean_goal(kind, new.goal, new.goal_date)?;
        let id = self.take_account_id();
        let sort = self.accounts.values().filter(|a| a.parent == new.parent && a.group == group).map(|a| a.sort).max().unwrap_or(0).saturating_add(1);
        let a = Account {
            id,
            group,
            kind,
            parent: new.parent,
            name,
            description: clean_text(&new.description, limits::DESCRIPTION)?,
            iban,
            goal,
            goal_date,
            sort,
            ..blank_account()
        };
        self.accounts.insert(id, Arc::new(a));
        if group == Group::Have && kind.is_physical() {
            self.add_unassigned_pot(id);
        }
        Ok(id)
    }

    /// Applies `patch`. Returns whether anything changed.
    pub fn update_account(&mut self, id: AccountId, patch: AccountPatch) -> Result<bool> {
        let old = self.accounts.get(&id).ok_or_else(|| not_found("account", id))?.clone();
        if old.system.is_some() {
            return Err(invalid(format!("{} is kept by the app and can't be changed", old.name)));
        }
        let mut a = Account::clone(&old);
        if let Some(n) = patch.name {
            if old.unassigned {
                return Err(invalid("the Unassigned pot keeps its name"));
            }
            a.name = clean_name(&n)?;
        }
        if let Some(d) = patch.description {
            a.description = clean_text(&d, limits::DESCRIPTION)?;
        }
        if let Some(i) = patch.iban {
            if i.is_some() && !a.kind.is_physical() {
                return Err(invalid("only bank accounts and cards have an IBAN"));
            }
            a.iban = self.clean_iban(i.as_deref(), Some(id))?;
        }
        if patch.goal.is_some() || patch.goal_date.is_some() {
            let (g, d) = clean_goal(a.kind, patch.goal.unwrap_or(a.goal), patch.goal_date.unwrap_or(a.goal_date))?;
            a.goal = g;
            a.goal_date = d;
        }
        if let Some(p) = patch.parent {
            if a.kind != AccountKind::Category {
                return Err(invalid("only categories move to another group"));
            }
            self.tree().check_placement(a.group, a.kind, Some(p))?;
            a.parent = Some(p);
        }
        if let Some(s) = patch.sort {
            a.sort = s.clamp(-1_000_000, 1_000_000);
        }
        if let Some(c) = patch.closed {
            if c && a.unassigned {
                return Err(invalid("the Unassigned pot closes with its account"));
            }
            a.closed = c;
        }
        if a.name != old.name || a.parent != old.parent {
            self.check_unique_name(&a.name, a.group, a.parent, Some(id))?;
        }
        if a == *old {
            return Ok(false);
        }
        // Closing an account closes its pots; opening it opens them.
        if a.closed != old.closed && a.kind.is_physical() {
            let pots: Vec<AccountId> = self.accounts.values().filter(|p| p.parent == Some(id)).map(|p| p.id).collect();
            for p in pots {
                if let Some(pa) = self.accounts.get_mut(&p) {
                    Arc::make_mut(pa).closed = a.closed;
                }
            }
        }
        self.accounts.insert(id, Arc::new(a));
        Ok(true)
    }

    /// Removes an account that has no money on it: no postings on it or below it, no transactions from it. Its pots
    /// (when empty) and the rules that answer with it go too. Close an account to keep its history instead.
    pub fn delete_account(&mut self, id: AccountId) -> Result<Vec<AccountId>> {
        let tree = self.tree();
        let a = tree.account(id)?.clone();
        if a.system.is_some() {
            return Err(invalid(format!("{} is kept by the app and can't be removed", a.name)));
        }
        if a.unassigned {
            return Err(invalid("the Unassigned pot goes with its account"));
        }
        let mut gone: Vec<AccountId> = vec![id];
        for c in tree.children(id) {
            if a.kind == AccountKind::CategoryGroup || (c.kind == AccountKind::Pot && !c.unassigned) {
                return Err(invalid(format!("{} still holds {}; remove or move that first", a.name, c.name)));
            }
            gone.push(c.id);
        }
        let used = self.txns.values().any(|t| t.postings.iter().any(|p| gone.contains(&p.account)) || t.account == Some(id));
        if used {
            return Err(invalid(format!("{} has transactions; close it instead to keep its history", a.name)));
        }
        for g in &gone {
            self.accounts.remove(g);
        }
        self.rules.retain(|_, r| !gone.contains(&r.account));
        self.meta.balances.retain(|b| b.account != id);
        // Guesses and keys that point at it.
        let touched: Vec<TxnId> = self
            .txns
            .values()
            .filter(|t| t.guesses.iter().any(|g| gone.contains(&g.account)) || t.keys.iter().any(|k| gone.contains(&k.account)))
            .map(|t| t.id)
            .collect();
        for t in touched {
            if let Some(t) = self.txns.get_mut(&t) {
                let t = Arc::make_mut(t);
                t.guesses.retain(|g| !gone.contains(&g.account));
                t.keys.retain(|k| !gone.contains(&k.account));
            }
        }
        Ok(gone)
    }

    fn check_unique_name(&self, name: &str, group: Group, parent: Option<AccountId>, except: Option<AccountId>) -> Result<()> {
        let clash =
            self.accounts.values().any(|a| Some(a.id) != except && a.group == group && a.parent == parent && a.name.eq_ignore_ascii_case(name));
        if clash {
            return Err(invalid(format!("there already is an account named {name} there")));
        }
        Ok(())
    }

    fn clean_iban(&self, iban: Option<&str>, except: Option<AccountId>) -> Result<Option<String>> {
        let Some(i) = iban.map(normalize_iban).filter(|i| !i.is_empty()) else { return Ok(None) };
        let ok = i.len() >= 8
            && i.len() <= limits::IBAN
            && i.chars().all(|c| c.is_ascii_alphanumeric())
            && i.chars().take(2).all(|c| c.is_ascii_alphabetic());
        if !ok {
            return Err(invalid(format!("{} is not an IBAN", truncate(&i, 40))));
        }
        if let Some(other) = self.accounts.values().find(|a| Some(a.id) != except && a.iban.as_deref() == Some(i.as_str())) {
            return Err(invalid(format!("{} already has that IBAN", other.name)));
        }
        Ok(Some(i))
    }

    // ------------------------------------------------------------------ balances people set

    /// Sets the starting balance of a physical account or pot (`shown`: what you have or owe, positive) on `date`.
    /// Replaces an earlier starting balance of the same account.
    pub fn set_starting_balance(&mut self, account: AccountId, shown: Money, date: NaiveDate, now: i64) -> Result<Option<TxnId>> {
        let tree = self.tree();
        let leaf = tree.resolve_leaf(account)?;
        if tree.physical_of(account).is_none() {
            return Err(LedgerError::NotPhysical { name: tree.name(account) }.into());
        }
        let existing = self.txns.values().find(|t| t.kind == TxnKind::Opening && t.postings.iter().any(|p| p.account == leaf)).map(|t| t.id);
        if shown.is_zero() {
            if let Some(e) = existing {
                self.txns.remove(&e);
            }
            return Ok(None);
        }
        let postings = ledger::opening_balance(&tree, account, shown)?;
        let id = existing.unwrap_or_else(|| self.take_txn_id());
        let t = Txn { date, postings, created: now, ..self.blank_txn(id, TxnKind::Opening, "Starting balance") };
        self.txns.insert(id, Arc::new(t));
        Ok(Some(id))
    }

    /// Books a new value for something estimated (a house): the difference with its balance on `date`.
    pub fn set_value(&mut self, account: AccountId, shown: Money, date: NaiveDate, now: i64) -> Result<Option<TxnId>> {
        if !shown.is_sane() {
            return Err(LedgerError::TooLarge.into());
        }
        let tree = self.tree();
        let current = self.balances_at(date).shown(&tree, account);
        if current == shown {
            return Ok(None);
        }
        let postings = ledger::revaluation(&tree, account, current, shown)?;
        let id = self.take_txn_id();
        let t = Txn { date, postings, created: now, ..self.blank_txn(id, TxnKind::Adjustment, "New value") };
        self.txns.insert(id, Arc::new(t));
        Ok(Some(id))
    }

    pub(crate) fn blank_txn(&self, id: TxnId, kind: TxnKind, payee: &str) -> Txn {
        Txn {
            id,
            kind,
            date: NaiveDate::default(),
            payee: payee.into(),
            payee_norm: String::new(),
            description: String::new(),
            counterparty_iban: None,
            note: String::new(),
            account: None,
            bank_amount: None,
            source: SourceKind::Manual,
            import: None,
            status: TxnStatus::Reviewed,
            class_source: None,
            guesses: Vec::new(),
            via: String::new(),
            transfer_likely: false,
            duplicate_of: None,
            who: None,
            postings: Vec::new(),
            keys: Vec::new(),
            created: 0,
        }
    }

    // ------------------------------------------------------------------ transactions

    /// Adds a transaction entered by hand: cash spent, a card nobody imports, a gift received.
    pub fn add_txn(&mut self, new: NewTxn, now: i64) -> Result<TxnId> {
        if self.txns.len() >= limits::TXNS {
            return Err(invalid(format!("a book holds at most {} transactions", limits::TXNS)));
        }
        if !new.amount.is_positive() {
            return Err(LedgerError::NotPositive.into());
        }
        if !new.amount.is_sane() {
            return Err(LedgerError::TooLarge.into());
        }
        let tree = self.tree();
        let (from, to) = (tree.resolve_leaf(new.from)?, tree.resolve_leaf(new.to)?);
        if from == to || (tree.physical_of(from).is_some() && tree.physical_of(from) == tree.physical_of(to)) {
            if tree.physical_of(from).is_some() && from != to {
                return Err(invalid("money between pots of one account is a pot move"));
            }
            return Err(LedgerError::SameAccount { name: tree.name(new.from) }.into());
        }
        for leaf in [from, to] {
            if tree.get(leaf).is_some_and(|a| a.closed) {
                return Err(invalid(format!("{} is closed", tree.name(leaf))));
            }
        }
        let postings = vec![Posting::new(from, -new.amount), Posting::new(to, new.amount)];
        ledger::check_postings(&tree, TxnKind::Manual, &postings)?;
        let payee = clean_payee(&new.payee)?;
        let id = self.take_txn_id();
        let t = Txn {
            date: new.date,
            payee_norm: crate::payees::normalize(None, &payee),
            note: clean_text(&new.note, limits::TEXT)?,
            who: new.who.map(|w| truncate(w.trim(), limits::NAME)).filter(|w| !w.is_empty()),
            postings,
            class_source: Some(ClassSource::User),
            created: now,
            ..self.blank_txn(id, TxnKind::Manual, &payee)
        };
        self.txns.insert(id, Arc::new(t));
        Ok(id)
    }

    /// Changes the date, payee, note or person of a transaction. Returns whether anything changed.
    pub fn update_txn(&mut self, id: TxnId, patch: TxnPatch) -> Result<bool> {
        let old = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?.clone();
        let mut t = Txn::clone(&old);
        if let Some(d) = patch.date {
            t.date = d;
        }
        if let Some(p) = patch.payee {
            t.payee = clean_payee(&p)?;
            if t.kind != TxnKind::Bank {
                t.payee_norm = crate::payees::normalize(None, &t.payee);
            }
        }
        if let Some(n) = patch.note {
            t.note = clean_text(&n, limits::TEXT)?;
        }
        if let Some(w) = patch.who {
            t.who = w.map(|w| truncate(w.trim(), limits::NAME)).filter(|w| !w.is_empty());
        }
        if t == *old {
            return Ok(false);
        }
        self.txns.insert(id, Arc::new(t));
        Ok(true)
    }

    /// Removes a transaction. Its dedup keys go too, so importing its bank row again brings it back.
    pub fn delete_txn(&mut self, id: TxnId) -> Result<Txn> {
        let t = self.txns.remove(&id).ok_or_else(|| not_found("transaction", id))?;
        let pointing: Vec<TxnId> = self.txns.values().filter(|o| o.duplicate_of == Some(id)).map(|o| o.id).collect();
        for p in pointing {
            if let Some(o) = self.txns.get_mut(&p) {
                Arc::make_mut(o).duplicate_of = None;
            }
        }
        Ok(Txn::clone(&t))
    }

    /// A person says where the money went or came from: a category, or another own account (a transfer). Books it as
    /// checked, then learns (ADR 0004): `always` makes a rule for this payee at once; otherwise two identical answers
    /// make a learned rule and a contradiction marks a learned rule mixed. Transfers teach no rules.
    pub fn answer(&mut self, id: TxnId, counter: AccountId, always: bool) -> Result<RuleChange> {
        let tree = self.tree();
        let old = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?.clone();
        let target = tree.account(counter)?.clone();
        if target.closed {
            return Err(invalid(format!("{} is closed", target.name)));
        }
        let leaf = tree.resolve_leaf(counter)?;
        let postings = match old.account {
            Some(_) => ledger::reclassify(&tree, &old, leaf)?,
            None => manual_counter(&tree, &old, leaf)?,
        };
        let is_category = target.kind == AccountKind::Category;
        let mut t = Txn::clone(&old);
        t.postings = postings;
        t.status = TxnStatus::Reviewed;
        t.class_source = Some(ClassSource::User);
        t.transfer_likely = !is_category && target.kind.is_physical();
        t.via = String::new();
        self.txns.insert(id, Arc::new(t));
        if !is_category || old.payee_norm.is_empty() || old.account.is_none() {
            return Ok(RuleChange::None);
        }
        let existing = self.rules.values().find(|r| r.payee_prefix.as_deref() == Some(old.payee_norm.as_str())).map(|r| (**r).clone());
        if always {
            match existing {
                None => {
                    self.insert_rule(Some(old.payee_norm.clone()), None, counter, RuleOrigin::Manual)?;
                }
                Some(mut r) => {
                    r.account = counter;
                    r.mixed = false;
                    r.origin = RuleOrigin::Manual;
                    self.rules.insert(r.id, Arc::new(r));
                }
            }
            return Ok(RuleChange::Create { payee_prefix: old.payee_norm.clone(), account: counter });
        }
        let mut answers: BTreeMap<AccountId, u32> = BTreeMap::new();
        for t in self
            .txns
            .values()
            .filter(|t| t.payee_norm == old.payee_norm && t.class_source == Some(ClassSource::User) && t.status == TxnStatus::Reviewed)
        {
            if let Some(c) = classify::counter_account(&tree, t) {
                *answers.entry(c).or_default() += 1;
            }
        }
        let change = classify::after_answer(&old.payee_norm, &answers, existing.as_ref());
        match &change {
            RuleChange::Create { payee_prefix, account } => {
                self.insert_rule(Some(payee_prefix.clone()), None, *account, RuleOrigin::Learned)?;
            }
            RuleChange::MarkMixed(r) => {
                if let Some(r) = self.rules.get_mut(r) {
                    Arc::make_mut(r).mixed = true;
                }
            }
            RuleChange::None => {}
        }
        Ok(change)
    }

    /// Marks a transaction as checked without changing where it is booked (a person agrees with the classifier).
    pub fn confirm(&mut self, id: TxnId) -> Result<bool> {
        let t = self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?;
        if t.status == TxnStatus::Reviewed && t.duplicate_of.is_none() {
            return Ok(false);
        }
        let not_sorted = self.accounts.values().find(|a| a.system == Some(SystemAccount::NotSortedYet)).map(|a| a.id);
        if t.postings.iter().any(|p| Some(p.account) == not_sorted) {
            return Err(invalid("say where this money went first"));
        }
        let t = Arc::make_mut(t);
        t.status = TxnStatus::Reviewed;
        t.duplicate_of = None;
        Ok(true)
    }

    /// Splits the counter side of a bank transaction over several accounts (mortgage = interest + repayment).
    pub fn split_txn(&mut self, id: TxnId, parts: &[Posting]) -> Result<()> {
        if parts.is_empty() || parts.len() > limits::POSTINGS {
            return Err(invalid(format!("a split has 1 to {} parts", limits::POSTINGS)));
        }
        let tree = self.tree();
        let old = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?;
        let postings = ledger::split(&tree, old, parts)?;
        let t = Arc::make_mut(self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?);
        t.postings = postings;
        t.status = TxnStatus::Reviewed;
        t.class_source = Some(ClassSource::User);
        t.transfer_likely = false;
        Ok(())
    }

    /// Moves the own side of a bank transaction to another pot of its account (pay the energy bill from Bills).
    pub fn move_to_pot(&mut self, id: TxnId, pot: AccountId) -> Result<()> {
        let tree = self.tree();
        let old = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?;
        let postings = ledger::move_own_side(&tree, old, pot)?;
        let t = Arc::make_mut(self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?);
        t.postings = postings;
        Ok(())
    }

    /// Moves money between two pots of one account.
    pub fn pot_move(&mut self, from: AccountId, to: AccountId, amount: Money, date: NaiveDate, note: &str, now: i64) -> Result<TxnId> {
        let tree = self.tree();
        let from = match tree.get(from) {
            Some(a) if a.kind.is_physical() => tree.own_leaf(a.id)?,
            _ => from,
        };
        let to = match tree.get(to) {
            Some(a) if a.kind.is_physical() => tree.own_leaf(a.id)?,
            _ => to,
        };
        let postings = ledger::pot_move(&tree, from, to, amount)?;
        let id = self.take_txn_id();
        let payee = format!("{} → {}", tree.name(from), tree.name(to));
        let t = Txn { date, postings, note: clean_text(note, limits::TEXT)?, created: now, ..self.blank_txn(id, TxnKind::PotMove, &payee) };
        self.txns.insert(id, Arc::new(t));
        Ok(id)
    }

    /// Decides about a possible duplicate: `keep` keeps both; otherwise this one goes and its dedup keys move to the
    /// original, so importing the row again finds it.
    pub fn resolve_duplicate(&mut self, id: TxnId, keep: bool) -> Result<Option<TxnId>> {
        let t = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?.clone();
        let Some(original) = t.duplicate_of else { return Err(invalid("this transaction is not marked as a possible duplicate")) };
        if keep {
            let t = Arc::make_mut(self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?);
            t.duplicate_of = None;
            return Ok(None);
        }
        self.txns.remove(&id);
        if let Some(o) = self.txns.get_mut(&original) {
            let o = Arc::make_mut(o);
            for k in &t.keys {
                if !o.keys.contains(k) {
                    o.keys.push(k.clone());
                }
            }
        }
        Ok(Some(original))
    }

    /// The classifier was wrong: this is not money between own accounts. It goes back to Review without guesses.
    pub fn not_a_transfer(&mut self, id: TxnId) -> Result<()> {
        let t = Arc::make_mut(self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?);
        if !t.transfer_likely {
            return Err(invalid("this transaction is not marked as a transfer"));
        }
        t.transfer_likely = false;
        t.guesses.clear();
        t.via = "Not a transfer".into();
        Ok(())
    }

    // ------------------------------------------------------------------ rules

    pub(crate) fn insert_rule(
        &mut self,
        payee_prefix: Option<String>,
        iban: Option<String>,
        account: AccountId,
        origin: RuleOrigin,
    ) -> Result<RuleId> {
        if self.rules.len() >= limits::RULES {
            return Err(invalid(format!("a book holds at most {} rules", limits::RULES)));
        }
        let id = self.take_rule_id();
        self.rules.insert(id, Arc::new(Rule { id, payee_prefix, iban, account, origin, mixed: false, uses: 0 }));
        Ok(id)
    }

    /// Adds a payee rule: transactions whose normalised payee starts with `payee` (or whose counterparty IBAN is
    /// `iban`) go to `account`. Unchecked transactions still on *Not sorted yet* that match are booked at once;
    /// returns how many.
    pub fn create_rule(&mut self, payee: Option<&str>, iban: Option<&str>, account: AccountId) -> Result<(RuleId, usize)> {
        let (payee, iban) = self.check_rule(payee, iban, account)?;
        if let Some(p) = &payee
            && self.rules.values().any(|r| r.payee_prefix.as_deref() == Some(p.as_str()))
        {
            return Err(invalid(format!("there already is a rule for {p}")));
        }
        let id = self.insert_rule(payee, iban, account, RuleOrigin::Manual)?;
        let applied = self.apply_rules();
        Ok((id, applied))
    }

    pub fn update_rule(&mut self, id: RuleId, payee: Option<&str>, iban: Option<Option<&str>>, account: Option<AccountId>) -> Result<bool> {
        let old = self.rules.get(&id).ok_or_else(|| not_found("rule", id))?.clone();
        let payee = payee.or(old.payee_prefix.as_deref()).map(str::to_string);
        let iban = match iban {
            Some(i) => i.map(str::to_string),
            None => old.iban.clone(),
        };
        let account = account.unwrap_or(old.account);
        let (payee, iban) = self.check_rule(payee.as_deref(), iban.as_deref(), account)?;
        let r = Rule { payee_prefix: payee, iban, account, origin: RuleOrigin::Manual, mixed: false, ..Rule::clone(&old) };
        if r == *old {
            return Ok(false);
        }
        self.rules.insert(id, Arc::new(r));
        self.apply_rules();
        Ok(true)
    }

    pub fn delete_rule(&mut self, id: RuleId) -> Result<Rule> {
        self.rules.remove(&id).map(|r| Rule::clone(&r)).ok_or_else(|| not_found("rule", id))
    }

    fn check_rule(&self, payee: Option<&str>, iban: Option<&str>, account: AccountId) -> Result<(Option<String>, Option<String>)> {
        let payee = payee.map(|p| crate::payees::normalize(None, p)).filter(|p| !p.is_empty());
        let iban = iban.map(normalize_iban).filter(|i| !i.is_empty());
        if payee.is_none() && iban.is_none() {
            return Err(invalid("a rule needs a payee or an IBAN to look for"));
        }
        if iban.as_ref().is_some_and(|i| i.len() > limits::IBAN) {
            return Err(invalid("that is not an IBAN"));
        }
        let tree = self.tree();
        let a = tree.account(account)?;
        if a.kind != AccountKind::Category {
            return Err(invalid("a rule answers with a category"));
        }
        Ok((payee, iban))
    }

    /// Runs the rules over unchecked transactions still on *Not sorted yet*; returns how many were booked.
    pub fn apply_rules(&mut self) -> usize {
        let tree = self.tree();
        let Ok(not_sorted) = tree.system(SystemAccount::NotSortedYet) else { return 0 };
        let classifier = classify::Classifier::new(self, &tree);
        let pending: Vec<TxnId> = self
            .txns
            .values()
            .filter(|t| t.status == TxnStatus::ToReview && t.duplicate_of.is_none() && t.postings.iter().any(|p| p.account == not_sorted))
            .map(|t| t.id)
            .collect();
        let mut n = 0;
        for id in pending {
            let Some(t) = self.txns.get(&id) else { continue };
            let c = classifier.classify(&classify::Facts::from(t.as_ref()));
            if c.source != Some(ClassSource::Rule) {
                continue;
            }
            if self.book_classified(id, &c).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// Stores a classifier result on a transaction: the counter account when it answered, else the guesses.
    pub(crate) fn book_classified(&mut self, id: TxnId, c: &classify::Classified) -> Result<()> {
        let tree = self.tree();
        let old = self.txns.get(&id).ok_or_else(|| not_found("transaction", id))?;
        let postings = match c.counter {
            Some(counter) => ledger::reclassify(&tree, old, counter)?,
            None => old.postings.clone(),
        };
        let t = Arc::make_mut(self.txns.get_mut(&id).ok_or_else(|| not_found("transaction", id))?);
        t.postings = postings;
        t.status = if t.duplicate_of.is_some() { TxnStatus::ToReview } else { c.status };
        t.class_source = c.source;
        t.guesses = c.guesses.clone();
        t.transfer_likely = c.transfer_likely;
        t.via = c.via.clone();
        if let Some(r) = c.rule.and_then(|r| self.rules.get_mut(&r)) {
            let r = Arc::make_mut(r);
            r.uses = r.uses.saturating_add(1);
        }
        Ok(())
    }

    // ------------------------------------------------------------------ CSV profiles

    /// Saves a CSV layout the person mapped (replaces one with the same name).
    pub fn save_profile(&mut self, p: CsvProfile) -> Result<()> {
        if p.name.trim().is_empty() || p.name.chars().count() > limits::NAME {
            return Err(invalid("a CSV layout needs a short name"));
        }
        if p.date_column.trim().is_empty() || p.amount_column.trim().is_empty() {
            return Err(invalid("a CSV layout needs a date and an amount column"));
        }
        self.meta.profiles.retain(|x| !x.name.eq_ignore_ascii_case(&p.name));
        if self.meta.profiles.len() >= limits::PROFILES {
            return Err(invalid(format!("a book holds at most {} CSV layouts", limits::PROFILES)));
        }
        self.meta.profiles.push(p);
        Ok(())
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<()> {
        let before = self.meta.profiles.len();
        self.meta.profiles.retain(|x| !x.name.eq_ignore_ascii_case(name.trim()));
        if self.meta.profiles.len() == before {
            return Err(not_found("CSV layout", name));
        }
        Ok(())
    }
}

/// For a transaction entered by hand: the posting that is not on an own account gets the new counter account.
fn manual_counter(tree: &crate::tree::AccountTree, t: &Txn, leaf: AccountId) -> Result<Vec<Posting>> {
    let counter: Vec<usize> = t.postings.iter().enumerate().filter(|(_, p)| tree.physical_of(p.account).is_none()).map(|(i, _)| i).collect();
    let [i] = counter.as_slice() else { return Err(invalid("this transaction has no single category to change; edit its parts instead")) };
    let mut postings = t.postings.clone();
    if let Some(p) = postings.get_mut(*i) {
        p.account = leaf;
    }
    if postings.iter().filter(|p| p.account == leaf).count() > 1
        || postings.iter().all(|p| tree.physical_of(p.account) == tree.physical_of(leaf) && tree.physical_of(leaf).is_some())
    {
        return Err(LedgerError::SameAccount { name: tree.name(leaf) }.into());
    }
    ledger::check_postings(tree, t.kind, &postings)?;
    Ok(postings)
}

fn clean_name(s: &str) -> Result<String> {
    let n = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err(invalid("the name is empty"));
    }
    if n.chars().count() > limits::NAME {
        return Err(invalid(format!("a name is at most {} characters", limits::NAME)));
    }
    Ok(n)
}

fn clean_payee(s: &str) -> Result<String> {
    let n = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.chars().count() > limits::PAYEE {
        return Err(invalid(format!("a payee is at most {} characters", limits::PAYEE)));
    }
    Ok(n)
}

fn clean_text(s: &str, max: usize) -> Result<String> {
    if s.chars().count() > max {
        return Err(invalid(format!("the text is longer than {max} characters")));
    }
    Ok(s.trim_end().to_string())
}

fn clean_goal(kind: AccountKind, goal: Option<Money>, date: Option<NaiveDate>) -> Result<(Option<Money>, Option<NaiveDate>)> {
    if (goal.is_some() || date.is_some()) && kind != AccountKind::Pot {
        return Err(invalid("only pots have a goal"));
    }
    if goal.is_some_and(|g| !g.is_positive() || !g.is_sane()) {
        return Err(invalid("a goal is an amount above zero"));
    }
    Ok((goal, date.filter(|_| goal.is_some())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn bank(id: u64, payee: &str, cents: i64) -> Txn {
        Txn {
            status: TxnStatus::ToReview,
            ..txn(id, d(2026, 9, id as u32 % 28 + 1), Some(BETAAL), payee, &[(BETAAL_FREE, cents), (NOT_SORTED, -cents)])
        }
    }

    #[test]
    fn creates_accounts_where_they_belong() {
        let mut b = book(vec![]);
        let asn = b
            .create_account(NewAccount {
                kind: Some(AccountKind::Savings),
                name: "ASN Sparen".into(),
                iban: Some("nl55 asnb 0001 2345 67".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(b.tree().unassigned_pot(asn).is_some(), "a Have account gets its Unassigned pot");
        assert_eq!(b.account(asn).unwrap().iban.as_deref(), Some("NL55ASNB0001234567"));
        let pot = b
            .create_account(NewAccount {
                kind: Some(AccountKind::Pot),
                name: "Car".into(),
                parent: Some(asn),
                goal: Some(Money(500_000)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(b.account(pot).unwrap().group, Group::Have);
        let cat = b
            .create_account(NewAccount { kind: Some(AccountKind::Category), name: "Pets".into(), parent: Some(DAILY), ..Default::default() })
            .unwrap();
        assert_eq!(b.account(cat).unwrap().group, Group::Goes);
        let card = b.create_account(NewAccount { kind: Some(AccountKind::CreditCard), name: "Visa".into(), ..Default::default() }).unwrap();
        assert_eq!(b.account(card).unwrap().group, Group::Owe);
        assert!(b.tree().pots(card).is_empty());
        assert_eq!(b.validate(), Ok(()));

        let bad = [
            NewAccount { kind: Some(AccountKind::Pot), name: "x".into(), parent: Some(CARD), ..Default::default() },
            NewAccount { kind: Some(AccountKind::Category), name: "x".into(), ..Default::default() },
            NewAccount { kind: Some(AccountKind::Category), name: "Groceries".into(), parent: Some(DAILY), ..Default::default() },
            NewAccount { kind: Some(AccountKind::CategoryGroup), name: "x".into(), group: Some(Group::Have), ..Default::default() },
            NewAccount { kind: Some(AccountKind::System), name: "x".into(), ..Default::default() },
            NewAccount { kind: Some(AccountKind::Current), name: "  ".into(), ..Default::default() },
            NewAccount { kind: Some(AccountKind::Current), name: "x".into(), iban: Some("NL12INGB0001234567".into()), ..Default::default() },
            NewAccount { kind: Some(AccountKind::Category), name: "x".into(), parent: Some(DAILY), goal: Some(Money(1)), ..Default::default() },
            NewAccount { kind: None, name: "x".into(), ..Default::default() },
        ];
        for n in bad {
            let before = b.clone();
            assert!(b.create_account(n.clone()).is_err(), "{n:?}");
            assert_eq!(b, before);
        }
    }

    #[test]
    fn deletes_only_unused_accounts() {
        let mut b = book(vec![txn(1, d(2026, 1, 1), Some(BETAAL), "AH", &[(BETAAL_FREE, -500), (GROCERIES, 500)])]);
        assert!(b.delete_account(GROCERIES).is_err(), "it has money on it");
        assert!(b.delete_account(DAILY).is_err(), "it holds categories");
        assert!(b.delete_account(NOT_SORTED).is_err());
        assert!(b.delete_account(WALLET_FREE).is_err());
        assert_eq!(b.delete_account(WALLET).unwrap(), vec![WALLET, WALLET_FREE]);
        assert_eq!(b.delete_account(SALARY).unwrap(), vec![SALARY]);
        assert!(b.update_account(GROCERIES, AccountPatch { closed: Some(true), ..Default::default() }).unwrap());
        assert_eq!(b.validate(), Ok(()));
    }

    #[test]
    fn manual_transactions_and_pot_moves() {
        let mut b = book(vec![]);
        let id = b
            .add_txn(
                NewTxn {
                    date: d(2026, 3, 1),
                    payee: "Market".into(),
                    from: WALLET,
                    to: GROCERIES,
                    amount: Money(1250),
                    note: String::new(),
                    who: None,
                },
                0,
            )
            .unwrap();
        assert_eq!(b.txn(id).unwrap().postings, vec![p(WALLET_FREE, -1250), p(GROCERIES, 1250)]);
        assert!(
            b.add_txn(
                NewTxn { date: d(2026, 3, 1), payee: "x".into(), from: WALLET, to: WALLET, amount: Money(1), note: String::new(), who: None },
                0
            )
            .is_err()
        );
        assert!(
            b.add_txn(
                NewTxn { date: d(2026, 3, 1), payee: "x".into(), from: BETAAL, to: BILLS, amount: Money(1), note: String::new(), who: None },
                0
            )
            .is_err()
        );
        assert!(
            b.add_txn(
                NewTxn { date: d(2026, 3, 1), payee: "x".into(), from: WALLET, to: GROCERIES, amount: Money(-1), note: String::new(), who: None },
                0
            )
            .is_err()
        );
        b.answer(id, INTEREST, false).unwrap();
        assert_eq!(b.txn(id).unwrap().postings, vec![p(WALLET_FREE, -1250), p(INTEREST, 1250)], "a manual transaction changes its category");
        assert!(b.answer(id, WALLET, false).is_err(), "not to the account it came from");
        let mv = b.pot_move(SPAAR, HOLIDAY, Money(10_000), d(2026, 3, 2), "", 0).unwrap();
        assert_eq!(b.txn(mv).unwrap().postings, vec![p(SPAAR_FREE, -10_000), p(HOLIDAY, 10_000)]);
        assert!(b.pot_move(BETAAL, HOLIDAY, Money(1), d(2026, 3, 2), "", 0).is_err());
        assert_eq!(b.validate(), Ok(()));
    }

    #[test]
    fn answers_book_the_counter_side_and_learn_rules() {
        let mut b = book(vec![bank(1, "JUMBO", -1000), bank(2, "JUMBO", -2000), bank(3, "JUMBO", -3000)]);
        assert_eq!(b.answer(TxnId(1), GROCERIES, false).unwrap(), RuleChange::None);
        assert_eq!(b.txn(TxnId(1)).unwrap().postings, vec![p(BETAAL_FREE, -1000), p(GROCERIES, 1000)]);
        assert_eq!(b.txn(TxnId(1)).unwrap().status, TxnStatus::Reviewed);
        assert_eq!(b.answer(TxnId(2), GROCERIES, false).unwrap(), RuleChange::Create { payee_prefix: "JUMBO".into(), account: GROCERIES });
        assert_eq!(b.rules().count(), 1);
        assert_eq!(b.answer(TxnId(3), INTEREST, false).unwrap(), RuleChange::MarkMixed(RuleId(1)));
        assert!(b.rule(RuleId(1)).unwrap().mixed);
        // A transfer: the other own account, no rule.
        let mut b = book(vec![bank(1, "NAAR SPAAR", -5000)]);
        assert_eq!(b.answer(TxnId(1), SPAAR, false).unwrap(), RuleChange::None);
        assert_eq!(b.txn(TxnId(1)).unwrap().postings, vec![p(BETAAL_FREE, -5000), p(SPAAR_FREE, 5000)]);
        assert!(b.answer(TxnId(1), BETAAL, false).is_err(), "not to itself");
        assert!(b.answer(TxnId(1), DAILY, false).is_err(), "not to a group");
        assert_eq!(b.validate(), Ok(()));
    }

    #[test]
    fn always_makes_a_rule_and_rules_apply_to_waiting_transactions() {
        let mut b = book(vec![bank(1, "ALBERT HEIJN", -1000), bank(2, "ALBERT HEIJN UTRECHT", -2000), bank(3, "JUMBO", -3000)]);
        b.answer(TxnId(1), GROCERIES, true).unwrap();
        assert_eq!(b.rules().next().map(|r| r.origin), Some(RuleOrigin::Manual));
        assert_eq!(b.apply_rules(), 1);
        let t2 = b.txn(TxnId(2)).unwrap();
        assert_eq!((t2.status, t2.class_source), (TxnStatus::Auto, Some(ClassSource::Rule)));
        let (_, applied) = b.create_rule(Some("Jumbo"), None, GROCERIES).unwrap();
        assert_eq!(applied, 1);
        assert!(b.create_rule(Some("jumbo"), None, GROCERIES).is_err(), "one rule per payee");
        assert!(b.create_rule(None, None, GROCERIES).is_err());
        assert!(b.create_rule(Some("x"), None, BETAAL).is_err(), "rules answer with a category");
        assert_eq!(b.validate(), Ok(()));
    }

    #[test]
    fn splits_moves_duplicates_and_opening_balances() {
        let dup = Txn { duplicate_of: Some(TxnId(1)), keys: vec![TxnKey { account: BETAAL, key: "hash:b".into() }], ..bank(2, "AH", -500) };
        let mut b = book(vec![bank(1, "AH", -500), dup, bank(3, "HYPOTHEEK", -124_500)]);
        b.split_txn(TxnId(3), &[p(INTEREST, 61_230), p(MORTGAGE, 63_270)]).unwrap();
        assert_eq!(b.txn(TxnId(3)).unwrap().postings.len(), 3);
        b.move_to_pot(TxnId(3), BILLS).unwrap();
        assert_eq!(b.txn(TxnId(3)).unwrap().postings.first(), Some(&p(BILLS, -124_500)));
        assert_eq!(b.resolve_duplicate(TxnId(2), false).unwrap(), Some(TxnId(1)));
        assert!(b.txn(TxnId(2)).is_none());
        assert_eq!(b.txn(TxnId(1)).unwrap().keys.len(), 1, "the original keeps the duplicate's key");
        assert!(b.resolve_duplicate(TxnId(1), true).is_err());

        let id = b.set_starting_balance(SPAAR, Money(100_000), d(2026, 1, 1), 0).unwrap().unwrap();
        let again = b.set_starting_balance(SPAAR, Money(120_000), d(2026, 1, 1), 0).unwrap().unwrap();
        assert_eq!(id, again, "a second starting balance replaces the first");
        assert_eq!(b.balances().shown(&b.tree(), SPAAR), Money(120_000));
        assert!(b.set_starting_balance(GROCERIES, Money(1), d(2026, 1, 1), 0).is_err());
        b.set_starting_balance(SPAAR, Money::ZERO, d(2026, 1, 1), 0).unwrap();
        assert!(b.txn(id).is_none());
        assert_eq!(b.validate(), Ok(()));
    }
}
