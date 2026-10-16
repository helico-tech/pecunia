# 0002. One household book in one `.pecunia` file

## Status
accepted (2026-10-16)

## Context
budget.rs keeps the books in PostgreSQL. A local-first app keeps them where the person decides:
a file on their disk, in a synced folder or a private repo. OpenIssue stores a workspace as one
JSON file behind a change-based `Store` trait (its ADRs 0002 and 0003), and that works in the
desktop app, the CLI, MCP and the browser.

## Decision
- A book is one pretty-printed JSON file with extension `.pecunia`: `format`, `version`, `meta`
  (name, counters, CSV profiles, stated bank balances, import log), `accounts`, `transactions`
  and `rules`, each sorted by number. Writes are atomic (`craft-atomic`).
- The engine sends a list of `Change`s (meta, upsert or delete of an account, transaction or rule)
  to the `Store`; the file backend rewrites the file. `MemoryStore` replays the changes in tests.
- Autosave after every command. Other writers win: if the file changed since the last read, the
  engine reloads and tells the person their last change was not saved (OpenIssue ADR 0003).
- Dedup keys live on the transaction that owns them (`keys`: account + key). Import builds its
  lookup once per file; there is no stored index.
- Loading repairs what breaks the rules (unbalanced or orphaned postings go to *Not sorted yet*,
  duplicate ids are dropped, counters move past the data) and reports each repair.

## Consequences
- A book with 20,000 transactions is a few MB. A write rewrites it all, in milliseconds.
- The file is readable and diffs well, but it holds bank data: people should keep it private.
- Real-time merging of two writers is not covered.
