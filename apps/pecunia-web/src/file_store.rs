//! A book file the person picked in the browser.
//!
//! Browser file access is asynchronous and [`Store`] is not, so this store keeps the file's bytes
//! in memory. `apply` encodes the book and queues the bytes; a background task in `web.rs` writes
//! them and polls the file for changes made by other programs. Both sides share [`Shared`] through
//! a mutex. See ADR 0005.

use std::sync::{Arc, Mutex, MutexGuard};

use pecunia_engine::model::{Book, Change};
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
}

impl BrowserFileStore {
    /// A store for a file just read (`bytes`, `stamp`); the returned handle is for the writer task.
    pub fn new(name: &str, bytes: Vec<u8>, stamp: Option<Stamp>) -> (BrowserFileStore, SharedRef) {
        let shared = Arc::new(Mutex::new(Shared { name: name.to_string(), bytes, stamp, ..Shared::default() }));
        (BrowserFileStore { shared: shared.clone() }, shared)
    }
}

impl Drop for BrowserFileStore {
    fn drop(&mut self) {
        lock(&self.shared).closed = true;
    }
}

impl Store for BrowserFileStore {
    fn describe(&self) -> String {
        lock(&self.shared).name.clone()
    }

    fn load(&mut self) -> store::Result<Loaded> {
        let mut s = lock(&self.shared);
        let loaded = store::decode(&s.bytes, &s.name)?;
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
        let bytes = store::encode(book)?;
        s.bytes = bytes.clone();
        s.pending = Some(bytes);
        // The engine shows "Not saved" until a write works again; the app retries then.
        match &s.write_error {
            Some(e) => Err(StoreError::Io(format!("{}: {e}", s.name))),
            None => Ok(()),
        }
    }

    fn changed_externally(&mut self) -> bool {
        lock(&self.shared).external
    }
}

#[cfg(test)]
mod tests {
    use pecunia_engine::Session;
    use serde_json::json;

    use super::*;

    fn file() -> (BrowserFileStore, SharedRef, Book) {
        let book = Book::with_starter_categories("Web").unwrap();
        let (store, shared) = BrowserFileStore::new("web.pecunia", store::encode(&book).unwrap(), Some((1.0, 2.0)));
        (store, shared, book)
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
        let (mut store, _shared) = BrowserFileStore::new("bad.pecunia", b"{\"format\":1".to_vec(), None);
        assert!(store.load().is_err());
    }

    #[test]
    fn dropping_the_store_tells_the_writer_to_stop() {
        let (store, shared, _) = file();
        drop(store);
        assert!(lock(&shared).closed);
    }
}
