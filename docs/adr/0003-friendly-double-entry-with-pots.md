# 0003. Friendly double entry with pots

## Status
accepted (2026-10-16). Taken over from budget.rs ADR 0003.

## Context
The household wants real bookkeeping (net worth and reports fall out of the data), friendlier than
GnuCash, with physical accounts that hold pots. budget.rs decided this with the owner and built it.

## Decision
- One account tree. Groups: `have` (money you have), `owe` (money you owe), `goes` (where money
  goes), `comes` (where money comes from) and a hidden `equity`. Kinds: `current`, `savings`,
  `cash`, `property` (have), `credit_card`, `loan` (owe), `pot` (child of a have account),
  `category_group` and `category` (goes/comes), `system` (equity).
- Every have account has exactly one automatic *Unassigned* pot that takes new money. Postings go
  to leaves only. A physical balance is the sum of its pots.
- A transaction is postings that sum to zero. Positive means money into that account.
- System accounts: *Starting balance*, *Value changes*, *Not sorted yet*. An unclassified bank row
  posts against *Not sorted yet*; Review empties it.
- A transfer between own accounts is one transaction; the other bank's copy is matched to it and
  only adds its dedup keys.
- The UI never says debit or credit. It shows arrows (*ING → Groceries*) and the plain groups.

## Consequences
- No envelope budgeting: a pot with a goal is the piggy bank.
- Reports are pure functions over postings.
- Splits (mortgage = interest + repayment) are just more postings.
