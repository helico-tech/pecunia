# 0004. An offline classifier: own accounts, rules, memory, Review

## Status
accepted (2026-10-16)

## Context
budget.rs classifies with own accounts → rules → memory → a System One model on Ollama → Review,
on Cicada actors. Pecunia v1 is offline only (the owner's choice): no network calls, and the same
behaviour in the browser.

## Decision
- Classification runs inside the import command, synchronously, as a pure function of the book:
  1. the counterparty IBAN is another own account → Review as a likely transfer;
  2. a payee rule (prefix of the normalised payee, or the counterparty IBAN) → booked, `auto`;
  3. memory: at least two confirmed transactions with a similar payee that agree for 90% or
     more → booked, `auto`;
  4. otherwise → Review, with the two best guesses from similar history.
- Two identical answers for one payee make a learned rule; a different later answer marks a
  learned rule `mixed`, and mixed rules are skipped (as in budget.rs).
- Transaction statuses: `auto`, `to_review`, `reviewed`. There is no `classifying` state.
- The model step and new-category proposals are left out. Adding them later needs async work
  outside the command and a new ADR.

## Consequences
- Import is instant and works in every surface, also offline.
- New payees always go to Review until rules or memory know them.
