---
status: done
created: 2026-10-16
---
# Compact layout

## Goal
Below 720 points wide the app shows one thing at a time instead of three squeezed columns.

## Acceptance criteria
- `UiState.compact` is set every frame from the window width (below 720 points).
- Compact: a top bar with a menu button, the book name and New; the sidebar opens full screen from the
  menu button and closes when you pick something.
- Compact: a selected transaction's details replace the screen, with a Back button.
- Compact: short DEMO banner and status bar text; toasts fit the width.
- kittest: a narrow window shows the menu button and no sidebar; picking a screen closes the menu.

## Plan
`lib.rs` decides the layout: wide = today's three panels; compact = top bar + one central view (menu,
detail or screen). Sidebar rows close the menu. Glyphs `Menu` and `ChevronLeft`.

## Progress log
- 2026-10-16: `COMPACT_WIDTH` 720, `UiState.compact`/`menu`, `is_compact(ctx)`; top bar (Menu, book, DEMO, New);
  sidebar as full-screen menu (rows 40 pt high; picking anything closes it and clears the selection); detail
  replaces the screen with Back; short DEMO banner and status bar; toasts fit. Tablet: detail takes the list's
  place when the list would get < 440 pt. Glyphs `Menu`, `ChevronLeft`. Snapshot `--menu`. kittest
  `a_phone_gets_one_view_at_a_time`, `a_wide_window_has_no_phone_menu`.

## Follow-ups
