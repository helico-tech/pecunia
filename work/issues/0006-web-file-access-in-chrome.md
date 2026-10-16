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
