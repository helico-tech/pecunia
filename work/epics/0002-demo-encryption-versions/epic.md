---
status: done
created: 2026-10-16
---
# 0002 Demo label, password encryption, file versions

## Goal
The first view people see is clearly a demo; books can be protected with a password; every new
version opens every older file.

## Scope
Demo flag and banner (all surfaces), ADR 0007 encryption (store, engine, desktop, web, CLI, MCP),
ADR 0006 versions and migrations with frozen fixtures.

## Done when
`cargo xtask ci` passes, fixtures of v1 (plain and encrypted) open in tests, the web app is
redeployed, and the owner's question about the file format is answered.

## Stories
- [01 Demo label](01-demo.md)
- [02 File versions and migrations](02-versions.md)
- [03 Password encryption](03-encryption.md)
