//! App preferences: theme, recent books, the last one open. One small JSON file in the
//! platform's app-data folder, written atomically.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: String,
    pub recent: Vec<String>,
    pub sidebar: Option<bool>,
}

/// `~/Library/Application Support/Pecunia` on macOS, `%APPDATA%\Pecunia` on Windows,
/// `$XDG_DATA_HOME/pecunia` (or `~/.local/share/pecunia`) elsewhere.
/// `PECUNIA_DATA_DIR` overrides it (tests, portable installs).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("PECUNIA_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Pecunia"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Pecunia"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("pecunia"))
    }
}

fn file() -> Option<PathBuf> {
    data_dir().map(|d| d.join("prefs.json"))
}

pub fn load() -> Prefs {
    file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(p: &Prefs) {
    let Some(f) = file() else { return };
    if let Some(dir) = f.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("can't create {}: {e}", dir.display());
        return;
    }
    match serde_json::to_vec_pretty(p) {
        Ok(bytes) => {
            if let Err(e) = craft_atomic::atomic_write(&f, &bytes) {
                log::warn!("can't save preferences: {e}");
            }
        }
        Err(e) => log::warn!("can't encode preferences: {e}"),
    }
}

/// Puts `path` first in the recent list (at most 10, no duplicates).
pub fn remember(p: &mut Prefs, path: &str) {
    p.recent.retain(|r| r != path);
    p.recent.insert(0, path.to_string());
    p.recent.truncate(10);
}
