//! The one path from bank rows to the books (budget.rs `ingest.rs`): dedup (external id → import hash → fuzzy match
//! as a *possible* duplicate), match the other copy of a transfer, book the rest against *Not sorted yet*, then
//! classify (ADR 0004). Importing the same file again finds every row by its keys and changes nothing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::NaiveDate;
use serde::Serialize;

use crate::book::{Book, truncate};
use crate::classify::{Classifier, Facts};
use crate::dedup::{self, DedupCandidate, MAX_DAYS};
use crate::ledger;
use crate::money::Money;
use crate::statement::{RawTransaction, StatementBalance};
use crate::tree::{AccountTree, normalize_iban};
use crate::types::*;
use crate::{Result, invalid, limits, payees};

/// What an import did (or, for a dry run, would do).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct IngestResult {
    pub import: Option<ImportId>,
    pub rows: usize,
    /// New transactions.
    pub new: Vec<TxnId>,
    /// Rows already in the books (same key).
    pub known: usize,
    /// Second copies of transfers between own accounts, matched to the transaction already there.
    pub transfers: usize,
    /// New rows that look like a duplicate of a row from another file; a person decides.
    pub duplicates: usize,
    /// Rows of zero euros, skipped.
    pub skipped: usize,
    /// New transactions the rules or memory sorted.
    pub sorted: usize,
    /// New transactions waiting in Review.
    pub to_review: usize,
    /// First and last date of the rows.
    pub first: Option<NaiveDate>,
    pub last: Option<NaiveDate>,
}

/// Where an import comes from, for the import log.
#[derive(Debug, Clone, Default)]
pub struct ImportSource {
    pub file: String,
    pub format: String,
    pub balances: Vec<StatementBalance>,
}

