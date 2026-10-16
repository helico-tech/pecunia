---
status: in-progress
created: 2026-10-16
---
# Web app at pecunia.helico.app

## Goal
The egui UI as wasm (ADR 0005): open, create and save a `.pecunia` file through the File System
Access API, import bank files from the browser, deploy to Cloudflare at pecunia.helico.app,
Dockerfile and k8s manifest for self-hosting.

## Acceptance criteria
- `cargo xtask web` builds `target/web/`; the deployed page loads with no console errors.

## Plan
Adapt openissue-web; add the bank-file picker (showOpenFilePicker, else <input type=file>); deploy with wrangler.

## Progress log
- 2026-10-16: web app adapted from OpenIssue (File System Access store, bank-file import through the browser picker, Dockerfile, k8s, wrangler for pecunia.helico.app); `cargo xtask web` builds; files served locally load. Next: deploy and check in a browser.

## Follow-ups
