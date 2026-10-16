---
status: done
created: 2026-10-16
---
# Password encryption

## Goal
ADR 0007 in every surface.

## Acceptance criteria
- Round trip, wrong password, tampered header and tampered data tested; hostile envelopes never
  panic.
- Desktop and web ask for the password; CLI and MCP read `PECUNIA_PASSWORD` or
  `--password-file`.

## Plan
`store::crypto` (argon2 + chacha20poly1305, randomness passed in); `Store::set_key`;
`JsonFileStore` and the web `BrowserFileStore` encrypt with a kept key; engine `password`
params, `file.setPassword`, `file.removePassword`, `UiRequest` "unlock"; UI dialogs.

## Progress log
- 2026-10-16: `store::crypto` (Argon2id 19 MiB/2/1, XChaCha20-Poly1305, header as AAD, bounded KDF settings from files); desktop and browser stores keep the key; engine `password` params, `file.setPassword`/`file.removePassword`, unlock dialog request; passwords redacted from the journal; UI unlock, set-password and New book fields; desktop asks at startup; web unlock flow and encrypted new files with `crypto.getRandomValues`; CLI `PECUNIA_PASSWORD`/`--password-file`, `init --encrypt`, `password`. Tested in store, engine, web store and by hand with the CLI. Not checked: the browser unlock dialog in a real browser with a real file (issue 0006).

## Follow-ups
