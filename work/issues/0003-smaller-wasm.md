---
status: done
type: debt
created: 2026-10-16
---
# The web app's wasm is 10 MB

## Problem
`wasm-opt` is not installed here, so `cargo xtask web` ships the wasm unoptimised (about 10 MB,
compressed in transit). First load is slow on a phone connection.

## Notes
`brew install binaryen`, or install it in the release workflow before `cargo xtask web`.

- 2026-10-17: Fixed. `cargo xtask web` now needs wasm-opt and builds the wasm for size (opt-level s, fat LTO,
  one codegen unit): 10.5 MB → 6.2 MB (2.7 MB gzipped), with the bundled font. The release job and the
  Dockerfile install binaryen version_133.
