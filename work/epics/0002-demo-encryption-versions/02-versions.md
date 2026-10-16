---
status: done
created: 2026-10-16
---
# File versions and migrations

## Goal
ADR 0006: decode through migrations, refuse newer files, back up the original on first save,
frozen fixtures per version.

## Acceptance criteria
- Migration steps run in order; tests prove it with test steps.
- `crates/engine/tests/fixtures/v1/*.pecunia` open with no repairs, forever.

## Plan
`store::migrate` over `serde_json::Value`; `Loaded.migrated_from`; `JsonFileStore` writes
`<name>.pecunia.v<old>.bak` once. Fixtures generated once from the demo book and committed.

## Progress log
- 2026-10-16: `decode` migrates through `MIGRATIONS` (tested with test steps), refuses newer files, backs up the original on first save after a migration (code path not reachable until v2, so not exercised yet). Frozen v1 fixtures (plain + encrypted) in `crates/engine/tests/fixtures/v1/`, opened by `tests/file_versions.rs`.

## Follow-ups
