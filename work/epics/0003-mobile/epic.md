---
status: done
created: 2026-10-16
---
# 0003 Mobile ready

## Goal
Pecunia works on a phone: the browser app at pecunia.helico.app fits a narrow screen and works with
touch, without a separate mobile app.

## Scope
A compact layout in `ui-egui` below 720 points wide (one column; the sidebar becomes a menu; a
transaction's details open full screen), screens and dialogs that reflow, a web page that suits phones
(viewport, safe areas, no page zoom on double tap). Not in scope: where a book lives on a phone
(phones can't edit local files from the browser), see issue 0008.

## Done when
`cargo xtask ci` passes, snapshots at 390×844 show every screen without overlap, a headless phone
emulation (touch, 390×844) loads the deployed app without console errors, and the web app is redeployed.

## Stories
- [01 Compact layout](01-compact-layout.md)
- [02 Screens and dialogs on a phone](02-compact-screens.md)
- [03 Web page on phones](03-web-page.md)
