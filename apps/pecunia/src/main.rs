// Windows: a release build is a GUI app with no console window behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Pecunia desktop app.
//!
//! `pecunia [path.pecunia]` opens that book; with no path, the last one used, or a default book
//! in the app-data folder. `--sample` opens the sample household in memory.

#[cfg(target_os = "macos")]
mod native_menu;
mod prefs;

use std::path::{Path, PathBuf};

use pecunia_engine::store::FILE_EXTENSION;
use pecunia_engine::store::StoreError;
use pecunia_engine::{EngineError, Session};
use pecunia_ui_egui::{FileRequest, ImportFile, PecuniaApp, ThemeMode};
use serde_json::json;

/// Extensions of the bank files Pecunia reads: CSV of Dutch banks, CAMT.053 (XML) and MT940.
const BANK_FILE_EXTENSIONS: &[&str] = &["csv", "txt", "xml", "sta", "940", "tab"];

/// "Any file" for the file dialog. macOS merges all filters into one list of allowed types, where
/// the type `public.data` lets every file through.
const ANY_FILE: &str = if cfg!(target_os = "macos") { "public.data" } else { "*" };

struct App {
    app: PecuniaApp,
    prefs: prefs::Prefs,
    title: String,
    #[cfg(target_os = "macos")]
    menu: Option<native_menu::NativeMenu>,
}

impl App {
    fn remember_current(&mut self) {
        if let Some(p) = self.app.session.store_path() {
            prefs::remember(&mut self.prefs, &p.display().to_string());
            self.app.ui.recent = self.prefs.recent.clone();
            self.save_prefs();
        }
    }

    fn save_prefs(&mut self) {
        self.prefs.theme = self.app.ui.theme.id().to_string();
        self.prefs.sidebar = Some(self.app.ui.sidebar);
        prefs::save(&self.prefs);
    }

