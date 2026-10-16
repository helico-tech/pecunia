# Pecunia

Household bookkeeping that stays on your computer, in pure Rust.

- **Friendly double entry with pots.** Money you have (with pots like *Holiday* or *Bills*), money
  you owe, where it goes and where it comes from. No debits or credits on screen: every
  transaction reads as *ING → Groceries*.
- **Import your bank's files.** CSV exports of ING, Rabobank, ABN AMRO, ASN, SNS, RegioBank,
  Triodos, bunq, Knab and ICS, plus CAMT.053 and MT940. Map any other CSV once. Importing the same
  file again only adds what is new; both copies of a transfer between your own accounts become one
  transaction; a likely duplicate from another file waits for you.
- **It learns your answers, offline.** Your own accounts → payee rules → similar past answers →
  Review. Two identical answers make a rule. Nothing leaves your machine.
- **Reports:** net worth, free to spend, income and spending, cash flow, fixed costs (with price
  changes), and a bank check against the balances your bank states.
- **One book is one `.pecunia` file.** Every change saves on its own, with undo. If another
  program changes the file (the CLI, an agent over MCP, a sync tool), the app reloads it.
- **The same commands everywhere:** the desktop app, the browser app, `pecunia-cli`, and MCP tools
  for agents.

## In the browser

Open **https://pecunia.helico.app** in Chrome or Edge. Pick a `.pecunia` file on your computer
(or create one) and every change is saved straight back to that file; nothing is stored on a
server. Bank files are read in the browser. Firefox and Safari can't edit local files yet, so
there you get the sample household, not saved.

## Build and run

You need Rust 1.95 or later (`rustup update`) and, for the `.app`, the Xcode Command Line Tools.

```sh
cargo run --release -p pecunia -- --sample           # the made-up sample household, in memory
cargo run --release -p pecunia -- home.pecunia       # open (or start with) a book
cargo xtask bundle && open target/release/bundle/Pecunia.app
```

### Command line

```sh
cargo install --path apps/pecunia-cli
pecunia-cli init home.pecunia --name "Our household"
pecunia-cli add-account home.pecunia --kind current --name "ING" --iban NL12INGB0001234567 --balance 1250,00 --date 2026-09-30
pecunia-cli import home.pecunia ~/Downloads/ing-export.csv --dry-run
pecunia-cli import home.pecunia ~/Downloads/ing-export.csv
pecunia-cli review home.pecunia
pecunia-cli answer home.pecunia 42 Groceries --always
pecunia-cli list home.pecunia --account ING --from 2026-10-01
pecunia-cli report home.pecunia income --from 2026-09-01 --to 2026-09-30
pecunia-cli commands                                  # every command; `run --cmd 'id={json}'` runs any of them
```

### Use it from Claude Code (MCP)

```sh
claude mcp add pecunia -- pecunia-cli mcp /absolute/path/to/home.pecunia
```

Every engine command becomes a tool (`book_info`, `review_list`, `txn_answer`, `import_file`,
`report_overview`, …). Amounts you send are euros; amounts in answers are integer cents.

### Keyboard

⌘N new transaction · ⌘I import · ⌘K command palette · ⌘F search · ⌘1–⌘6 screens · ↑/↓ or J/K
move · ⌘↩ mark as checked · Esc close · ⌘Z/⇧⌘Z undo/redo · ⌘⌫ delete · ⌘\ sidebar.

### Develop

```sh
cargo xtask ci        # fmt, clippy -D warnings, all tests, layering rules
cargo run -p pecunia-ui-egui --example snapshot -- --sample --dark out.png   # render the UI offscreen
```

Architecture: [docs/architecture.md](docs/architecture.md). Decisions: [docs/adr/](docs/adr/).
Agent rules: [AGENTS.md](AGENTS.md). The bookkeeping is ported from budget.rs; the app shape follows
OpenIssue.
