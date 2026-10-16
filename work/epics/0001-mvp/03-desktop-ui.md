---
status: done
created: 2026-10-16
---
# Desktop UI

## Goal
The egui front end and the desktop app, in OpenIssue's look: sidebar (book, accounts with
balances, pots, review count, reports), toolbar, a virtualised register, the transaction detail
pane, Overview, Review, Pots, Accounts and categories, Reports, the import and new-transaction
dialogs, command palette, toasts, light and dark themes. Native macOS menu, file dialogs, prefs.

## Acceptance criteria
- Snapshots of every screen in light and dark look right at 1440×900.
- egui_kittest tests for the main flows (select a row, answer Review, import dialog).
- `cargo xtask bundle` makes a launchable Pecunia.app.

## Plan
Shell, register, Review, detail pane and dialogs first (one hand, one style); then the Overview, Pots, Reports and Accounts screens in parallel worktrees; desktop app adapted from OpenIssue's.

## Progress log
- 2026-10-16: Overview, Pots, Reports and Accounts screens built by subagents and merged; desktop app (menu, file dialogs, prefs, drop to open or import) adapted from OpenIssue; `cargo xtask bundle` makes a launchable Pecunia.app (launched headless, default book created). `cargo xtask ci` green.
- 2026-10-16: shell, sidebar, register, Review, detail pane, dialogs (incl. import with CSV mapping), palette, Rules and Imports done and checked in snapshots. Overview, Pots, Reports and Accounts screens and the desktop app are being built by subagents in worktrees.

## Follow-ups
