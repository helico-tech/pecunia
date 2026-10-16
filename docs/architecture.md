# Pecunia architecture

> Last updated: 2026-10-16

Pecunia is a local-first household budgeting app. The bookkeeping comes from `hobby/budget.rs`
(ADR 0001, 0003); the shape comes from `helico/openissue`: a Cargo workspace of layered crates,
one command registry that every surface dispatches through, a swappable egui UI, a never-crash
policy, and one data file per book (ADR 0002).

## Layers

| Layer | Crate | Job |
|---|---|---|
| L0 | `crates/model` | `Money` (integer cents, saturating), the account tree (`AccountTree`), transactions with postings, payee rules. `Book` with validated edits (accounts, transactions, answers, splits, pot moves, rules, starting balances), `from_parts` (repairs hostile data), `diff` → `Change`s. `ledger` (booking rules, balances, transfer matching), `payees` (normalisation, similarity, import hashes), `dedup`, `ingest` (bank rows → books), `classify` (own accounts → rules → memory), `review`, `register` (rows, arrows, running balances), `reports`, `catalogue`. Dates and times are passed in; no clock, no I/O. |
| L1 | `crates/formats` | Bank-file parsers: built-in CSV profiles of Dutch banks (ING, Rabobank, ABN AMRO, ASN/SNS/RegioBank, Triodos, bunq, Knab, ICS), saved profiles, CAMT.053, MT940; `detect`, `parse`, `preview_csv`; synthetic sample-file writers. Bytes in, rows out. |
| L1 | `crates/store` | `trait Store { load, apply(book, changes), changed_externally, encrypted, set_password }`. `encode`/`decode` are the file format, with migrations (ADR 0006); `crypto` seals and opens protected books (ADR 0007). `MemoryStore` replays changes; `JsonFileStore` writes one pretty JSON file atomically (`craft-atomic`) and detects other writers by modification time and size. |
| L2 | `crates/engine` | `Session`: the open book (`Arc` snapshots), store, selection, `ViewState`, undo/redo, journal, notices, UI requests, a revision counter. `Session::execute(id, params)` runs a `CommandSpec`, catches panics, records undo, then flushes the diff to the store. `cmd/*.rs` holds the registry (`file`, `txn`, `review`, `import`, `account`, `rule`, `view`, `query`, `report`). `sample.rs` builds the made-up sample household. |
| L3 | `crates/mcp` | JSON-RPC 2.0 over stdio. `tools/list` comes from the registry (`txn.create` → `txn_create`); `tools/call` uses `execute_headless`. |
| L4 | `crates/ui-egui` | `PecuniaApp`: sidebar, Overview, Review, register, Pots, Reports, Accounts, Rules, Imports, the transaction detail pane, dialogs, palette, toasts. `theme::Tokens` (light/dark), `icons.rs` (all drawn in code), `pickers.rs`, `widgets.rs`. Examples: `snapshot` (offscreen PNG), `app_icon`. |
| apps | `apps/pecunia` | eframe/wgpu window, native macOS menu, file dialogs, prefs and recent books, default book, quit through `craft-quit`. |
| apps | `apps/pecunia-cli` | `init`, `accounts`, `add-account`, `import`, `list`, `show`, `add`, `review`, `answer`, `report`, `run`, `commands`, `mcp`. |
| apps | `apps/pecunia-web` | The same UI as wasm (https://pecunia.helico.app). The book is a local file through the File System Access API; bank files come in through the browser's file picker (ADR 0005). |
| — | `xtask` | `ci`, `layers`, `bundle [--universal] [--dmg]` (macOS `.app`, `.dmg`, CLI archive), `package-linux`, `package-windows` (the release downloads in `target/dist/`, built by `.github/workflows/release.yml` on a `vX.Y.Z` tag), `web`, `icon`. |

## The books (ADR 0003)

Groups: money you have (`have`), money you owe (`owe`), where money goes (`goes`), where money
comes from (`comes`), hidden `equity`. Physical accounts are roots of have/owe; every have account
has exactly one *Unassigned* pot; categories sit under category groups. Postings go to leaves
only and every transaction sums to zero. An unclassified bank row posts against *Not sorted yet*.

## The file format

A book is one `.pecunia` file (ADR 0002), UTF-8 JSON, pretty-printed:

```json
{
  "format": "pecunia",
  "version": 1,
  "meta": { "name": "…", "demo": false, "next_account": 41, "next_txn": 312, "next_rule": 18,
            "next_import": 8, "profiles": [ CSV layouts ], "balances": [ stated bank balances ],
            "imports": [ import log ] },
  "accounts": [ { "id", "group", "kind", "parent", "name", "iban", "unassigned", "system", "goal", … } ],
  "transactions": [ { "id", "kind", "date", "payee", "payee_norm", "account", "bank_amount",
                      "source", "import", "status", "guesses", "postings": [ { "account", "amount" } ],
                      "keys": [ { "account", "key" } ], … } ],
  "rules": [ { "id", "payee_prefix", "iban", "account", "origin", "mixed", "uses" } ]
}
```

Money is integer cents, dates are `YYYY-MM-DD`, records are sorted by number and empty fields are
left out, so the file diffs well. A protected book (ADR 0007) holds an envelope instead:
`{"format": "pecunia-encrypted", "version": 1, "kdf": {argon2id settings, salt}, "cipher":
"xchacha20poly1305", "nonce", "data"}`, where `data` is the plain document above, encrypted.

Versions (ADR 0006): `crates/store/src/file.rs` has `VERSION` and `MIGRATIONS`; `decode` migrates
older documents step by step and refuses newer ones. `crates/engine/tests/fixtures/v<N>/` holds a
frozen plain and encrypted book per version that CI opens on every run.

## Data flow

1. **Open:** `store::open(path)` → `Store::load` → `Book::from_parts` repairs anything invalid and
   reports what it fixed (shown as notices).
2. **Edit:** UI, CLI or MCP → `Session::execute(id, params)`. It first polls the store for outside
   changes, then runs the command on `Arc::make_mut(book)`. A failed command restores the book.
3. **Save:** `diff(saved, book)` → `Store::apply`. On `Conflict` the engine reloads; the other
   writer wins and the person is told.
4. **Import:** bytes (a path, or base64 from the browser) → `formats::detect` + `parse` → the
   account by the IBAN in the file (or the `account` param) → `Book::ingest`: keys (external id,
   import hash) → transfer matching → fuzzy duplicates → booked against *Not sorted yet* →
   classified (ADR 0004) → `auto` or `to_review`. Stated closing balances go to the bank check.
5. **Review:** answers (`txn.answer`) book the counter side as checked; two identical answers make
   a learned rule; `always` makes a rule at once; a new rule books waiting matches.

## Conventions

- Params: amounts in euros (`12.50`, `"12,50"`), dates `YYYY-MM-DD` or `today`, accounts by
  number, name, IBAN or path (`ING / Holiday`), transactions by number. Answers: amounts in
  integer cents.
- `view.*` and `txn.select` only steer the screen; MCP leaves them out.

## Testing

- `model`: ledger, payees, dedup, ingest, classifier, review, register, reports, repair, diff.
- `formats`: every bank format round trip, malformed and hostile files.
- `store`: round trip, external writers, hostile files.
- `engine`: command flows, undo/autosave, headless dialogs, file reload, import through the real
  parsers, reports, and `hostile_params_never_panic` (every command × junk params).
- `mcp`: protocol, tools, garbage input. `ui-egui`: egui_kittest flows and every screen rendered.
