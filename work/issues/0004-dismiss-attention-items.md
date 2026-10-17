---
status: done
type: idea
created: 2026-10-16
---
# Dismiss items in Needs attention

## Problem
Price changes, new and stopped fixed costs and own-account candidates stay on the Overview until
the data changes. budget.rs let people answer them ("seen", "someone else's account").

## Notes
Needs a place in the book for the answers (meta) and commands; then filter in `needs_attention`.

- 2026-10-17: Done. Fixed-cost news gets a "Seen" button, an own-account question "Not mine". The answers are keys in
  `meta.dismissed` (an optional field: older apps may drop it, then the items show again; ADR 0006 needs no new
  version). A price change or missed payment asks again when it changes. Commands `attention.dismiss {key}` and
  `attention.restore`; `report.overview` gives each item its `key`. Bank differences and Review cannot be dismissed.
