---
status: todo
type: question
created: 2026-10-16
---
# Check the web app's file access in a real Chrome

## Problem
The deployed app was checked headless: it loads the sample with no console errors. Opening and
saving a `.pecunia` file and picking a bank file need a real browser with the File System
Access API and a person to pick files.

## Notes
Open https://pecunia.helico.app in Chrome: New file…, import a bank export, reload, reopen.

- 2026-10-17: Still needs a person. Driving Chrome from here failed: the screen-control helper has no
  Accessibility or Screen Recording permission, and Playwright cannot click the native save dialog.
  Also check on a real iPhone and Android phone (ADR 0009): start a book, edit, close and reopen, Add to Home
  Screen, open offline, Save a copy.
