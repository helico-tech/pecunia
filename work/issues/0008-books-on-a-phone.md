---
status: done
type: question
created: 2026-10-16
---
# Where does a book live on a phone?

## Problem
The browser app edits a `.pecunia` file on your computer through the File System Access API
(ADR 0005). Safari on iOS has no such API, and it is unclear whether Chrome on Android has the file
pickers (`showOpenFilePicker`, `showSaveFilePicker`). Where the API is missing, the app shows the
DEMO household only, not saved. The layout itself is phone ready (epic 0003).

## Notes
Options:
- **Do nothing:** a phone shows the demo; you keep books on a computer. Costs nothing.
- **Open and download:** open a book with a plain file input, save by downloading a new copy.
  Works in every browser, but there is no autosave and you get many copies.
- **Keep the book in the browser** (OPFS, the browser's private file storage), with Export and
  Import. Real autosave on the phone; but the browser may clear it, and the book lives apart from
  your other copies, so it needs a clear backup story.
- First check on a real Android phone what Chrome supports (feature detection is already in
  `js/fs.js`).

- 2026-10-17: The owner asked about a PWA or an iOS app, then chose "installable web app + book kept in the
  browser". Done as ADR 0009: IndexedDB books behind the same handle API, a chooser for them, open-from-file
  copies a book in, `file.saveCopy` (all apps) to take one out, manifest + service worker for home screen and
  offline. Checked in headless Chrome without the file API, at phone size with touch: create, edit, reload,
  offline reload, open from a file, chooser, download. Not checked on a real iPhone or Android phone.
  A native iOS app stays an idea for later.
