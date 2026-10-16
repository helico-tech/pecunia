# 0006. File versions and migrations

## Status
accepted (2026-10-16)

## Context
A `.pecunia` file lives for years and new app versions will change the model. Every new version
must open every older file (backwards compatible). An older app cannot know what a newer one
added, so it must refuse a newer file rather than damage it.

## Decision
- The plain book document carries `format: "pecunia"` and `version` (the file version, now 1).
- `decode` reads the document as JSON first, refuses a version newer than the app's, then runs
  migrations one version at a time (`v1 → v2 → …`, each a function over the JSON) before it
  turns the JSON into the model. `encode` always writes the current version.
- A change needs a version bump and a migration when an older app would misread the file or lose
  meaning by ignoring it (renamed or removed fields, changed meaning or units, new required
  data). A new optional field with a default that an older app may drop without harm does not.
- Every file version has a frozen fixture under `crates/engine/tests/fixtures/` (plain and
  encrypted) and a test that opens it with no repairs. Fixtures are never edited; a new version
  adds a new fixture.
- When a file is migrated, the first save keeps the original next to it as
  `<name>.pecunia.v<old>.bak`, so going back to an older app stays possible.
- The encryption envelope (ADR 0007) has its own version, separate from the book's.

## Consequences
- Old files keep opening; the fixtures prove it in CI.
- After a migration the file only opens in the new app (the backup does in the old one).
- Model changes cost a migration and a fixture; that is the price of years-long files.
