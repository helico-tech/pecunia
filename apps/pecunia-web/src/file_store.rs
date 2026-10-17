//! A book file the person picked in the browser.
//!
//! Browser file access is asynchronous and [`Store`] is not, so this store keeps the file's bytes
//! in memory. `apply` encodes the book and queues the bytes; a background task in `web.rs` writes
//! them and polls the file for changes made by other programs. Both sides share [`Shared`] through
//! a mutex. See ADR 0005.

use std::sync::{Arc, Mutex, MutexGuard};

use pecunia_engine::model::{Book, Change};
use pecunia_engine::store::crypto::{Key, Random, Unlock};
use pecunia_engine::store::{self, Loaded, Store, StoreError};

/// The file's modification time (ms) and size, as the browser reports them.
pub type Stamp = (f64, f64);

#[derive(Debug, Default)]
pub struct Shared {
    /// The file name, for people (browsers don't reveal the folder).
    pub name: String,
    /// The file as last read, written or queued.
    pub bytes: Vec<u8>,
    /// Bytes waiting to be written.
    pub pending: Option<Vec<u8>>,
    /// The file's stamp after our last read or write; another stamp means another program wrote.
    pub stamp: Option<Stamp>,
    /// Another program changed the file; `bytes` holds its version.
    pub external: bool,
    /// Why the last write failed, until one works again.
    pub write_error: Option<String>,
    /// The store was dropped (another file was opened): write what is pending, then stop.
    pub closed: bool,
}

pub type SharedRef = Arc<Mutex<Shared>>;

/// Locks `s`. A panic while holding the lock can't leave `Shared` half-changed in a way that
/// matters (every field is replaced whole), so a poisoned lock is used as is.
pub fn lock(s: &SharedRef) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct BrowserFileStore {
    shared: SharedRef,
    /// The key of a protected book, kept while it is open (ADR 0007).
    key: Option<Key>,
    /// The password to try on the next load.
    password: Option<String>,
    random: Random,
    /// Kept in the browser's storage rather than a file (ADR 0009); people are told so.
    in_browser: bool,
}

impl BrowserFileStore {
    /// A store for a file just read (`bytes`, `stamp`); the returned handle is for the writer task. `password` unlocks
    /// a protected book; `random` is the browser's random source.
    pub fn new(name: &str, bytes: Vec<u8>, stamp: Option<Stamp>, password: Option<String>, random: Random) -> (BrowserFileStore, SharedRef) {
        let shared = Arc::new(Mutex::new(Shared { name: name.to_string(), bytes, stamp, ..Shared::default() }));
        (BrowserFileStore { shared: shared.clone(), key: None, password, random, in_browser: false }, shared)
    }

    /// The same, for a book kept in the browser's storage.
    pub fn kept_in_browser(mut self) -> BrowserFileStore {
        self.in_browser = true;
        self
    }
}

impl Drop for BrowserFileStore {
    fn drop(&mut self) {
        lock(&self.shared).closed = true;
    }
}

impl Store for BrowserFileStore {
    fn describe(&self) -> String {
        let name = lock(&self.shared).name.clone();
        if self.in_browser { format!("{name} (kept in this browser)") } else { name }
    }

    fn load(&mut self) -> store::Result<Loaded> {
        let mut s = lock(&self.shared);
        let unlock = match (&self.password, &self.key) {
            (Some(p), _) => Some(Unlock::Password(p.as_str())),
            (None, Some(k)) => Some(Unlock::Key(k)),
            (None, None) => None,
        };
        let (loaded, key) = store::decode_with(&s.bytes, &s.name, unlock)?;
        self.password = None;
        self.key = key;
        s.external = false;
        Ok(loaded)
    }

