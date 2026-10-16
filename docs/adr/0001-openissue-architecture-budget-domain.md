# 0001. OpenIssue's architecture, budget.rs's bookkeeping

## Status
accepted (2026-10-16)

## Context
Pecunia is a local-first household budgeting app. Its bookkeeping comes from `hobby/budget.rs`
(a server app: PostgreSQL, axum, React, Cicada actors). Its shape should be the same as
`helico/openissue`: a pure-Rust desktop app that keeps its data in one file, with a CLI, an MCP
server and a browser build, all running the same commands. OpenIssue itself follows the ArtCraft
"Craft" apps (ADR 0001 there).

## Decision
- Use OpenIssue's layers: `model` → `store` → `engine` (command registry, `Session`) → `mcp` /
  `ui-egui` → apps (`pecunia`, `pecunia-cli`, `pecunia-web`), checked by `cargo xtask layers`.
  Bank-file parsing is its own crate, `formats` (L1, on top of `model`).
- Every user-visible action is a `CommandSpec`. Production crates deny `unwrap`, `expect`,
  `panic` and indexing on input; `unsafe` is forbidden. Icons are drawn in code.
- Port budget.rs's pure domain: money, the ledger with pots (ADR 0003), payee normalisation,
  dedup, transfer matching, the rules-and-memory classifier, reports and the CSV/CAMT.053/MT940
  parsers. Leave out what needs a server: PostgreSQL, Cicada actors, HTTP routes, the React UI.
- Reuse `craft-atomic` and `craft-quit` from storytold/craft-libs, pinned by commit, as OpenIssue
  does.

## Consequences
- The app, CLI and MCP behave the same, and an agent can do the bookkeeping over MCP.
- budget.rs's money code panics on overflow; here amounts are capped on input and arithmetic
  saturates, to keep the never-crash rule.
- budget.rs keeps working as it is; the two apps do not share code or files.