impl Book {
    /// Ingests the rows of one physical account. With `dry_run` it only counts and changes nothing.
    pub fn ingest(
        &mut self,
        account: AccountId,
        rows: &[RawTransaction],
        source: SourceKind,
        from: &ImportSource,
        now: i64,
        dry_run: bool,
    ) -> Result<IngestResult> {
        let tree = self.tree();
        let a = tree.account(account)?;
        if !a.kind.is_physical() {
            return Err(invalid(format!("{} is not a bank account, card or loan; pick one to import into", a.name)));
        }
        if a.closed {
            return Err(invalid(format!("{} is closed; open it again to import into it", a.name)));
        }
        if rows.len() > limits::ROWS {
            return Err(invalid(format!("a file holds at most {} rows", limits::ROWS)));
        }
        if self.txns.len().saturating_add(rows.len()) > limits::TXNS {
            return Err(invalid(format!("a book holds at most {} transactions", limits::TXNS)));
        }
        let not_sorted = tree.system(SystemAccount::NotSortedYet)?;
        let mut result = IngestResult {
            rows: rows.len(),
            first: rows.iter().map(|r| r.date).min(),
            last: rows.iter().map(|r| r.date).max(),
            ..IngestResult::default()
        };
        let hashes = payees::import_hashes(rows);
        let known_keys: HashSet<String> =
            self.txns.values().flat_map(|t| t.keys.iter()).filter(|k| k.account == account).map(|k| k.key.clone()).collect();
        // Transactions of other accounts that may hold the other copy of a transfer, by date.
        let mut others: Vec<Arc<Txn>> =
            self.txns.values().filter(|t| t.account.is_some_and(|s| s != account) && !t.keys.iter().any(|k| k.account == account)).cloned().collect();
        others.sort_by_key(|t| (t.date, t.id));
        // This account's own transactions, for fuzzy dedup.
        let mut mine: Vec<Arc<Txn>> = self.txns.values().filter(|t| t.account == Some(account)).cloned().collect();
        mine.sort_by_key(|t| (t.date, t.id));
        let mut absorbed: HashSet<TxnId> = HashSet::new();
        let mut seen_now: HashSet<String> = HashSet::new();
        let classifier = Classifier::new(self, &tree);
        let import = if dry_run { None } else { Some(self.take_import_id()) };
        result.import = import;
        let mut pending: Vec<(TxnId, crate::classify::Classified)> = Vec::new();

        for (row, hash) in rows.iter().zip(&hashes) {
            if row.amount.is_zero() || !row.amount.is_sane() {
                result.skipped += 1;
                continue;
            }
            let mut keys = Vec::new();
            if let Some(id) = row.external_id.as_deref().filter(|i| !i.trim().is_empty()) {
                keys.push(dedup::external_key(source, id));
            }
            keys.push(dedup::hash_key(hash));
            if keys.iter().any(|k| known_keys.contains(k) || seen_now.contains(k)) {
                result.known += 1;
                continue;
            }
            seen_now.extend(keys.iter().cloned());

            let candidates = window(&others, row.date, &absorbed);
            if let Some(hit) = transfer_match(&tree, account, row, not_sorted, &candidates) {
                result.known += 1;
                result.transfers += 1;
                absorbed.insert(hit.id);
                if !dry_run {
                    self.absorb_transfer(&tree, account, &hit, not_sorted, &keys)?;
                }
                continue;
            }

            let payee_norm = payees::normalize(row.counterparty_name.as_deref(), &row.description);
            let near: Vec<DedupCandidate> = window(&mine, row.date, &absorbed)
                .into_iter()
                .map(|t| DedupCandidate {
                    id: t.id,
                    date: t.date,
                    amount: t.bank_amount.unwrap_or_default(),
                    payee_norm: t.payee_norm.clone(),
                    source: t.source,
                })
                .collect();
            let duplicate_of = dedup::find_possible_duplicate(row.date, row.amount, &payee_norm, source, &near).map(|c| c.id);
            if let Some(d) = duplicate_of {
                result.duplicates += 1;
                absorbed.insert(d); // one original absorbs one duplicate
            }
            let iban = row.counterparty_iban.as_deref().map(normalize_iban).filter(|i| !i.is_empty() && i.len() <= limits::IBAN);
            let class = classifier.classify(&Facts { account: Some(account), payee_norm: &payee_norm, counterparty_iban: iban.as_deref() });
            if duplicate_of.is_some() || class.status != TxnStatus::Auto {
                result.to_review += 1;
            } else {
                result.sorted += 1;
            }
            if dry_run {
                continue;
            }
            let payee = [row.counterparty_name.as_deref(), Some(row.description.as_str()).filter(|d| !d.trim().is_empty())]
                .into_iter()
                .flatten()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" · ");
            let id = self.take_txn_id();
            let t = Txn {
                date: row.date,
                payee_norm: truncate(&payee_norm, limits::PAYEE),
                description: truncate(&row.description, limits::TEXT),
                counterparty_iban: iban,
                account: Some(account),
                bank_amount: Some(row.amount),
                source,
                import,
                status: TxnStatus::ToReview,
                duplicate_of,
                who: row.who.as_deref().map(|w| truncate(w.trim(), limits::NAME)).filter(|w| !w.is_empty()),
                postings: ledger::book_bank_row(&tree, account, row.amount, None)?,
                keys: keys.into_iter().map(|key| TxnKey { account, key: truncate(&key, limits::KEY) }).collect(),
                created: now,
                ..self.blank_txn(id, TxnKind::Bank, &truncate(if payee.is_empty() { "(no description)" } else { &payee }, limits::PAYEE))
            };
            self.txns.insert(id, Arc::new(t));
            result.new.push(id);
            pending.push((id, class));
        }
        if dry_run {
            return Ok(result);
        }
        for (id, class) in &pending {
            self.book_classified(*id, class)?;
        }
        for b in &from.balances {
            if !b.amount.is_sane() {
                continue;
            }
            self.meta.balances.retain(|x| !(x.account == account && x.date == b.date));
            if self.meta.balances.len() < limits::BALANCES {
                self.meta.balances.push(StatedBalance { account, date: b.date, amount: b.amount, source: truncate(&from.file, limits::NAME * 2) });
            }
        }
        if let Some(id) = import {
            if self.meta.imports.len() >= limits::IMPORTS {
                self.meta.imports.remove(0);
            }
            self.meta.imports.push(ImportRecord {
                id,
                account,
                file: truncate(&from.file, limits::NAME * 2),
                format: truncate(&from.format, limits::NAME),
                at: now,
                rows: result.rows,
                new: result.new.len(),
                known: result.known,
                duplicates: result.duplicates,
                transfers: result.transfers,
            });
        }
        Ok(result)
    }

    /// Adds the row's keys to the matched transfer; when its counter side was still *Not sorted yet*, it becomes the
    /// transfer to `account` now.
    fn absorb_transfer(&mut self, tree: &AccountTree, account: AccountId, hit: &Txn, not_sorted: AccountId, keys: &[String]) -> Result<()> {
        let postings =
            if hit.postings.iter().any(|p| p.account == not_sorted) { Some(ledger::reclassify(tree, hit, tree.own_leaf(account)?)?) } else { None };
        if let Some(t) = self.txns.get_mut(&hit.id) {
            let t = Arc::make_mut(t);
            if let Some(p) = postings {
                t.postings = p;
                t.status = if t.duplicate_of.is_some() { TxnStatus::ToReview } else { TxnStatus::Auto };
                t.guesses.clear();
                t.via = "Transfer: seen at both banks".into();
            }
            t.transfer_likely = true;
            for key in keys {
                t.keys.push(TxnKey { account, key: truncate(key, limits::KEY) });
            }
        }
        Ok(())
    }

    /// The import log entries with the transactions they brought, newest first.
    pub fn imports(&self) -> Vec<(&ImportRecord, usize)> {
        let mut counts: HashMap<ImportId, usize> = HashMap::new();
        for t in self.txns.values() {
            if let Some(i) = t.import {
                *counts.entry(i).or_default() += 1;
            }
        }
        self.meta.imports.iter().rev().map(|i| (i, counts.get(&i.id).copied().unwrap_or(0))).collect()
    }

    /// Removes what an import brought that nobody has checked or changed since; returns how many transactions went.
    pub fn undo_import(&mut self, id: ImportId) -> Result<usize> {
        if !self.meta.imports.iter().any(|i| i.id == id) {
            return Err(crate::ModelError::NotFound(format!("import {id}")));
        }
        let gone: Vec<TxnId> = self.txns.values().filter(|t| t.import == Some(id) && t.status != TxnStatus::Reviewed).map(|t| t.id).collect();
        for g in &gone {
            self.delete_txn(*g)?;
        }
        if !self.txns.values().any(|t| t.import == Some(id)) {
            self.meta.imports.retain(|i| i.id != id);
        }
        Ok(gone.len())
    }
}

