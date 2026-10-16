---
status: done
created: 2026-10-16
---
# Demo label

## Goal
Nobody mistakes the made-up sample household for their own book.

## Acceptance criteria
- The sample book is named "Demo household" and carries `meta.demo`.
- Every screen shows a DEMO banner while a demo book is open, with New book and Open book.
- The sidebar shows a DEMO badge on the book.

## Plan
`BookMeta.demo` (optional field, ADR 0006: no bump); `sample::book` sets it; banner in
`ui-egui` above the content; the web app's own notice keeps only the browser message.

## Progress log
- 2026-10-16: `meta.demo`, "Demo household", DEMO banner (UI crate; the web app draws its own with its file actions), DEMO badge in the sidebar, status bar says the in-memory demo is not saved. Desktop `--demo` (alias of `--sample`). kittest + snapshot checked.

## Follow-ups
