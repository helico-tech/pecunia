//! macOS: the native menu bar. Items run the same command ids as the palette and shortcuts: the
//! engine commands that name a menu, plus the UI commands. Shortcuts egui also needs inside text
//! fields or dialogs (⌘Z, ⌘C, ⌘⌫, ⌘↩…) have no menu accelerator, so the key reaches the focused
//! field first; the menu items still work by mouse.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use muda::accelerator::Accelerator;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use pecunia_engine::command_specs;
use pecunia_ui_egui::{PecuniaApp, UI_COMMANDS};
use serde_json::json;

/// (label, command id, accelerator). Label `-` is a separator.
type Entry = (&'static str, &'static str, Option<String>);

/// The menus, in this order; an engine menu not listed here comes after them.
const MENUS: &[&str] = &["File", "Edit", "View", "Transaction", "Review", "Account"];

/// Where the UI-only commands go, after the menu's engine commands.
const UI_PLACES: &[(&str, &[&str])] = &[
    ("File", &["-", "ui.rename"]),
    ("Edit", &["-", "clip.cut", "clip.copy", "clip.paste", "-", "ui.search", "ui.palette"]),
    ("View", &["-", "ui.toggleSidebar", "-", "ui.theme.system", "ui.theme.light", "ui.theme.dark"]),
    ("Account", &["ui.newPot", "ui.newCategory"]),
];

/// Edit-menu items egui handles itself (labels; no accelerator, see the module docs).
const CLIPBOARD: &[(&str, &str)] = &[("clip.cut", "Cut"), ("clip.copy", "Copy"), ("clip.paste", "Paste")];

/// Commands whose shortcut must reach a text field or dialog first.
const NO_ACCELERATOR: &[&str] = &["edit.undo", "edit.redo", "txn.delete", "txn.confirm"];

/// `⇧⌘N` → `SHIFT+CMD+N`, as muda reads it.
fn accelerator(shortcut: &str) -> Option<String> {
    let parts: Vec<String> = shortcut
        .chars()
        .map(|c| match c {
            '⌘' => "CMD".to_string(),
            '⇧' => "SHIFT".to_string(),
            '⌥' => "ALT".to_string(),
            '⌃' => "CTRL".to_string(),
            '↩' => "ENTER".to_string(),
            '⌫' => "BACKSPACE".to_string(),
            '\\' => "BACKSLASH".to_string(),
            other => other.to_uppercase().to_string(),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

fn accel_for(id: &str, shortcut: Option<&str>) -> Option<String> {
    if NO_ACCELERATOR.contains(&id) {
        return None;
    }
    shortcut.and_then(accelerator)
}

pub fn tree() -> Vec<(&'static str, Vec<Entry>)> {
    let mut menus: Vec<&'static str> = MENUS.to_vec();
    for spec in command_specs() {
        if let Some(m) = spec.menu.first()
            && !menus.contains(m)
        {
            menus.push(m);
        }
    }
    menus
        .into_iter()
        .map(|menu| {
            let mut entries: Vec<Entry> =
                command_specs().iter().filter(|s| s.menu.first() == Some(&menu)).map(|s| (s.label, s.id, accel_for(s.id, s.shortcut))).collect();
            for (_, ids) in UI_PLACES.iter().filter(|(m, _)| *m == menu) {
                for id in ids.iter().copied() {
                    if id == "-" {
                        if !entries.is_empty() {
                            entries.push(("-", "", None));
                        }
                    } else if let Some((_, label)) = CLIPBOARD.iter().find(|(c, _)| *c == id) {
                        entries.push((label, id, None));
                    } else if let Some((_, label, sc)) = UI_COMMANDS.iter().find(|(c, _, _)| *c == id) {
                        entries.push((label, id, accel_for(id, *sc)));
                    }
                }
            }
            (menu, entries)
        })
        .filter(|(_, entries)| !entries.is_empty())
        .collect()
}

pub struct NativeMenu {
    _menu: Menu,
    events: Receiver<MenuEvent>,
}

impl NativeMenu {
    pub fn install(ctx: &egui::Context) -> NativeMenu {
        let menu = Menu::new();
        let app_menu = Submenu::new("Pecunia", true);
        let _ = app_menu.append_items(&[
            &PredefinedMenuItem::about(Some("About Pecunia"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(craft_quit::QUIT_MENU_ID, "Quit Pecunia", true, Accelerator::from_str("CMD+Q").ok()),
        ]);
        let _ = menu.append(&app_menu);
        for (title, entries) in tree() {
            let sub = Submenu::new(title, true);
            for (label, id, acc) in entries {
                if label == "-" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                    continue;
                }
                let accel = acc.and_then(|a| Accelerator::from_str(&a).ok());
                let _ = sub.append(&MenuItem::with_id(id, label, true, accel));
            }
            let _ = menu.append(&sub);
        }
        let window = Submenu::new("Window", true);
        let _ =
            window.append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::maximize(None), &PredefinedMenuItem::fullscreen(None)]);
        let _ = menu.append(&window);
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { _menu: menu, events: rx }
    }

    pub fn poll(&self, app: &mut PecuniaApp, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            let id = ev.id.as_ref().to_string();
            if craft_quit::on_menu_event(ctx, &id) {
                continue;
            }
            match id.as_str() {
                "clip.cut" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
                "clip.copy" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
                "clip.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
                // Inside a text field, Undo/Redo belong to the field.
                "edit.undo" | "edit.redo" if ctx.text_edit_focused() => ctx.input_mut(|i| {
                    i.events.push(egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers { command: true, mac_cmd: true, shift: id == "edit.redo", ..Default::default() },
                    })
                }),
                other => {
                    app.run_any(other, json!({}));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_runs_a_known_command_and_every_accelerator_parses() {
        let menus = tree();
        assert_eq!(menus.first().map(|m| m.0), Some("File"));
        let mut ids = Vec::new();
        for (_, entries) in &menus {
            for (label, id, acc) in entries {
                if *label == "-" {
                    continue;
                }
                let known = pecunia_engine::find_command(id).is_some()
                    || UI_COMMANDS.iter().any(|(c, _, _)| c == id)
                    || CLIPBOARD.iter().any(|(c, _)| c == id);
                assert!(known, "{id} is not a command");
                if let Some(a) = acc {
                    assert!(Accelerator::from_str(a).is_ok(), "{id}: {a}");
                }
                ids.push(*id);
            }
        }
        for id in ["file.new", "file.open", "import.file", "edit.undo", "ui.palette", "ui.theme.dark", "clip.paste"] {
            assert!(ids.contains(&id), "{id} is missing from the menu");
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "an item appears twice");
    }

    #[test]
    fn shortcuts_become_accelerators() {
        assert_eq!(accelerator("⇧⌘N").as_deref(), Some("SHIFT+CMD+N"));
        assert_eq!(accelerator("⌘\\").as_deref(), Some("CMD+BACKSLASH"));
        assert_eq!(accel_for("txn.delete", Some("⌘⌫")), None);
    }
}
