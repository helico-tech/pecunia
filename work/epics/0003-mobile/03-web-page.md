---
status: done
created: 2026-10-16
---
# Web page on phones

## Goal
The page around the canvas suits phones.

## Acceptance criteria
- `index.html`: viewport without page zoom, `100dvh` height, theme colour.
- The web DEMO banner has a short phone text.
- Headless Chromium with phone emulation (390×844, touch) loads the deployed app without console errors.

## Plan
Edit `apps/pecunia-web/static/index.html` and `web.rs` (banner); extend the headless check with a
phone viewport.

## Progress log
- 2026-10-16: viewport without page zoom (`interactive-widget=resizes-content`), `100dvh`, theme colours,
  `touch-action: none` on the canvas; short web DEMO banner. Safe-area insets tried and dropped: a canvas doesn't
  stretch between left/right, so it rendered tiny; the browser keeps the page clear of the notch on its own.
- 2026-10-16: Headless Chromium as a phone (390×844, touch, 1×): loads with no errors; tapping the menu, a screen,
  a row and Back works. At 3× the headless shell gives eframe CSS pixels for `devicePixelContentBoxSize`, so the
  UI renders 3× too big in the emulator (the old site does the same); real phones not checked yet.

## Follow-ups
- [0008 Where does a book live on a phone?](../../issues/0008-books-on-a-phone.md)
- [0009 Web font](../../issues/0009-web-font.md)
- Check on a real iPhone and Android phone (pixel ratio, keyboard, touch scrolling).
