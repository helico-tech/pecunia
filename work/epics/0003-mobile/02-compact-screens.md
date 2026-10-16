---
status: done
created: 2026-10-16
---
# Screens and dialogs on a phone

## Goal
Every screen and dialog reads well at 390 points wide.

## Acceptance criteria
- Overview, Review, Transactions, Pots, Reports, Accounts, Rules and Imports: no overlapping text at
  390×844 (snapshot), headers wrap instead of overlap.
- Dialogs and the palette are never wider than the screen and scroll when taller.
- Touch targets in compact lists are at least 40 points high where rows are tapped.

## Plan
Use `app.ui.compact` in each screen's header and row layout; `dialogs::fit` caps widths; two-column
dialog rows stack when compact.

## Progress log
- 2026-10-16: Register (title, own search row, short filters), Review (sideways tabs, quick answer on a second
  line), Pots (title row, card header), Reports (tiles one per line, share rows with a thin bar, two-line Cash flow
  and Fixed costs), Accounts (short title, no captions), Rules (note under the payee). Dialogs: `fit` caps widths,
  two-column rows stack, tall dialogs scroll; palette fits. Checked every screen and report at 390×844, dark too.

## Follow-ups
