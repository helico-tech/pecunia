# Pecunia — instructions for agents

Pecunia is a local-first household budgeting app in pure Rust: a friendly double-entry ledger with
pots, bank-file import (Dutch CSV layouts, CAMT.053, MT940) with dedup and transfer matching, an
offline classifier (own accounts → payee rules → memory → Review) and reports. The bookkeeping is
ported from `hobby/budget.rs`; the shape (layered crates, command registry, thin egui UI, one
data file) follows `helico/openissue`. A book is one `.pecunia` JSON file.

## Build, run, test
- `cargo xtask ci` — fmt, clippy `-D warnings`, all tests, layering. Run before every commit.
- `cargo run -p pecunia -- [file.pecunia | --sample]` — the desktop app.
- `cargo run -p pecunia-cli -- help` — the CLI; `pecunia-cli mcp <file>` is the MCP server.
- `cargo xtask bundle [--universal] [--dmg]` — `target/release/bundle/Pecunia.app`; `--dmg` also writes the
  `.dmg` and CLI archive to `target/dist/`. `cargo xtask package-linux` / `package-windows` — the Linux tarball
  and Windows zip (run on that OS). Release: [below](#releases). `cargo xtask web` — the browser app in
  `target/web/` (deploy: `npx wrangler deploy --config apps/pecunia-web/wrangler.toml`).
- **Look at UI changes:** `cargo run -p pecunia-ui-egui --example snapshot -- --sample [--dark] [--cmd 'id={json}'] out.png`, then read the PNG.
- `PECUNIA_DATA_DIR=/tmp/x` keeps the app's prefs and default book out of your home folder while testing.

## Layers (enforced by `cargo xtask layers`)
| Layer | Crate | Job |
|---|---|---|
| L0 | `model` | Money, accounts, transactions, rules, validated edits on `Book`, repair, diff, payees, dedup, ingest, classifier, Review, register, reports. No clock, no I/O. |
| L1 | `formats` | Bank-file parsers: CSV profiles, CAMT.053, MT940. Bytes in, rows out. |
| L1 | `store` | The `Store` trait and its backends (memory, JSON file). |
| L2 | `engine` | `Session`: undo, autosave, external-change reload, **command registry**, view state, sample book. |
| L3 | `mcp` | MCP server: every command as a tool. |
| L4 | `ui-egui` | The egui front end (swappable). |
| apps | `pecunia`, `pecunia-cli`, `pecunia-web` | Desktop app, CLI, browser app (wasm, edits a local file). |

Details: [docs/architecture.md](docs/architecture.md). Decisions: [docs/adr/](docs/adr/).

## Rules
- **Money is integer cents** (`Money`, `i64`); amounts are capped on input and arithmetic saturates.
- **Everything is a command.** User-visible behaviour is a `CommandSpec` in `crates/engine/src/cmd/*.rs`
  with tests. The UI, palette, menu, CLI and MCP dispatch by id. Programmatic calls never open dialogs.
- **Never crash.** No `unwrap`/`expect`/`panic!`/indexing on input in non-test code (crate roots deny
  them; `unsafe` is forbidden). Files, bank files, command params and MCP input are hostile.
  `engine::tests::hostile_params_never_panic` must stay green.
- **File format changes follow ADR 0006.** A change an older app would misread bumps `VERSION`, appends a
  migration to `MIGRATIONS` (`crates/store/src/file.rs`) and writes new fixtures (`cargo test -p pecunia-engine
  --test file_versions -- --ignored write_fixtures`). Never edit an old fixture or migration.
- **Passwords** (ADR 0007): never stored, logged or kept in the journal (the engine redacts `password` params); the
  CLI and MCP read `PECUNIA_PASSWORD` or `--password-file`.
- **Storage goes through `Store` only.** **The UI is thin**: it reads `Session` and acts through commands.
- **No network.** No transaction data leaves the machine (ADR 0004). Tests and samples use synthetic
  bank data only; never commit real bank exports.

## Releases
- Bump `version` in the root `Cargo.toml`, commit, then `git tag vX.Y.Z && git push --tags`.
  `.github/workflows/release.yml` builds the universal macOS `.dmg`, the macOS CLI archive, the Linux
  x86_64/arm64 tarballs and the Windows x86_64 zip, and publishes them as a GitHub Release with
  `SHA256SUMS.txt` and `.github/release-notes.md`. The tag must match the version.
- The builds are not signed or notarised yet (issue 0007). A tag also publishes the web app's image
  (ghcr.io/helico-tech/pecunia-web) and deploys pecunia.helico.app when the Cloudflare secrets are set.
- The app icon files in `packaging/` are committed (CI has no GPU to render them); after changing
  the app icon, run `cargo xtask icon` on a Mac and commit the result.

## Work
Work items and decisions live in the repo: `work/queue.md` (next up), `work/epics/`, `work/issues/`,
and `docs/adr/`. Start a session by reading `work/queue.md`.
