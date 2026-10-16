---
status: todo
type: debt
created: 2026-10-16
---
# The web app's wasm is 10 MB

## Problem
`wasm-opt` is not installed here, so `cargo xtask web` ships the wasm unoptimised (about 10 MB,
compressed in transit). First load is slow on a phone connection.

## Notes
`brew install binaryen`, or install it in the release workflow before `cargo xtask web`.
