#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Where a book lives (L1).
//!
//! The engine talks to storage only through [`Store`]. A backend loads a whole book once and then receives each edit
//! as a list of [`Change`]s (upserts and deletes of accounts, transactions and rules), so a backend that keeps rows
//! could write just those, while the file backend rewrites the file (ADR 0002). Backends today: [`MemoryStore`] and
//! [`JsonFileStore`] (plain or encrypted, ADR 0007; versioned with migrations, ADR 0006). [`open`] picks one from a
//! location string; that is the one place to register a new backend.

pub mod crypto;
mod file;
mod memory;

use std::path::Path;

pub use file::{FILE_EXTENSION, JsonFileStore, MAX_FILE_BYTES, MIGRATIONS, VERSION, decode, decode_with, encode, encode_with};
pub use memory::MemoryStore;
use pecunia_model::{Book, Change};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Io(String),
    #[error("not a Pecunia book: {0}")]
    Format(String),
    /// Someone else changed the data since this store last read or wrote it. Reload, then retry.
    #[error("the book was changed by another program")]
    Conflict,
    /// The file is encrypted and no password (or an old key) was given.
    #[error("this book is protected with a password")]
    Locked,
    #[error("wrong password, or the file is damaged")]
    WrongPassword,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// A loaded book, with one line per repair made to the stored data (see [`Book::from_parts`]).
#[derive(Debug)]
pub struct Loaded {
    pub book: Book,
    pub notes: Vec<String>,
    /// The file version the book was migrated from (ADR 0006), if it was older than this app's.
    pub migrated_from: Option<u32>,
}

/// A storage backend for one book.
pub trait Store: Send {
    /// Where the data lives, for people: a path, "memory".
    fn describe(&self) -> String;

    /// The file behind this store, if it is one.
    fn path(&self) -> Option<&Path> {
        None
    }

    /// Reads the whole book.
    fn load(&mut self) -> Result<Loaded>;

    /// Stores `changes`, which turn the last loaded or stored state into `book`. A backend may write only the changes
    /// or all of `book`. Fails with [`StoreError::Conflict`] when someone else changed the data in between; nothing is
    /// written then.
    fn apply(&mut self, book: &Book, changes: &[Change]) -> Result<()>;

    /// Whether someone else changed the data since the last load or apply. Cheap: the app calls it about once a
    /// second.
    fn changed_externally(&mut self) -> bool {
        false
    }

    /// Whether the book is stored encrypted (ADR 0007).
    fn encrypted(&self) -> bool {
        false
    }

    /// Protects the stored book with `password`, changes it, or (`None`) stores the book in the clear again. Writes at
    /// once. Fails with [`StoreError::Conflict`] when someone else changed the data since the last load or write.
    fn set_password(&mut self, _book: &Book, _password: Option<&str>) -> Result<()> {
        Err(StoreError::Io(format!("{} can't be protected with a password", self.describe())))
    }
}

/// Opens the store at `location`: `memory:` for a throwaway in-memory book, anything else is a file path. New
/// backends are added here.
pub fn open(location: &str, password: Option<&str>) -> Result<Box<dyn Store>> {
    if location.trim() == "memory:" {
        let book = Book::new("Scratch").map_err(|e| StoreError::Format(e.to_string()))?;
        return Ok(Box::new(MemoryStore::new(book)));
    }
    Ok(Box::new(JsonFileStore::open(Path::new(location)).with_password(password)))
}

/// Creates a new store at `location` holding `book`, encrypted when a password is given. A file must not exist yet.
pub fn create(location: &str, book: &Book, password: Option<&str>) -> Result<Box<dyn Store>> {
    if location.trim() == "memory:" {
        return Ok(Box::new(MemoryStore::new(book.clone())));
    }
    Ok(Box::new(JsonFileStore::create(Path::new(location), book, password)?))
}
