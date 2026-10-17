# 0009. Books kept in the browser, and an installable web app

## Status
accepted (2026-10-17). Extends ADR 0005; that ADR still holds where the browser can edit files.

## Context
The web app edits a `.pecunia` file through the File System Access API (ADR 0005). Phones (Safari
on iOS, Chrome on Android), desktop Safari and Firefox lack its file pickers, so there the app only
showed the demo. The owner wants Pecunia usable on a phone without an app store. A native iOS app
was weighed and left for later: it needs a paid developer account, signing and store review.

## Decision
- Where the file pickers are missing, books are kept in the browser's own storage: IndexedDB,
  database `pecunia`, store `books`, `{ bytes, modified }` by file name. The bytes are exactly a
  `.pecunia` file (encrypted when protected, ADR 0007). IndexedDB rather than OPFS because every
  browser has it and Safari's OPFS writes need a worker.
- `js/fs.js` gives such a book a "browser handle" `{ browser: true, name }` that the same read,
  write and stat functions accept, so `web.rs` and `BrowserFileStore` treat it like a file: same
  autosave, same check for changes from another tab, same password flow.
- New book: kept under its name, never over an existing one. Open book: a list of the books kept
  here, or a file from the device, which is copied in (as "name (2)" when the name is taken).
- Getting a book out: `file.saveCopy` ("Save a Copy…", every app) writes the book as stored to a
  new file; the browser offers it as a download. The new-book dialog says to do this now and then.
- The page asks for persistent storage (`navigator.storage.persist()`), so the browser doesn't
  clear it under storage pressure. Installed home-screen apps keep their data on iOS.
- The web app is installable: `manifest.webmanifest`, icons, and a service worker (`sw.js`) that
  keeps the release's files in a cache so the app starts offline. `cargo xtask web` writes the file
  list and a cache name that changes with every build. Books never pass through the worker.

## Consequences
- A phone can keep real books, offline, with nothing on a server.
- A book kept in the browser lives on that device only, apart from copies elsewhere; losing the
  browser's data loses it. Save a copy is the backup; there is no sync.
- Browsers with the File System Access API keep editing files as before; they don't use the
  browser's storage.
