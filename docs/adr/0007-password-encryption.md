# 0007. Password encryption of book files

## Status
accepted (2026-10-16)

## Context
A book holds a household's bank history. It sits on disk, in sync folders or in a repo, so it
should be possible to protect it with a password. It must keep working in every surface
(desktop, browser, CLI, MCP) and stay local (ADR 0004): no key server, no account.

## Decision
- Optional per book. A protected book is still one `.pecunia` file, holding an envelope:
  `{ "format": "pecunia-encrypted", "version": 1, "kdf": { "name": "argon2id", "m_kib", "t",
  "p", "salt" }, "cipher": "xchacha20poly1305", "nonce", "data" }` (binary values base64). `data`
  is the encrypted plain book document (ADR 0006), so versions and migrations work the same.
- The key comes from the password with Argon2id (19 MiB, 2 passes, 1 lane; a random 16-byte
  salt per password). Each save uses a fresh random 24-byte nonce. The header is bound to the
  ciphertext as associated data, so it can't be swapped.
- The password is never stored. The derived key is kept in memory while the book is open, so
  saving stays fast, and is wiped when it is dropped.
- A wrong password and a damaged file give the same error ("wrong password, or the file is
  damaged"). There is no recovery: a lost password means a lost book (the app says so).
- Commands: `file.open` and `file.new` take `password`; `file.setPassword` sets or changes it;
  `file.removePassword` stores the book in the clear again. The desktop and web apps ask with a
  dialog; the CLI and MCP read `PECUNIA_PASSWORD` or `--password-file`.
- Randomness comes from the host: the operating system on desktop and the CLI,
  `crypto.getRandomValues` in the browser.

## Consequences
- An encrypted file doesn't diff in git, and other tools can't read it.
- Opening takes a moment (the key derivation, on purpose).
- Older Pecunia versions (before encryption) refuse an encrypted file with a clear message.
