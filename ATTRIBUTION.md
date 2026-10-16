# Attribution

Every asset file in this repository, with its author, source and license.

| File | Author | Source | License |
|---|---|---|---|
| `packaging/macos/Pecunia.icns` | Pecunia contributors | Rendered from `crates/ui-egui/src/icons.rs::app_icon` by `cargo xtask icon` | Same as the code |
| `packaging/linux/pecunia.png` | Pecunia contributors | Rendered from `crates/ui-egui/src/icons.rs::app_icon` by `cargo xtask icon` | Same as the code |

All other icons are drawn in code (`crates/ui-egui/src/icons.rs`). Fonts are not bundled; the app
uses the operating system's fonts, with egui's built-in fonts as a fallback.
