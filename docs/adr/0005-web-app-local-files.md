# 0005. Web app edits local files, hosted at pecunia.helico.app

## Status
accepted (2026-10-16). The same approach as OpenIssue ADR 0004.

## Context
Pecunia should also run in a browser while the data stays the person's own `.pecunia` file. Bank
files to import are local files too.

## Decision
- `apps/pecunia-web` compiles the egui UI to wasm. It opens and saves the book through the File
  System Access API (`BrowserFileStore`: bytes in memory, a background task writes them and polls
  the file for outside changes). Other browsers get the sample book in memory and a message.
- Bank files are picked with the browser's file picker and passed to `import.file` as
  `contentBase64`; the engine never reads paths in the browser.
- Hosting: static files on Cloudflare Workers with the custom domain `pecunia.helico.app`, plus a
  container image (nginx) for self-hosting. No server stores or sees data.

## Consequences
- The browser, desktop app, CLI and MCP can work on the same file at the same time.
- Firefox and Safari can't edit local files until they support the API.
