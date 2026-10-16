use std::path::{Path, PathBuf};
use std::time::SystemTime;

use pecunia_model::{Account, Book, BookMeta, Change, Rule, Txn};
use serde::{Deserialize, Serialize};

use crate::{Loaded, Result, Store, StoreError};

pub const FILE_EXTENSION: &str = "pecunia";
const FORMAT: &str = "pecunia";
/// The newest file version this app reads.
const VERSION: u32 = 1;
/// Files larger than this are refused rather than read into memory.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// One book in one pretty-printed JSON file, records sorted by number, so the file diffs well. Every write replaces
/// the file atomically (`craft-atomic`): a crash leaves either the old or the new file, never half of one.
pub struct JsonFileStore {
    path: PathBuf,
    /// Modification time and length after our last read or write; a different value on disk means another program
    /// wrote the file.
    stamp: Option<(SystemTime, u64)>,
}

#[derive(Serialize)]
struct FileOut<'a> {
    format: &'a str,
    version: u32,
    meta: &'a BookMeta,
    accounts: Vec<&'a Account>,
    transactions: Vec<&'a Txn>,
    rules: Vec<&'a Rule>,
}

#[derive(Deserialize)]
struct FileIn {
    format: String,
    version: u32,
    meta: BookMeta,
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    transactions: Vec<Txn>,
    #[serde(default)]
    rules: Vec<Rule>,
}

impl JsonFileStore {
    /// A store for the file at `path`. Nothing is read until [`Store::load`].
    pub fn open(path: &Path) -> JsonFileStore {
        JsonFileStore { path: path.to_path_buf(), stamp: None }
    }

    /// Writes `book` to a new file at `path`; fails if the file exists.
    pub fn create(path: &Path, book: &Book) -> Result<JsonFileStore> {
        let bytes = encode(book)?;
        craft_atomic::write_new(path, &bytes).map_err(|e| io(path, e))?;
        let mut s = JsonFileStore::open(path);
        s.stamp = stamp(path);
        Ok(s)
    }
}

impl Store for JsonFileStore {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }

    fn path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn load(&mut self) -> Result<Loaded> {
        let meta = std::fs::metadata(&self.path).map_err(|e| io(&self.path, e))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(StoreError::Format(format!("{} is larger than {} MB", self.path.display(), MAX_FILE_BYTES / 1024 / 1024)));
        }
        let bytes = std::fs::read(&self.path).map_err(|e| io(&self.path, e))?;
        let loaded = decode(&bytes, &self.path.display().to_string())?;
        self.stamp = stamp(&self.path);
        Ok(loaded)
    }

    fn apply(&mut self, book: &Book, changes: &[Change]) -> Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        if self.changed_externally() {
            return Err(StoreError::Conflict);
        }
        let bytes = encode(book)?;
        craft_atomic::atomic_write(&self.path, &bytes).map_err(|e| io(&self.path, e))?;
        self.stamp = stamp(&self.path);
        Ok(())
    }

    fn changed_externally(&mut self) -> bool {
        stamp(&self.path) != self.stamp
    }
}