/// The transactions of `list` (sorted by date) at most [`MAX_DAYS`] from `date`, leaving out absorbed ones.
fn window(list: &[Arc<Txn>], date: NaiveDate, absorbed: &HashSet<TxnId>) -> Vec<Arc<Txn>> {
    let lo = date.checked_sub_days(chrono::Days::new(MAX_DAYS as u64)).unwrap_or(date);
    let hi = date.checked_add_days(chrono::Days::new(MAX_DAYS as u64)).unwrap_or(date);
    let start = list.partition_point(|t| t.date < lo);
    list.iter().skip(start).take_while(|t| t.date <= hi).filter(|t| !absorbed.contains(&t.id)).cloned().collect()
}

/// The transaction a row on `account` is the second copy of: either already booked as a transfer to `account`
/// ([`ledger::find_transfer_match`]), or still unsorted while its counterparty IBAN is this account's (or the row's
/// counterparty IBAN is the other account's).
fn transfer_match(tree: &AccountTree, account: AccountId, row: &RawTransaction, not_sorted: AccountId, candidates: &[Arc<Txn>]) -> Option<Txn> {
    let refs: Vec<&Txn> = candidates.iter().map(|t| t.as_ref()).collect();
    if let Some(hit) = ledger::find_transfer_match(tree, account, row.date, row.amount, &refs) {
        return Some(hit.clone());
    }
    let own_iban = tree.get(account).and_then(|a| a.iban.as_deref()).map(normalize_iban);
    let row_iban = row.counterparty_iban.as_deref().map(normalize_iban);
    refs.into_iter()
        .filter(|t| t.postings.iter().any(|p| p.account == not_sorted))
        .filter(|t| t.bank_amount.is_some_and(|m| m == -row.amount))
        .filter(|t| {
            let theirs = t.counterparty_iban.as_deref().map(normalize_iban);
            let source_iban = t.account.and_then(|s| tree.get(s)).and_then(|a| a.iban.as_deref()).map(normalize_iban);
            (own_iban.is_some() && theirs == own_iban) || (row_iban.is_some() && row_iban == source_iban)
        })
        .min_by_key(|t| ((t.date - row.date).num_days().abs(), t.id))
        .cloned()
}

