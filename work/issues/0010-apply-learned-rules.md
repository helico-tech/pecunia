---
status: done
type: bug
created: 2026-10-17
---
# Rules made in Review don't book the other waiting transactions

## Problem
A rule made from Review answers (two identical answers make a learned rule; "always" makes one at
once) only affects later imports. Transactions of the same payee already waiting in Review stay
there. A rule made or edited on the Rules screen does book them (`Book::apply_rules`).

## How to reproduce
Import two or more bank rows of one payee. In Review, answer one with "always" (or answer two the
same way): the toast says "New payee rule", but the others are still in Review.

## Notes
`Book::answer` (`crates/model/src/edit.rs`) creates the rule but does not call `apply_rules`;
`txn.answer` (`crates/engine/src/cmd/txn.rs`) only reports it. Fix: run `apply_rules` after a
created rule and say how many it booked ("New payee rule: X → Y, booked 3 waiting"). Undo already
covers it, since it is one command.

- 2026-10-17: Fixed. `txn.answer`, `review.acceptSuggestions` and `review.answerAllLike` run `apply_rules` after
  answering and say how many it booked; one undo takes it all back. Test: `rules_made_in_review_book_the_waiting_ones`.
