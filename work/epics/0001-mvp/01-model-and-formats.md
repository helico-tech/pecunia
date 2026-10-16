---
status: in-progress
created: 2026-10-16
---
# Model and bank-file formats

## Goal
The pure domain (L0): money, dates, the account tree, transactions with postings, validated edits
on a `Book`, repair of hostile data, diff into changes, payee normalisation, dedup, transfer
matching, ingest, the offline classifier with rule learning, Review, register queries and reports.
`formats`: CSV profiles of the Dutch banks, CAMT.053 and MT940, ported from budget.rs.

## Acceptance criteria
- Ported behaviour keeps budget.rs's tests (ledger, payees, dedup, classifier, reports, parsers).
- No `unwrap`/`expect`/`panic`/indexing on input outside tests; money never overflows.
- `Book::from_parts` repairs any file content and reports the repairs.

## Plan
1. `model`: `money`, `ids`, `account`, `txn`, `rule`, `tree` (AccountTree over the book),
   `ledger` (book_bank_row, reclassify, split, pot_move, opening, revaluation), `payees`,
   `dedup`, `classify` (own account → rules → memory), `book` (Book, edits, from_parts, diff),
   `ingest`, `review`, `register`, `reports`, `catalogue` (default categories).
2. `formats`: port `budget-adapters/src/files` (detect, CSV profiles, CAMT.053, MT940) and its
   tests, with synthetic test files only.

## Progress log
- 2026-10-16: plan written.
- 2026-10-16: `formats` ported: `built_in_profiles`, `detect`, `parse`, `preview_csv` (plain functions, no
  trait), `samples` (synthetic bank-file writers). All budget.rs parser tests kept; new tests for saved
  profiles, the preview, the amount cap and hostile input. Found and fixed: roxmltree recurses per
  nesting level, so CAMT files nested deeper than 128 are refused before parsing (stack overflow otherwise).

## Follow-ups