/// The bytes of a book file holding `book`. Every store that keeps a file uses this, so all Pecunia programs write
/// the same format.
pub fn encode(book: &Book) -> Result<Vec<u8>> {
    let (meta, accounts, transactions, rules) = book.parts();
    let out = FileOut { format: FORMAT, version: VERSION, meta, accounts, transactions, rules };
    let mut bytes = serde_json::to_vec_pretty(&out).map_err(|e| StoreError::Io(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Reads book file bytes; `name` (a path, a file name) is used in error messages. Invalid data is repaired and
/// reported in [`Loaded::notes`].
pub fn decode(bytes: &[u8], name: &str) -> Result<Loaded> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(StoreError::Format(format!("{name} is larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)));
    }
    let file: FileIn = serde_json::from_slice(bytes).map_err(|e| StoreError::Format(format!("{name}: {e}")))?;
    if file.format != FORMAT {
        return Err(StoreError::Format(format!("{name}: format is {:?}", file.format.chars().take(40).collect::<String>())));
    }
    if file.version > VERSION {
        return Err(StoreError::Format(format!(
            "{name} was made by a newer Pecunia (file version {}, this app reads up to {VERSION})",
            file.version
        )));
    }
    let (book, notes) = Book::from_parts(file.meta, file.accounts, file.transactions, file.rules);
    Ok(Loaded { book, notes })
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn io(path: &Path, e: std::io::Error) -> StoreError {
    StoreError::Io(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use pecunia_model::{AccountKind, Money, NaiveDate, NewAccount, NewTxn, diff};

    use super::*;

    fn sample() -> Book {
        let mut b = Book::with_starter_categories("Home").unwrap();
        let ing = b.create_account(NewAccount { kind: Some(AccountKind::Current), name: "ING".into(), ..Default::default() }).unwrap();
        let groceries = b.resolve_account("Groceries").unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        b.add_txn(NewTxn { date, payee: "Market".into(), from: ing, to: groceries, amount: Money(1250), note: String::new(), who: None }, 1).unwrap();
        b
    }

    #[test]
    fn create_load_apply_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.pecunia");
        let book = sample();
        let mut store = JsonFileStore::create(&path, &book).unwrap();
        assert!(JsonFileStore::create(&path, &book).is_err(), "create never overwrites");
        let loaded = store.load().unwrap();
        assert_eq!(loaded.book, book);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);

        let mut next = book.clone();
        next.rename("Renamed").unwrap();
        store.apply(&next, &diff(&book, &next)).unwrap();
        assert!(!store.changed_externally());
        assert_eq!(JsonFileStore::open(&path).load().unwrap().book, next);
    }

    #[test]
    fn external_writes_are_detected_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.pecunia");
        let book = sample();
        let mut store = JsonFileStore::create(&path, &book).unwrap();
        store.load().unwrap();
        let mut other = book.clone();
        other.rename("From the CLI, a longer name").unwrap();
        let mut cli = JsonFileStore::open(&path);
        assert_eq!(cli.apply(&other, &diff(&book, &other)), Err(StoreError::Conflict), "a store that never read the file doesn't write it");
        cli.load().unwrap();
        cli.apply(&other, &diff(&book, &other)).unwrap();
        assert!(store.changed_externally());
        let mut mine = book.clone();
        mine.rename("Mine").unwrap();
        assert_eq!(store.apply(&mine, &diff(&book, &mine)), Err(StoreError::Conflict));
        assert_eq!(JsonFileStore::open(&path).load().unwrap().book, other);
    }

    #[test]
    fn hostile_files_are_errors_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let cases: &[&[u8]] = &[
            b"",
            b"null",
            b"{}",
            b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[",
            br#"{"format":"other","version":1,"meta":{"name":"a"}}"#,
            br#"{"format":"pecunia","version":99,"meta":{"name":"a"}}"#,
            br#"{"format":"pecunia","version":1,"meta":{"name":"a"},"transactions":[{"id":-1}]}"#,
            &[0xff, 0xfe, 0x00],
        ];
        for (i, bytes) in cases.iter().enumerate() {
            let path = dir.path().join(format!("{i}.pecunia"));
            std::fs::write(&path, bytes).unwrap();
            assert!(JsonFileStore::open(&path).load().is_err(), "case {i}");
        }
        assert!(JsonFileStore::open(&dir.path().join("missing.pecunia")).load().is_err());
    }

    #[test]
    fn minimal_files_are_repaired_and_reported() {
        let loaded = decode(br#"{"format":"pecunia","version":1,"meta":{"name":"Mini"}}"#, "x").unwrap();
        assert_eq!(loaded.book.accounts().count(), 3, "system accounts are added");
        assert_eq!(loaded.notes.len(), 3);
        assert_eq!(loaded.book.validate(), Ok(()));
    }
}
