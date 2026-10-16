---
status: todo
created: 2026-10-16
---
# Store, engine and sample book

## Goal
`store`: the `Store` trait, `MemoryStore`, `JsonFileStore` (`.pecunia`), `encode`/`decode`.
`engine`: `Session` with undo, autosave, external reload, the command registry, view state, and
a sample household book (synthetic, after budget.rs's demo household).

## Acceptance criteria
- Every user-visible action is a command with tests; `hostile_params_never_panic` covers all.
- A file edited by two sessions behaves as in OpenIssue (other writers win).

## Plan
Commands: `file.*`, `book.rename`, `account.*`, `pot.*`, `txn.*`, `import.*`, `review.*`,
`rule.*`, `report.*`, `view.*`, `edit.undo/redo`, queries (`book.info`, `txn.get`,
`txns.list`, `accounts.list`, `review.list`).

## Progress log

## Follow-ups
