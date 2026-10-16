# 0008. One UI with a phone layout

## Status
accepted (2026-10-16)

## Context
People open pecunia.helico.app on a phone. The UI was built for a window with a sidebar, a list and a
detail pane side by side; at 390 points wide the sidebar filled the screen and text overlapped. A
separate mobile app would double the UI work.

## Decision
- The same egui UI adapts to the width. Below `COMPACT_WIDTH` (720 points) `UiState.compact` is set and
  the app shows one view at a time: a top bar with a menu button, the book and New; the sidebar opens
  as a full-screen menu; a selected transaction's details replace the list, with Back.
- Screens and dialogs check `app.ui.compact` (or `crate::is_compact(ctx)` where they have no app) and
  reflow: rows that held a table get two lines, toolbars wrap or drop extras, dialogs never get
  wider than the screen and scroll when taller, keyboard hints are hidden.
- Between phone and desktop (a tablet), the detail takes the list's place when the list would get
  less than 440 points.
- `cargo run -p pecunia-ui-egui --example snapshot -- --sample --size 390x844 [--menu] out.png` shows
  the phone layout; a kittest test drives it.

## Consequences
- Every new screen or dialog must read well at 390×844 as well as on the desktop.
- Where a phone keeps a book is a separate question (issue 0008): phones' browsers can't edit local
  files the way desktop Chrome can.
