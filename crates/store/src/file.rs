use std::path::{Path, PathBuf};
use std::time::SystemTime;

use pecunia_model::{Account, Book, BookMeta, Change, Rule, Txn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::crypto::{self, Key, Random, Unlock};
use crate::{Loaded, Result, Store, StoreError};

pub const FILE_EXTENSION: &str = "pecunia";
const FORMAT: &str = "pecunia";
/// The file version this app writes, and the newest it reads (ADR 0006).
pub const VERSION: u32 = 1;
/// Files larger than this are refused rather than read into memory.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// A migration turns a book document of one version into the next, as JSON.
pub type Step = fn(&mut Value) -> std::result::Result<(), String>;

/// `MIGRATIONS[i]` turns file version `i + 1` into `i + 2`. Append a step with every version bump; never change one.
pub const MIGRATIONS: &[Step] = &[];

/// One book in one pretty-printed JSON file, records sorted by number, so the file diffs well; or, with a password,
/// that document encrypted (ADR 0007). Every write replaces the file atomically (`craft-atomic`): a crash leaves
/// either the old or the new file, never half of one.
pub struct JsonFileStore {
    path: PathBuf,
    /// Modification time and length after our last read or write; a different value on disk means another program
    /// wrote the file.
    stamp: Option<(SystemTime, u64)>,
    /// The key of an encrypted book, kept while it is open.
    key: Option<Key>,
    /// The password to try on the next load.
    password: Option<Zeroizing<String>>,
    /// The file version the book was migrated from; the first save keeps the original as a backup.
    migrated_from: Option<u32>,
    random: Random,
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
    meta: BookMeta,
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    transactions: Vec<Txn>,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[cfg(not(target_arch = "wasm32"))]
fn default_random() -> Random {
    crypto::os_random
}

#[cfg(target_arch = "wasm32")]
fn default_random() -> Random {
    |_| Err("no random source here".into())
}

impl JsonFileStore {
    /// A store for the file at `path`. Nothing is read until [`Store::load`].
    pub fn open(path: &Path) -> JsonFileStore {
        JsonFileStore { path: path.to_path_buf(), stamp: None, key: None, password: None, migrated_from: None, random: default_random() }
    }

    /// The same, with the password to unlock an encrypted book.
    pub fn with_password(mut self, password: Option<&str>) -> JsonFileStore {
        self.password = password.map(|p| Zeroizing::new(p.to_string()));
        self
    }

    /// Writes `book` to a new file at `path` (encrypted when a password is given); fails if the file exists.
    pub fn create(path: &Path, book: &Book, password: Option<&str>) -> Result<JsonFileStore> {
        let mut s = JsonFileStore::open(path);
        s.key = password.map(|p| Key::new(p, s.random)).transpose()?;
        let bytes = encode_with(book, s.key.as_ref(), s.random)?;
        craft_atomic::write_new(path, &bytes).map_err(|e| io(path, e))?;
        s.stamp = stamp(path);
        Ok(s)
    }

    /// Keeps the original file as `<name>.v<old>.bak` before the first save of a migrated book.
    fn back_up_original(&mut self) {
        let Some(old) = self.migrated_from.take() else { return };
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".v{old}.bak"));
        let backup = PathBuf::from(name);
        if !backup.exists()
            && let Err(e) = std::fs::copy(&self.path, &backup)
        {
            // A failed backup must not stop the save; the person still has their data.
            log::warn!("couldn't keep a backup of the old file: {e}");
        }
    }
}

impl Store for JsonFileStore {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }

    fn snapshot(&mut self) -> Result<Vec<u8>> {
        let meta = std::fs::metadata(&self.path).map_err(|e| io(&self.path, e))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(StoreError::Format(format!("{} is larger than {} MB", self.path.display(), MAX_FILE_BYTES / 1024 / 1024)));
        }
        std::fs::read(&self.path).map_err(|e| io(&self.path, e))
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
        // A password given to open the file wins; after that, the key kept from the last load.
        let unlock = match (&self.password, &self.key) {
            (Some(p), _) => Some(Unlock::Password(p.as_str())),
            (None, Some(k)) => Some(Unlock::Key(k)),
            (None, None) => None,
        };
        let (loaded, key) = decode_with(&bytes, &self.path.display().to_string(), unlock)?;
        self.password = None;
        self.key = key;
        self.migrated_from = loaded.migrated_from;
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
        let bytes = encode_with(book, self.key.as_ref(), self.random)?;
        self.back_up_original();
        craft_atomic::atomic_write(&self.path, &bytes).map_err(|e| io(&self.path, e))?;
        self.stamp = stamp(&self.path);
        Ok(())
    }

    fn changed_externally(&mut self) -> bool {
        stamp(&self.path) != self.stamp
    }

    fn encrypted(&self) -> bool {
        self.key.is_some()
    }

    fn set_password(&mut self, book: &Book, password: Option<&str>) -> Result<()> {
        if self.changed_externally() {
            return Err(StoreError::Conflict);
        }
        let key = password.map(|p| Key::new(p, self.random)).transpose()?;
        let bytes = encode_with(book, key.as_ref(), self.random)?;
        self.back_up_original();
        craft_atomic::atomic_write(&self.path, &bytes).map_err(|e| io(&self.path, e))?;
        self.key = key;
        self.stamp = stamp(&self.path);
        Ok(())
    }
}

