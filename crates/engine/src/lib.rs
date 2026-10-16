#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! The Pecunia engine (L2): one open book, its store, selection, view, undo and the **command registry**. Every
//! user-visible action is a command id run through [`Session::execute`]; the UI, the command palette, the CLI and MCP
//! all dispatch by id.
//!
//! Saving is automatic: after each command the engine sends what changed to the [`Store`]. If another program (the
//! CLI, an agent over MCP, a sync tool) changed the data, the engine reloads instead of overwriting it (ADR 0002).

pub mod cmd;
pub mod sample;
pub mod view;

use std::sync::Arc;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use pecunia_model as model;
use pecunia_model::{Book, ModelError, NaiveDate, TxnId, diff};
pub use pecunia_store as store;
use pecunia_store::{Store, StoreError};
use serde_json::Value as Json;
pub use view::{Screen, ViewState};

const UNDO_LIMIT: usize = 100;
const JOURNAL_LIMIT: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command {0}")]
    UnknownCommand(String),
    #[error("{0} is not available: {1}")]
    Disabled(String, String),
    #[error("{cmd}: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{0} failed unexpectedly ({1}); the book was restored")]
    Internal(String, String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Something the UI should do that the engine can't: show a dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum UiRequest {
    /// Open the named dialog (`newTxn`, `newAccount`, `import`, `open`, `newBook`, …) with these defaults.
    Dialog(String, Json),
}

/// A short message for the person, shown as a toast. `undo` offers an Undo button.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub undo: bool,
    pub error: bool,
}

struct History {
    label: String,
    book: Arc<Book>,
    selection: Option<TxnId>,
}

pub struct Session {
    book: Arc<Book>,
    /// What the store holds. `book != saved` only while a save is failing.
    saved: Arc<Book>,
    store: Box<dyn Store>,
    /// The selected transaction (the detail pane shows it).
    pub selection: Option<TxnId>,
    pub view: ViewState,
    undo: Vec<History>,
    redo: Vec<History>,
    journal: Vec<(String, Json)>,
    ui_requests: Vec<UiRequest>,
    notices: Vec<Notice>,
    /// The last save error, until a save works again.
    pub save_error: Option<String>,
    /// The clock (Unix milliseconds). Tests replace it.
    pub now: fn() -> i64,
}

