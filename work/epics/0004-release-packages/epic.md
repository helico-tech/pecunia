---
status: done
created: 2026-10-16
---
# 0004 Release packages for Mac, Linux and Windows

## Goal
A `vX.Y.Z` tag builds the downloads for macOS, Linux and Windows and publishes them as a GitHub
Release, the same way OpenIssue and OpenTeams do.

## Scope
`cargo xtask bundle [--universal] [--dmg]`, `package-linux`, `package-windows`;
`.github/workflows/release.yml` (macOS universal, Linux x86_64 + arm64, Windows x86_64, publish with
`SHA256SUMS.txt`); release notes; README download section; AGENTS.md and architecture notes.
The web app's image and Cloudflare jobs in the same workflow stay as they are. Not in scope:
signing and notarising (issue 0007).

## Done when
`cargo xtask ci` passes, `cargo xtask bundle --universal --dmg` writes the `.dmg` and the CLI
archive on a Mac, and the workflow YAML parses. The Linux and Windows packages are checked by the
first release run.

## Stories
- [01 Release packages](01-release-packages.md)