/// The bytes of a plain book file holding `book`. Every store that keeps a file uses this, so all Pecunia programs
/// write the same format.
pub fn encode(book: &Book) -> Result<Vec<u8>> {
    let (meta, accounts, transactions, rules) = book.parts();
    let out = FileOut { format: FORMAT, version: VERSION, meta, accounts, transactions, rules };
    let mut bytes = serde_json::to_vec_pretty(&out).map_err(|e| StoreError::Io(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The bytes of a book file: plain, or sealed with `key`.
pub fn encode_with(book: &Book, key: Option<&Key>, random: Random) -> Result<Vec<u8>> {
    let plain = Zeroizing::new(encode(book)?);
    match key {
        Some(k) => crypto::seal(&plain, k, random),
        None => Ok(plain.to_vec()),
    }
}

/// Reads plain book file bytes; `name` (a path, a file name) is used in error messages. An encrypted file is
/// [`StoreError::Locked`]. Invalid data is repaired and reported in [`Loaded::notes`].
pub fn decode(bytes: &[u8], name: &str) -> Result<Loaded> {
    decode_with(bytes, name, None).map(|(l, _)| l)
}

/// Reads book file bytes, plain or encrypted. Returns the key an encrypted file was opened with.
pub fn decode_with(bytes: &[u8], name: &str, unlock: Option<Unlock>) -> Result<(Loaded, Option<Key>)> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(StoreError::Format(format!("{name} is larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)));
    }
    if crypto::is_encrypted(bytes) {
        let unlock = unlock.ok_or(StoreError::Locked)?;
        let (plain, key) = crypto::open(bytes, unlock, name)?;
        return Ok((decode_plain(&plain, name, MIGRATIONS)?, Some(key)));
    }
    Ok((decode_plain(bytes, name, MIGRATIONS)?, None))
}

/// Reads a plain book document, migrating it with `steps` (ADR 0006).
fn decode_plain(bytes: &[u8], name: &str, steps: &[Step]) -> Result<Loaded> {
    let bad = |e: String| StoreError::Format(format!("{name}: {e}"));
    let mut doc: Value = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
    let format = doc.get("format").and_then(Value::as_str).unwrap_or("");
    if format != FORMAT {
        return Err(bad(format!("format is {:?}", format.chars().take(40).collect::<String>())));
    }
    let version = doc
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 1)
        .ok_or_else(|| bad("no file version".into()))?;
    let newest = 1 + steps.len() as u32;
    if version > newest {
        return Err(StoreError::Format(format!("{name} was made by a newer Pecunia (file version {version}, this app reads up to {newest})")));
    }
    migrate(&mut doc, version, steps).map_err(bad)?;
    let file: FileIn = serde_json::from_value(doc).map_err(|e| bad(e.to_string()))?;
    let (book, notes) = Book::from_parts(file.meta, file.accounts, file.transactions, file.rules);
    Ok(Loaded { book, notes, migrated_from: (version < newest).then_some(version) })
}

/// Runs the steps from version `from` up to the newest, and sets the document's version.
fn migrate(doc: &mut Value, from: u32, steps: &[Step]) -> std::result::Result<(), String> {
    for (i, step) in steps.iter().enumerate().skip(from.saturating_sub(1) as usize) {
        let to = i as u32 + 2;
        step(doc).map_err(|e| format!("can't update the file to version {to}: {e}"))?;
        if let Some(o) = doc.as_object_mut() {
            o.insert("version".into(), Value::from(to));
        }
    }
    Ok(())
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
    use serde_json::json;

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
        let mut store = JsonFileStore::create(&path, &book, None).unwrap();
        assert!(JsonFileStore::create(&path, &book, None).is_err(), "create never overwrites");
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
    fn encrypted_books_round_trip_and_need_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.pecunia");
        let book = sample();
        let mut store = JsonFileStore::create(&path, &book, Some("correct horse")).unwrap();
        assert!(store.encrypted());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("pecunia-encrypted") && !raw.contains("Market"), "nothing readable in the file");
        assert_eq!(JsonFileStore::open(&path).load().unwrap_err(), StoreError::Locked);
        assert_eq!(JsonFileStore::open(&path).with_password(Some("nope")).load().unwrap_err(), StoreError::WrongPassword);
        let mut other = JsonFileStore::open(&path).with_password(Some("correct horse"));
        assert_eq!(other.load().unwrap().book, book);

        // The key kept from the first load reads and writes later saves without the password.
        let mut next = book.clone();
        next.rename("Renamed").unwrap();
        other.apply(&next, &diff(&book, &next)).unwrap();
        assert!(store.changed_externally());
        assert_eq!(store.load().unwrap().book, next);

        // Taking the password off writes the book in the clear; putting a new one on changes it.
        store.set_password(&next, None).unwrap();
        assert!(!store.encrypted());
        assert_eq!(JsonFileStore::open(&path).load().unwrap().book, next);
        store.set_password(&next, Some("new one")).unwrap();
        assert_eq!(JsonFileStore::open(&path).with_password(Some("new one")).load().unwrap().book, next);
        assert_eq!(JsonFileStore::open(&path).with_password(Some("correct horse")).load().unwrap_err(), StoreError::WrongPassword);
    }

    #[test]
    fn external_writes_are_detected_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("home.pecunia");
        let book = sample();
        let mut store = JsonFileStore::create(&path, &book, None).unwrap();
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
            br#"{"format":"pecunia","version":0,"meta":{"name":"a"}}"#,
            br#"{"format":"pecunia","version":-3,"meta":{"name":"a"}}"#,
            br#"{"format":"pecunia","version":1,"meta":{"name":"a"},"transactions":[{"id":-1}]}"#,
            br#"{"format":"pecunia-encrypted","version":1}"#,
            &[0xff, 0xfe, 0x00],
        ];
        for (i, bytes) in cases.iter().enumerate() {
            let path = dir.path().join(format!("{i}.pecunia"));
            std::fs::write(&path, bytes).unwrap();
            assert!(JsonFileStore::open(&path).with_password(Some("x")).load().is_err(), "case {i}");
        }
        assert!(JsonFileStore::open(&dir.path().join("missing.pecunia")).load().is_err());
    }

    #[test]
    fn minimal_files_are_repaired_and_reported() {
        let loaded = decode(br#"{"format":"pecunia","version":1,"meta":{"name":"Mini"}}"#, "x").unwrap();
        assert_eq!(loaded.book.accounts().count(), 3, "system accounts are added");
        assert_eq!(loaded.notes.len(), 3);
        assert_eq!(loaded.migrated_from, None);
        assert_eq!(loaded.book.validate(), Ok(()));
    }

    /// Migrations run in order from the file's version, and the newest version is one past the last step.
    #[test]
    fn migrations_run_in_order() {
        fn rename_book(doc: &mut Value) -> std::result::Result<(), String> {
            let name = doc["meta"]["title"].take();
            doc["meta"]["name"] = name;
            Ok(())
        }
        fn add_suffix(doc: &mut Value) -> std::result::Result<(), String> {
            let name = doc["meta"]["name"].as_str().ok_or("no name")?.to_string();
            doc["meta"]["name"] = json!(format!("{name} (v3)"));
            Ok(())
        }
        let steps: &[Step] = &[rename_book, add_suffix];
        let v1 = json!({ "format": "pecunia", "version": 1, "meta": { "title": "Old" } }).to_string();
        let l = decode_plain(v1.as_bytes(), "x", steps).unwrap();
        assert_eq!((l.book.name(), l.migrated_from), ("Old (v3)", Some(1)));
        let v2 = json!({ "format": "pecunia", "version": 2, "meta": { "name": "Mid" } }).to_string();
        assert_eq!(decode_plain(v2.as_bytes(), "x", steps).unwrap().book.name(), "Mid (v3)");
        let v3 = json!({ "format": "pecunia", "version": 3, "meta": { "name": "New" } }).to_string();
        let l = decode_plain(v3.as_bytes(), "x", steps).unwrap();
        assert_eq!((l.book.name(), l.migrated_from), ("New", None));
        let v4 = json!({ "format": "pecunia", "version": 4, "meta": { "name": "Future" } }).to_string();
        assert!(decode_plain(v4.as_bytes(), "x", steps).unwrap_err().to_string().contains("newer Pecunia"));
        fn fails(_: &mut Value) -> std::result::Result<(), String> {
            Err("broken".into())
        }
        assert!(decode_plain(v1.as_bytes(), "x", &[fails]).unwrap_err().to_string().contains("version 2"));
        assert_eq!(VERSION as usize, MIGRATIONS.len() + 1, "VERSION and MIGRATIONS move together");
    }
}
