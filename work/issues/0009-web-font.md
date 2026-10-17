---
status: done
type: bug
created: 2026-10-16
---
# The browser app uses egui's default font, without → and other symbols

## Problem
`theme::font_definitions` loads the system UI font from disk (SF, Segoe UI, Noto). The browser has no
disk, so the web app falls back to egui's built-in font. It looks different from the desktop app, and
symbols it lacks, such as → in "ING → Groceries", show as an empty box.

## How to reproduce
Open https://pecunia.helico.app, go to All transactions: the second line of each row shows
"ING Betaalrekening ☐ Groceries".

## Notes
Embed one open font with the symbols we use (e.g. Inter or Noto Sans, regular + semibold, subset)
for the web build only; it adds about 100–300 KB to the download. Or replace → with a drawn arrow
where it appears.

- 2026-10-17: Fixed. The web build bundles Inter 4.1 Regular and SemiBold (OFL), subset to Latin, punctuation,
  arrows and key symbols (about 70 KB each, `crates/ui-egui/assets/fonts/`); the desktop keeps the system font.
  The one symbol Inter lacks (▸) is replaced by →. Checked headless: → and ⌘ render.