    /// Native file dialogs the UI asked for.
    fn file_requests(&mut self) {
        let Some(req) = self.app.ui.file_request.take() else { return };
        let start = self.app.session.store_path().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(prefs::data_dir);
        match req {
            FileRequest::Open => {
                let mut d = rfd::FileDialog::new().set_title("Open book").add_filter("Pecunia book", &[FILE_EXTENSION]);
                if let Some(s) = &start {
                    d = d.set_directory(s);
                }
                if let Some(path) = d.pick_file() {
                    self.open(&path.display().to_string());
                }
            }
            FileRequest::New(params) => {
                let name = params.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()).unwrap_or("Household").to_string();
                let mut d = rfd::FileDialog::new()
                    .set_title("New book")
                    .set_file_name(format!("{name}.{FILE_EXTENSION}"))
                    .add_filter("Pecunia book", &[FILE_EXTENSION]);
                if let Some(s) = &start {
                    d = d.set_directory(s);
                }
                if let Some(mut path) = d.save_file() {
                    if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXTENSION) {
                        path.set_extension(FILE_EXTENSION);
                    }
                    if path.exists() {
                        // Never overwrite a file; the person picks another name.
                        self.app.toast(format!("{} already exists. Choose another name, or open it instead.", path.display()), false, true);
                        return;
                    }
                    let mut p = params.clone();
                    p["path"] = json!(path.display().to_string());
                    if self.app.run("file.new", p) {
                        self.remember_current();
                    }
                }
            }
            FileRequest::SaveCopy => {
                let stem = self.app.session.store_path().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()));
                let name = stem.unwrap_or_else(|| self.app.session.book().name().to_string());
                let mut d = rfd::FileDialog::new()
                    .set_title("Save a copy")
                    .set_file_name(format!("{name} copy.{FILE_EXTENSION}"))
                    .add_filter("Pecunia book", &[FILE_EXTENSION]);
                if let Some(s) = &start {
                    d = d.set_directory(s);
                }
                if let Some(mut path) = d.save_file() {
                    if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXTENSION) {
                        path.set_extension(FILE_EXTENSION);
                    }
                    self.app.run("file.saveCopy", json!({ "path": path.display().to_string() }));
                }
            }
            FileRequest::Import => {
                // Bank files are usually in Downloads, not next to the book.
                let d = rfd::FileDialog::new()
                    .set_title("Import bank file")
                    .add_filter("Bank file (CSV, CAMT.053, MT940)", BANK_FILE_EXTENSIONS)
                    .add_filter("All files", &[ANY_FILE]);
                if let Some(path) = d.pick_file() {
                    self.app.open_import(ImportFile::Path(path.display().to_string()));
                }
            }
        }
    }

    fn open(&mut self, path: &str) {
        if self.app.run("file.open", json!({ "path": path })) {
            self.remember_current();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.menu.is_none() && std::env::var_os("PECUNIA_NO_NATIVE_MENU").is_none() {
                self.menu = Some(native_menu::NativeMenu::install(ctx));
            }
            if let Some(m) = &self.menu {
                m.poll(&mut self.app, ctx);
            }
        }
        let before = self.app.session.store_path();
        self.app.logic(ctx);
        self.file_requests();
        if self.app.session.store_path() != before {
            self.remember_current();
        }
        let title = format!("{} — Pecunia", self.app.session.book().name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        // Everything is saved after each change, so closing needs no prompt.
        craft_quit::on_close_request(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // A book dropped on the window opens; any other file is imported as a bank file.
        for f in std::mem::take(&mut raw.dropped_files) {
            let path = f.path().to_path_buf();
            if path.as_os_str().is_empty() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(FILE_EXTENSION)) {
                self.open(&path.display().to_string());
            } else {
                self.app.open_import(ImportFile::Path(path.display().to_string()));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }

    fn on_exit(&mut self) {
        self.save_prefs();
    }
}

/// The book to start with: the path given, else the last one used, else the default book
/// (created on first run).
/// How the app starts: a session, a problem to show, and a protected book to ask the password for.
struct Start {
    session: Session,
    problem: Option<String>,
    locked: Option<String>,
}

fn start_session(args: &[String], prefs: &prefs::Prefs) -> Start {
    let start = |session, problem| Start { session, problem, locked: None };
    if args.iter().any(|a| a == "--sample" || a == "--demo") {
        return start(Session::sample(), None);
    }
    let given = args.iter().find(|a| !a.starts_with("--")).cloned();
    let mut problem = None;
    if let Some(p) = given.or_else(|| prefs.recent.first().cloned()) {
        match Session::open(&p) {
            Ok(s) => return start(s, None),
            // A protected book: show an empty book behind the password dialog.
            Err(EngineError::Store(StoreError::Locked)) => return Start { session: Session::scratch(), problem: None, locked: Some(p) },
            Err(e) => problem = Some(format!("Couldn't open {p}: {e}")),
        }
    }
    match default_book() {
        Ok(s) => start(s, problem),
        Err(e) => {
            let msg =
                format!("{}Couldn't open the default book: {e}. Changes are kept in memory only.", problem.map(|p| p + ". ").unwrap_or_default());
            start(Session::scratch(), Some(msg))
        }
    }
}

/// `My Book.pecunia` in the app-data folder, with the usual household categories.
fn default_book() -> Result<Session, String> {
    let dir = prefs::data_dir().ok_or("no home folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path: PathBuf = dir.join(format!("My Book.{FILE_EXTENSION}"));
    let p = path.display().to_string();
    if path.exists() {
        return Session::open(&p).map_err(|e| e.to_string());
    }
    let mut s = Session::scratch();
    s.execute("file.new", json!({ "path": p, "name": "My Book", "categories": true })).map_err(|e| e.to_string())?;
    Ok(s)
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prefs = prefs::load();
    let Start { session, problem, locked } = start_session(&args, &prefs);
    let mut app = PecuniaApp::new(session);
    app.ui.theme = ThemeMode::parse(&prefs.theme).unwrap_or_default();
    app.ui.sidebar = prefs.sidebar.unwrap_or(true);
    app.ui.recent = prefs.recent.clone();
    if let Some(p) = problem {
        app.toast(p, false, true);
    }
    if let Some(path) = locked {
        app.open_dialog("unlock", json!({ "path": path }));
    }
    app.route();
    let mut state = App {
        app,
        prefs,
        title: String::new(),
        #[cfg(target_os = "macos")]
        menu: None,
    };
    state.remember_current();

    let viewport = egui::ViewportBuilder::default()
        .with_title("Pecunia")
        .with_app_id("nl.helico.pecunia")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([880.0, 560.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Pecunia", options, Box::new(move |_cc| Ok(Box::new(state))))
}