    fn apply(&mut self, book: &Book, changes: &[Change]) -> store::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut s = lock(&self.shared);
        if s.external {
            return Err(StoreError::Conflict);
        }
        let bytes = store::encode_with(book, self.key.as_ref(), self.random)?;
        s.bytes = bytes.clone();
        s.pending = Some(bytes);
        // The engine shows "Not saved" until a write works again; the app retries then.
        match &s.write_error {
            Some(e) => Err(StoreError::Io(format!("{}: {e}", s.name))),
            None => Ok(()),
        }
    }

    fn snapshot(&mut self) -> store::Result<Vec<u8>> {
        Ok(lock(&self.shared).bytes.clone())
    }

    fn changed_externally(&mut self) -> bool {
        lock(&self.shared).external
    }

    fn can_protect(&self) -> bool {
        true
    }

    fn encrypted(&self) -> bool {
        self.key.is_some()
    }

    fn set_password(&mut self, book: &Book, password: Option<&str>) -> store::Result<()> {
        let key = password.map(|p| Key::new(p, self.random)).transpose()?;
        let mut s = lock(&self.shared);
        if s.external {
            return Err(StoreError::Conflict);
        }
        let bytes = store::encode_with(book, key.as_ref(), self.random)?;
        s.bytes = bytes.clone();
        s.pending = Some(bytes);
        self.key = key;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pecunia_engine::Session;
    use serde_json::json;

    use super::*;

    /// Counts instead of randomness, so tests are repeatable.
    fn counter(buf: &mut [u8]) -> Result<(), String> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        Ok(())
    }

    fn file() -> (BrowserFileStore, SharedRef, Book) {
        let book = Book::with_starter_categories("Web").unwrap();
        let (store, shared) = BrowserFileStore::new("web.pecunia", store::encode(&book).unwrap(), Some((1.0, 2.0)), None, counter);
        (store, shared, book)
    }

    #[test]
    fn protected_books_open_with_the_password_and_save_encrypted() {
        let book = Book::with_starter_categories("Secret").unwrap();
        let (mut plain, shared, _) = file();
        plain.set_password(&book, Some("correct horse")).unwrap();
        let bytes = lock(&shared).pending.clone().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("pecunia-encrypted"));
        let (mut locked, _) = BrowserFileStore::new("secret.pecunia", bytes.clone(), None, None, counter);
        assert_eq!(locked.load().unwrap_err(), StoreError::Locked);
        let (mut wrong, _) = BrowserFileStore::new("secret.pecunia", bytes.clone(), None, Some("nope".into()), counter);
        assert_eq!(wrong.load().unwrap_err(), StoreError::WrongPassword);
        let (store, shared) = BrowserFileStore::new("secret.pecunia", bytes, None, Some("correct horse".into()), counter);
        let mut s = Session::scratch();
        s.open_store(Box::new(store)).unwrap();
        assert!(s.encrypted());
        s.execute("book.rename", json!({ "name": "Renamed" })).unwrap();
        let saved = lock(&shared).pending.clone().unwrap();
        assert!(!String::from_utf8_lossy(&saved).contains("Renamed"), "saves stay encrypted");
    }

    #[test]
    fn edits_queue_the_same_bytes_the_desktop_writes() {
        let (store, shared, book) = file();
        let mut s = Session::new(Box::new(store), book);
        s.execute("book.rename", json!({ "name": "From the browser" })).unwrap();
        let pending = lock(&shared).pending.clone().unwrap();
        assert_eq!(pending, store::encode(s.book()).unwrap());
        assert_eq!(store::decode(&pending, "x").unwrap().book, *s.book());
    }

    #[test]
    fn outside_changes_reload_and_win() {
        let (store, shared, book) = file();
        let mut s = Session::new(Box::new(store), book.clone());
        let mut other = book.clone();
        other.rename("From the CLI").unwrap();
        {
            let mut sh = lock(&shared);
            sh.bytes = store::encode(&other).unwrap();
            sh.external = true;
        }
        assert!(s.poll_external());
        assert_eq!(*s.book(), other);
        assert!(!lock(&shared).external);
    }

    #[test]
    fn a_failed_write_shows_as_not_saved() {
        let (store, shared, book) = file();
        let mut s = Session::new(Box::new(store), book);
        lock(&shared).write_error = Some("permission denied".into());
        assert!(s.execute("book.rename", json!({ "name": "x" })).is_err());
        assert!(s.save_error.is_some());
        assert!(lock(&shared).pending.is_some(), "the bytes stay queued for a retry");
        lock(&shared).write_error = None;
        s.flush().unwrap();
        assert!(s.save_error.is_none());
    }

    #[test]
    fn hostile_bytes_are_an_error() {
        let (mut store, _shared) = BrowserFileStore::new("bad.pecunia", b"{\"format\":1".to_vec(), None, None, counter);
        assert!(store.load().is_err());
    }

    #[test]
    fn dropping_the_store_tells_the_writer_to_stop() {
        let (store, shared, _) = file();
        drop(store);
        assert!(lock(&shared).closed);
    }
}
