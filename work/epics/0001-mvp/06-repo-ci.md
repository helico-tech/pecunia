---
status: done
created: 2026-10-16
---
# GitHub repo and CI

## Goal
Private repo github.com/helico-tech/pecunia with CI (`cargo xtask ci` on macOS and Linux, wasm
clippy) and the release workflow copied from OpenIssue (not tagged until the owner asks).

## Acceptance criteria
- CI is green on `main`.

## Plan
gh repo create helico-tech/pecunia --private; push main; watch CI.

## Progress log
- 2026-10-16: CI green on Linux, macOS and Windows (run 38894826299).
- 2026-10-16: workflows adapted from OpenIssue (ci on macOS, Linux, Windows; release on v* tags only). Next: create the private repo and push.

## Follow-ups