/// `web-time` is `std::time` on native and the JS clock in the browser (where `std`'s clock panics).
#[allow(clippy::disallowed_methods)]
fn system_now() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Session {
    /// A session on an already loaded book and its store.
    pub fn new(store: Box<dyn Store>, book: Book) -> Session {
        let book = Arc::new(book);
        Session {
            saved: book.clone(),
            book,
            store,
            selection: None,
            view: ViewState::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            journal: Vec::new(),
            ui_requests: Vec::new(),
            notices: Vec::new(),
            save_error: None,
            now: system_now,
        }
    }

    /// Opens the store at `location` (a file path or `memory:`) and loads it.
    pub fn open(location: &str) -> Result<Session> {
        let mut store = pecunia_store::open(location)?;
        let loaded = store.load()?;
        let mut s = Session::new(store, loaded.book);
        s.repaired(&loaded.notes);
        Ok(s)
    }

    fn repaired(&mut self, notes: &[String]) {
        for n in notes.iter().take(3) {
            self.notify(format!("Repaired: {n}"), false);
        }
        if notes.len() > 3 {
            self.notify(format!("…and {} more repairs", notes.len() - 3), false);
        }
    }

    /// Loads `store` and makes it the open book (open, or a store an app built itself, like the web app's browser
    /// file). Undo is cleared; repairs to the stored data become notices.
    pub fn open_store(&mut self, mut store: Box<dyn Store>) -> Result<()> {
        let loaded = store.load()?;
        self.replace(Some(store), loaded.book);
        self.selection = None;
        self.repaired(&loaded.notes);
        Ok(())
    }

    /// A throwaway in-memory session holding the sample household.
    pub fn sample() -> Session {
        let book = sample::book(system_now());
        Session::new(Box::new(pecunia_store::MemoryStore::new(book.clone())), book)
    }

    /// A throwaway in-memory session with a new book with the usual categories.
    pub fn scratch() -> Session {
        let book = Book::with_starter_categories("Scratch").or_else(|_| Book::new("Scratch")).unwrap_or_else(|_| sample::book(0));
        Session::new(Box::new(pecunia_store::MemoryStore::new(book.clone())), book)
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    /// Where the book is stored, for people.
    pub fn location(&self) -> String {
        self.store.describe()
    }

    pub fn store_path(&self) -> Option<std::path::PathBuf> {
        self.store.path().map(|p| p.to_path_buf())
    }

    /// Today, from the clock.
    pub fn today(&self) -> NaiveDate {
        pecunia_model::date_of(self.clock())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|h| h.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|h| h.label.as_str())
    }

    /// Every command run so far that changed something, with its params.
    pub fn journal(&self) -> &[(String, Json)] {
        &self.journal
    }

    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    pub(crate) fn request(&mut self, r: UiRequest) {
        self.ui_requests.push(r);
    }

    pub(crate) fn notify(&mut self, text: impl Into<String>, undo: bool) {
        self.notices.push(Notice { text: text.into(), undo, error: false });
    }

    fn notify_error(&mut self, text: impl Into<String>) {
        self.notices.push(Notice { text: text.into(), undo: false, error: true });
    }

    /// Changes the book. Commands call this; it clones the book only when an undo snapshot still shares it (records
    /// themselves stay shared).
    pub(crate) fn book_mut(&mut self) -> &mut Book {
        Arc::make_mut(&mut self.book)
    }

    /// Replaces the open book (open, new, reload): clears undo and keeps the selection only if it still exists.
    pub(crate) fn replace(&mut self, store: Option<Box<dyn Store>>, book: Book) {
        if let Some(s) = store {
            self.store = s;
            self.view = ViewState::default();
        }
        let book = Arc::new(book);
        self.saved = book.clone();
        self.book = book;
        self.undo.clear();
        self.redo.clear();
        self.save_error = None;
        self.fix_references();
    }

    /// Drops a selection or filter that points at something no longer in the book.
    fn fix_references(&mut self) {
        if self.selection.is_some_and(|id| self.book.txn(id).is_none()) {
            self.selection = None;
        }
        if self.view.filter.account.is_some_and(|id| self.book.account(id).is_none()) {
            self.view.filter.account = None;
        }
    }

    /// Runs command `id`. On success the change is undoable (if the command is) and saved.
    pub fn execute(&mut self, id: &str, params: Json) -> Result<Json> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.chars().take(60).collect()))?;
        // Pick up edits from other programs first, so reads are fresh and edits land on top.
        self.poll_external();
        if let Err(why) = (spec.enabled)(self) {
            return Err(EngineError::Disabled(id.to_string(), why));
        }
        let params = if params.is_null() { Json::Object(Default::default()) } else { params };
        let before = (self.book.clone(), self.selection);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, &params)));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                self.book = before.0;
                self.selection = before.1;
                return Err(EngineError::Internal(id.to_string(), msg));
            }
        };
        if result.is_err() {
            // A failed command leaves the book as it was.
            self.book = before.0;
            return result;
        }
        if spec.kind != cmd::Kind::Query {
            self.journal.push((id.to_string(), params));
            if self.journal.len() > JOURNAL_LIMIT {
                self.journal.drain(..JOURNAL_LIMIT / 2);
            }
        }
        if spec.kind == cmd::Kind::Edit && !Arc::ptr_eq(&self.book, &before.0) && *self.book != *before.0 {
            self.undo.push(History { label: spec.label.trim_end_matches('…').to_string(), book: before.0, selection: before.1 });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.fix_references();
        self.flush()?;
        result
    }

    /// Runs a command for a caller with no UI (CLI, MCP): a command that only asked the UI for a dialog because a
    /// required param was missing is an error, and a failed save is an error.
    pub fn execute_headless(&mut self, id: &str, params: Json) -> Result<Json> {
        let r = self.execute(id, params);
        let asked = self.take_ui_requests();
        if r.is_ok()
            && let Some(UiRequest::Dialog(name, _)) = asked.first()
        {
            let what = match name.as_str() {
                "newTxn" => "`payee`, `from`, `to` and `amount`",
                "newAccount" => "`kind` and `name`",
                "import" => "`path` (or `contentBase64` and `name`)",
                "newBook" | "open" => "`path`",
                _ => "params",
            };
            return Err(EngineError::BadParams { cmd: id.into(), msg: format!("missing {what}: programmatic calls never open dialogs") });
        }
        if let Some(e) = &self.save_error {
            return Err(EngineError::Store(StoreError::Io(e.clone())));
        }
        r
    }

    /// Sends unsaved changes to the store. On a conflict the book is reloaded from the store (the other program wins)
    /// and the person is told their last change was not saved.
    pub fn flush(&mut self) -> Result<()> {
        if Arc::ptr_eq(&self.book, &self.saved) {
            return Ok(());
        }
        let changes = diff(&self.saved, &self.book);
        if changes.is_empty() {
            self.saved = self.book.clone();
            return Ok(());
        }
        match self.store.apply(&self.book, &changes) {
            Ok(()) => {
                self.saved = self.book.clone();
                self.save_error = None;
                Ok(())
            }
            Err(StoreError::Conflict) => match self.store.load() {
                Ok(l) => {
                    self.replace(None, l.book);
                    self.notify_error("Another program changed this book at the same moment. It was reloaded; your last change was not saved.");
                    Err(EngineError::Store(StoreError::Conflict))
                }
                Err(e) => self.save_failed(e),
            },
            Err(e) => self.save_failed(e),
        }
    }

    fn save_failed(&mut self, e: StoreError) -> Result<()> {
        let msg = e.to_string();
        if self.save_error.as_deref() != Some(msg.as_str()) {
            self.notify_error(format!("Couldn't save: {msg}"));
        }
        self.save_error = Some(msg);
        Err(e.into())
    }

    /// Reloads when another program changed the store and nothing here is unsaved. Returns whether it reloaded. The app
    /// calls this about once a second.
    pub fn poll_external(&mut self) -> bool {
        if !Arc::ptr_eq(&self.book, &self.saved) || !self.store.changed_externally() {
            return false;
        }
        match self.store.load() {
            Ok(l) => {
                if l.book != *self.saved {
                    self.replace(None, l.book);
                    self.notify("Updated with changes from another program", false);
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if self.save_error.as_deref() != Some(msg.as_str()) {
                    self.notify_error(format!("Couldn't read the book: {msg}"));
                }
                self.save_error = Some(msg);
                false
            }
        }
    }

    pub(crate) fn undo_step(&mut self) -> Option<String> {
        let h = self.undo.pop()?;
        let label = h.label.clone();
        self.redo.push(History { label: h.label, book: std::mem::replace(&mut self.book, h.book), selection: self.selection });
        self.selection = h.selection;
        Some(label)
    }

    pub(crate) fn redo_step(&mut self) -> Option<String> {
        let h = self.redo.pop()?;
        let label = h.label.clone();
        self.undo.push(History { label: h.label, book: std::mem::replace(&mut self.book, h.book), selection: self.selection });
        self.selection = h.selection;
        Some(label)
    }

    /// Runs a command and turns the error into a message (for UIs).
    pub fn run(&mut self, id: &str, params: Json) -> std::result::Result<Json, String> {
        self.execute(id, params).map_err(|e| e.to_string())
    }

    pub(crate) fn clock(&self) -> i64 {
        (self.now)()
    }
}

#[cfg(test)]
mod tests;
