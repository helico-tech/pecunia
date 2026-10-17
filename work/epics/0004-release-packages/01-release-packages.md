---
status: done
created: 2026-10-16
---
# Release packages

## Goal
Mac, Linux and Windows downloads built by `cargo xtask` and published by a GitHub workflow on a
`vX.Y.Z` tag, exactly as in OpenIssue and OpenTeams.

## Acceptance criteria
- `cargo xtask bundle [--universal] [--dmg]` builds `Pecunia.app` and, with `--dmg`,
  `target/dist/Pecunia-<version>-macos-<arch>.dmg` and `pecunia-cli-<version>-macos-<arch>.tar.gz`.
- `cargo xtask package-linux [--target T]` builds `pecunia-<version>-linux-<arch>.tar.gz` with both
  binaries, `pecunia.desktop`, `pecunia.png` and the README; `cargo xtask package-windows` builds
  `pecunia-<version>-windows-<arch>.zip` with both `.exe` files and the README. Each refuses to run
  on another OS.
- `.github/workflows/release.yml`: jobs `macos` (universal), `linux` (x86_64 + arm64), `windows`,
  `publish` (only on a tag that matches the workspace version; `SHA256SUMS.txt`;
  `gh release create` with `.github/release-notes.md`). Same action versions as OpenIssue; the
  Linux build dependencies match `ci.yml`. The existing web image and Cloudflare jobs stay.
- README has a Download section (private repo, unsigned-build steps); AGENTS.md and
  docs/architecture.md name the commands and the release steps.

## Plan
Compare Pecunia's xtask, workflows, release notes and packaging with OpenIssue and OpenTeams; fill
the gaps; build the Mac packages locally; parse the workflow YAML.

## Progress log
- 2026-10-16: Found most of it already in place from the first commit (344c0fc): the xtask
  `bundle --universal --dmg`, `package-linux`, `package-windows`, `packaging/` (icns, png,
  .desktop), `.github/release-notes.md` and `release.yml` are line-for-line OpenIssue's with
  Pecunia names. OpenIssue and OpenTeams have no Windows exe icon or version resource (no
  build.rs, .rc or winres), so none was added.
- 2026-10-16: Added the README Download section, the
  AGENTS.md build line and Releases section, and the release commands in docs/architecture.md.
- 2026-10-16: Verified on a Mac: `cargo xtask bundle --universal --dmg` writes
  `Pecunia-0.1.0-macos-universal.dmg` (UDZO, 22 MB) and `pecunia-cli-0.1.0-macos-universal.tar.gz`;
  both binaries are x86_64 + arm64, Info.plist lints, the app starts with `--sample` and the bundled
  `pecunia-cli help` runs. Workflow YAML parses; `cargo xtask ci` passes. `hdiutil create` prints a
  deprecation warning on this macOS (same in OpenIssue); it still works. `package-linux` and `package-windows`
  compile (runtime `cfg!` guards) but are unverified until the first release run on CI.

- 2026-10-16: First run of `release.yml` (manual, run 38908219883): macOS universal, Linux x86_64 and arm64,
  Windows x86_64 all built; artifacts 25, 13, 13 and 10 MB. Publish, image and deploy jobs skipped (no tag).

## Follow-ups
- [0007 Sign and notarise the builds](../../issues/0007-sign-and-notarise.md)
- First tagged release (`v0.1.0`): the owner decides when.

- 2026-10-17: v0.2.0 published four v0.1.0 packages and the container job's build record too: the restored build
  cache held old `target/dist` files, and download-artifact took every artifact. Fixed by hand on the release
  (assets removed, SHA256SUMS rebuilt) and in the workflow: caches skip `target/dist`, publish downloads only the
  package artifacts and drops files of other versions.