/// A bank row, for tests and the sample.
pub fn row(date: NaiveDate, cents: i64, name: &str, description: &str) -> RawTransaction {
    let mut r = RawTransaction::new(date, Money(cents), description);
    if !name.is_empty() {
        r.counterparty_name = Some(name.into());
    }
    r.raw_line = format!("{date}|{cents}|{name}|{description}");
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    fn src() -> ImportSource {
        ImportSource {
            file: "ing.csv".into(),
            format: "CSV · ING".into(),
            balances: vec![StatementBalance { date: d(2026, 9, 30), amount: Money(150_000) }],
        }
    }

    #[test]
    fn imports_once_and_finds_rows_again() {
        let mut b = book(vec![]);
        let rows = vec![row(d(2026, 9, 1), -5210, "Albert Heijn 1427", "Boodschappen"), row(d(2026, 9, 2), 250_000, "Werkgever BV", "Salaris")];
        let dry = b.clone().ingest(BETAAL, &rows, SourceKind::Csv, &src(), 0, true).unwrap();
        assert_eq!((dry.new.len(), dry.to_review), (0, 2));
        let r = b.ingest(BETAAL, &rows, SourceKind::Csv, &src(), 0, false).unwrap();
        assert_eq!(r.new.len(), 2);
        assert_eq!(r.to_review, 2);
        let t = b.txn(r.new[0]).unwrap();
        assert_eq!(t.payee, "Albert Heijn 1427 · Boodschappen");
        assert_eq!(t.payee_norm, "ALBERT HEIJN");
        assert_eq!(t.postings, vec![p(BETAAL_FREE, -5210), p(NOT_SORTED, 5210)]);
        assert_eq!(b.meta.imports.len(), 1);
        assert_eq!(b.meta.balances.len(), 1);
        let again = b.ingest(BETAAL, &rows, SourceKind::Csv, &src(), 0, false).unwrap();
        assert_eq!((again.new.len(), again.known), (0, 2));
        assert_eq!(b.validate(), Ok(()));
        assert!(b.ingest(GROCERIES, &rows, SourceKind::Csv, &src(), 0, false).is_err());
    }

    #[test]
    fn matches_both_copies_of_a_transfer() {
        let mut b = book(vec![]);
        let mut out = row(d(2026, 9, 5), -20_000, "Oranje Spaarrekening", "Naar spaar");
        out.counterparty_iban = Some("NL34INGB0007654321".into());
        let r = b.ingest(BETAAL, &[out], SourceKind::Csv, &src(), 0, false).unwrap();
        let first = b.txn(r.new[0]).unwrap();
        assert!(first.transfer_likely, "the other IBAN is one of ours");
        assert_eq!(first.status, TxnStatus::ToReview);
        let mut back = row(d(2026, 9, 6), 20_000, "ING Betaalrekening", "Van betaal");
        back.counterparty_iban = Some("NL12INGB0001234567".into());
        let r2 = b.ingest(SPAAR, &[back], SourceKind::Mt940, &src(), 0, false).unwrap();
        assert_eq!((r2.new.len(), r2.transfers), (0, 1));
        let t = b.txn(r.new[0]).unwrap();
        assert_eq!(t.postings, vec![p(BETAAL_FREE, -20_000), p(SPAAR_FREE, 20_000)]);
        assert_eq!(t.status, TxnStatus::Auto);
        assert_eq!(t.keys.len(), 2, "keys on both accounts");
        assert_eq!(b.validate(), Ok(()));
    }

    #[test]
    fn rules_sort_and_duplicates_wait() {
        let mut b = book(vec![]);
        b.create_rule(Some("Albert Heijn"), None, GROCERIES).unwrap();
        let r = b.ingest(BETAAL, &[row(d(2026, 9, 1), -5210, "Albert Heijn 1427", "")], SourceKind::Csv, &src(), 0, false).unwrap();
        let t = b.txn(r.new[0]).unwrap();
        assert_eq!((t.status, t.class_source), (TxnStatus::Auto, Some(ClassSource::Rule)));
        assert_eq!(b.rules().next().map(|r| r.uses), Some(1));
        // The same purchase from another file of the same account: a possible duplicate, not dropped.
        let r2 = b.ingest(BETAAL, &[row(d(2026, 9, 2), -5210, "ALBERT HEIJN", "pin")], SourceKind::Mt940, &src(), 0, false).unwrap();
        assert_eq!(r2.duplicates, 1);
        let dup = b.txn(r2.new[0]).unwrap();
        assert_eq!((dup.duplicate_of, dup.status), (Some(r.new[0]), TxnStatus::ToReview));
        assert_eq!(b.undo_import(ImportId(2)).unwrap(), 1);
        assert_eq!(b.txn_count(), 1);
        assert_eq!(b.validate(), Ok(()));
    }
}
