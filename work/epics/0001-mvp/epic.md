---
status: in-progress
created: 2026-10-16
---
# 0001 MVP: local-first budgeting in the OpenIssue style

## Goal
A working Pecunia: the bookkeeping of budget.rs (ledger with pots, bank-file import, dedup,
rules-and-memory classifier, Review, reports) as a local-first app built like OpenIssue: one
`.pecunia` file, a command registry used by the desktop app, the CLI, MCP and the browser.

## Scope
Offline core only (ADR 0004): no Ollama model, no Enable Banking. Desktop (macOS first), CLI,
MCP, web app at pecunia.helico.app, private GitHub repo with CI.

## Done when
`cargo xtask ci` passes; the desktop app, CLI, MCP and web app open, import, review and report on
a book; the UI has been checked through snapshots; the repo is on GitHub with green CI; the web
app is live at https://pecunia.helico.app.

## Stories
- [01 Model and bank-file formats](01-model-and-formats.md)
- [02 Store, engine and sample book](02-store-engine.md)
- [03 Desktop UI](03-desktop-ui.md)
- [04 CLI and MCP](04-cli-mcp.md)
- [05 Web app at pecunia.helico.app](05-web-app.md)
- [06 GitHub repo and CI](06-repo-ci.md)
