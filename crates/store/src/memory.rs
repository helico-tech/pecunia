use pecunia_model::{Book, Change};

use crate::{Loaded, Result, Store};

/// Keeps the book in memory only: for tests, the CLI's scratch runs and the sample.
pub struct MemoryStore {
    book: Book,
    /// How many times `apply` ran; tests use it.
    pub applies: usize,
}

impl MemoryStore {
    pub fn new(book: Book) -> MemoryStore {
        MemoryStore { book, applies: 0 }
    }
}

impl Store for MemoryStore {
    fn describe(&self) -> String {
        "memory".into()
    }

    fn load(&mut self) -> Result<Loaded> {
        Ok(Loaded { book: self.book.clone(), notes: Vec::new() })
    }

    fn apply(&mut self, _book: &Book, changes: &[Change]) -> Result<()> {
        self.book.apply(changes);
        self.applies += 1;
        Ok(())
    }
}
