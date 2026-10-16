//! The browser shell: starts eframe on the page's canvas, turns the UI's file requests into the
//! browser's file pickers, and runs the task that writes the open file and watches it for changes
//! made by other programs.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use pecunia_engine::Session;
use pecunia_engine::store::{self, FILE_EXTENSION};
use pecunia_ui_egui::{FileRequest, ImportFile, PecuniaApp, ThemeMode};
use serde_json::{Value as Json, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::file_store::{BrowserFileStore, SharedRef, Stamp, lock};

#[wasm_bindgen(module = "/js/fs.js")]
extern "C" {
    fn supported() -> bool;
    #[wasm_bindgen(catch, js_name = pickOpen)]
    async fn pick_open() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pickSave)]
    async fn pick_save(name: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pickBankFile)]
    async fn pick_bank_file() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = allowWrite)]
    async fn allow_write(handle: &JsValue, ask: bool) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = readFile)]
    async fn read_file(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = statFile)]
    async fn stat_file(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = writeFile)]
    async fn write_file(handle: &JsValue, bytes: &[u8]) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn remember(handle: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = lastHandle)]
    async fn last_handle() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn sleep(ms: u32) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = setTitle)]
    fn set_title(title: &str);
    fn ready();
    fn failed(message: &str);
    #[wasm_bindgen(js_name = loadTheme)]
    fn load_theme() -> String;
    #[wasm_bindgen(js_name = saveTheme)]
    fn save_theme(id: &str);
}

const RELEASES: &str = "https://github.com/helico-tech/pecunia/releases";
const REPO: &str = "https://github.com/helico-tech/pecunia";

/// Starts the app on `<canvas id="app">`.
pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Warn).ok();
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("app"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        let Some(canvas) = canvas else {
            failed("Pecunia couldn't start: the page has no canvas.");
            return;
        };
        let runner = eframe::WebRunner::new();
        match runner.start(canvas, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx))))).await {
            Ok(()) => ready(),
            Err(e) => failed(&format!("Pecunia couldn't start: {}. It needs a browser with WebGL 2.", js_error(&e))),
        }
    });
}

/// What an async task reports back to the frame loop.
enum Event {
    /// A file was read; open it. `note` is shown as a toast.
    Opened {
        handle: JsValue,
        name: String,
        bytes: Vec<u8>,
        stamp: Stamp,
        note: Option<String>,
    },
    /// The file used last time, which needs the person's permission before it can be opened.
    Last {
        handle: JsValue,
        name: String,
    },
    /// A bank file was picked; show the import dialog for it.
    Import {
        name: String,
        bytes: Vec<u8>,
    },
    Failed(String),
}

type Inbox = Rc<RefCell<Vec<Event>>>;

struct App {
    app: PecuniaApp,
    ctx: egui::Context,
    inbox: Inbox,
    /// The open file, while one is open (otherwise the in-memory sample book is shown).
    file: Option<SharedRef>,
    /// The file to offer in the banner ("Reopen …").
    last: Option<(JsValue, String)>,
    supported: bool,
    title: String,
    /// The appearance last saved to the browser.
    theme: ThemeMode,
    shown_write_error: Option<String>,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let supported = supported();
        let mut pecunia = PecuniaApp::new(Session::sample());
        pecunia.ui.theme = ThemeMode::parse(&load_theme()).unwrap_or_default();
        let theme = pecunia.ui.theme;
        pecunia.ui.footer_link = Some(("Pecunia on GitHub".into(), REPO.into()));
        pecunia.ui.can_pick_files = supported;
        let app = App {
            app: pecunia,
            ctx: ctx.clone(),
            inbox: Rc::default(),
            file: None,
            last: None,
            supported,
            title: String::new(),
            theme,
            shown_write_error: None,
        };
        if app.supported {
            app.spawn(async {
                let handle = last_handle().await.ok().filter(|h| !h.is_null() && !h.is_undefined())?;
                let name = js_string(&handle, "name");
                // Browsers that keep permission between visits open it straight away.
                if allow_write(&handle, false).await.ok().and_then(|v| v.as_bool()) == Some(true) {
                    Some(read(handle, None).await)
                } else {
                    Some(Event::Last { handle, name })
                }
            });
        }
        app
    }

    /// Runs `task` in the background; its event (if any) is handled on a later frame.
    fn spawn(&self, task: impl Future<Output = Option<Event>> + 'static) {
        let inbox = self.inbox.clone();
        let ctx = self.ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(e) = task.await {
                inbox.borrow_mut().push(e);
                ctx.request_repaint();
            }
        });
    }

    fn open_picker(&self) {
        self.spawn(async {
            match pick_open().await {
                Ok(h) if h.is_null() => None,
                Ok(h) => Some(open_with_permission(h).await),
                Err(e) => Some(Event::Failed(format!("Couldn't open the file: {}", js_error(&e)))),
            }
        });
    }

    /// Asks for a bank file and reads it. Works in every browser; the file is only read.
    fn import_picker(&self) {
        self.spawn(async {
            match pick_bank_file().await {
                Ok(f) if f.is_null() || f.is_undefined() => None,
                Ok(f) => Some(Event::Import { name: js_string(&f, "name"), bytes: js_bytes(&f) }),
                Err(e) => Some(Event::Failed(format!("Couldn't read the bank file: {}", js_error(&e)))),
            }
        });
    }

    fn reopen(&self, handle: JsValue) {
        self.spawn(async move { Some(open_with_permission(handle).await) });
    }

    /// Asks where to save a new book, writes it there and opens it.
    fn new_file(&self, params: Json) {
        let now = (self.app.session.now)();
        self.spawn(async move {
            let name = params.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()).unwrap_or("Household").to_string();
            let handle = match pick_save(&format!("{name}.{FILE_EXTENSION}")).await {
                Ok(h) if h.is_null() => return None,
                Ok(h) => h,
                Err(e) => return Some(Event::Failed(format!("Couldn't create the file: {}", js_error(&e)))),
            };
            let file_name = js_string(&handle, "name");
            // The save dialog lets people pick an existing file; never overwrite one.
            if let Ok(st) = stat_file(&handle).await
                && js_number(&st, "size") > 0.0
            {
                return Some(Event::Failed(format!("{file_name} already exists. Choose another name, or open it instead.")));
            }
            let bytes = match pecunia_engine::cmd::new_book(&params, &name, now).and_then(|b| Ok(store::encode(&b)?)) {
                Ok(b) => b,
                Err(e) => return Some(Event::Failed(e.to_string())),
            };
            if allow_write(&handle, true).await.ok().and_then(|v| v.as_bool()) != Some(true) {
                return Some(Event::Failed("The browser didn't allow writing the new file.".into()));
            }
            match write_file(&handle, &bytes).await {
                Ok(st) => {
                    remember(&handle).await.ok();
                    Some(Event::Opened { handle, name: file_name, bytes, stamp: stamp(&st), note: None })
                }
                Err(e) => Some(Event::Failed(format!("Couldn't write {file_name}: {}", js_error(&e)))),
            }
        });
    }

    fn handle_events(&mut self) {
        let events: Vec<Event> = std::mem::take(&mut *self.inbox.borrow_mut());
        for e in events {
            match e {
                Event::Opened { handle, name, bytes, stamp, note } => {
                    let (store, shared) = BrowserFileStore::new(&name, bytes, Some(stamp));
                    match self.app.session.open_store(Box::new(store)) {
                        Ok(()) => {
                            self.app.ui.buffers = Default::default();
                            self.file = Some(shared.clone());
                            self.last = None;
                            self.shown_write_error = None;
                            wasm_bindgen_futures::spawn_local(writer(handle, shared, self.ctx.clone()));
                            if let Some(n) = note {
                                self.app.toast(n, false, true);
                            }
                        }
                        Err(e) => self.app.toast(format!("Couldn't open {name}: {e}"), false, true),
                    }
                    self.app.route();
                }
                Event::Last { handle, name } => self.last = Some((handle, name)),
                Event::Import { name, bytes } => {
                    let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    self.app.open_import(ImportFile::Content { name, base64 });
                    self.app.route();
                }
                Event::Failed(msg) => self.app.toast(msg, false, true),
            }
        }
    }

    /// Shows a failed background write once, and saves again when writing works again.
    fn watch_writes(&mut self) {
        let Some(shared) = &self.file else { return };
        let (error, idle) = {
            let s = lock(shared);
            (s.write_error.clone(), s.pending.is_none())
        };
        if error.is_some() && error != self.shown_write_error {
            self.app.toast(format!("Couldn't save to the file: {}", error.clone().unwrap_or_default()), false, true);
        }
        self.shown_write_error = error.clone();
        if error.is_none() && idle && self.app.session.save_error.is_some() {
            self.app.session.flush().ok();
            self.app.route();
        }
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        if self.file.is_some() {
            return;
        }
        let t = self.app.tokens();
        let mut action = None;
        egui::Panel::top("web-banner")
            .resizable(false)
            .frame(egui::Frame::new().fill(t.accent_soft).inner_margin(egui::Margin::symmetric(12, 7)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if !self.supported {
                        ui.label(
                            egui::RichText::new(
                                "This browser can't edit files on your computer, so this is a sample book that isn't saved. Use Chrome or Edge, or the desktop app.",
                            )
                            .color(t.text),
                        );
                        ui.hyperlink_to("Download Pecunia", RELEASES);
                        return;
                    }
                    ui.label(egui::RichText::new("Sample book: changes are not saved. Your book is a file on your own computer.").color(t.text));
                    if ui.button("Open file…").clicked() {
                        action = Some("open");
                    }
                    if ui.button("New file…").clicked() {
                        action = Some("new");
                    }
                    if let Some((_, name)) = &self.last
                        && ui.button(format!("Reopen {name}")).clicked()
                    {
                        action = Some("reopen");
                    }
                    ui.hyperlink_to("Source on GitHub", REPO);
                });
            });
        match action {
            Some("open") => self.open_picker(),
            Some("new") => {
                self.app.run("file.new", json!({}));
            }
            Some("reopen") => {
                if let Some((h, _)) = self.last.clone() {
                    self.reopen(h);
                }
            }
            _ => {}
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        self.app.logic(ctx);
        self.watch_writes();
        match self.app.ui.file_request.take() {
            // Bank files are only read, which every browser can do.
            Some(FileRequest::Import) => self.import_picker(),
            Some(_) if !self.supported => self.app.toast("This browser can't open files on your computer. Use Chrome or Edge.", false, true),
            Some(FileRequest::Open) => self.open_picker(),
            Some(FileRequest::New(params)) => self.new_file(params),
            None => {}
        }
        if self.app.ui.theme != self.theme {
            self.theme = self.app.ui.theme;
            save_theme(self.theme.id());
        }
        let title = format!("{} — Pecunia", self.app.session.book().name());
        if title != self.title {
            set_title(&title);
            self.title = title;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.banner(ui);
        self.app.ui(ui);
    }
}

/// Asks for write permission (the browser shows a prompt), then reads the file. Without
/// permission the file opens anyway; saving then fails and says so.
async fn open_with_permission(handle: JsValue) -> Event {
    let writable = allow_write(&handle, true).await.ok().and_then(|v| v.as_bool()) == Some(true);
    let note = (!writable).then(|| "The browser didn't allow saving to this file, so changes won't be saved.".to_string());
    let e = read(handle.clone(), note).await;
    if matches!(e, Event::Opened { .. }) {
        remember(&handle).await.ok();
    }
    e
}

async fn read(handle: JsValue, note: Option<String>) -> Event {
    match read_file(&handle).await {
        Ok(f) => Event::Opened { handle, name: js_string(&f, "name"), bytes: js_bytes(&f), stamp: stamp(&f), note },
        Err(e) => Event::Failed(format!("Couldn't read {}: {}", js_string(&handle, "name"), js_error(&e))),
    }
}

/// Writes queued bytes and polls the file about once a second for changes by other programs.
/// Stops when the store is dropped (another file was opened), after writing what is pending.
async fn writer(handle: JsValue, shared: SharedRef, ctx: egui::Context) {
    let mut tick: u32 = 0;
    loop {
        sleep(250).await.ok();
        tick = tick.wrapping_add(1);
        let pending = lock(&shared).pending.take();
        if let Some(bytes) = pending {
            // Never write over a change another program just made: that one wins (ADR 0002).
            if outside_change(&handle, &shared).await {
                ctx.request_repaint();
                continue;
            }
            match write_file(&handle, &bytes).await {
                Ok(st) => {
                    let mut s = lock(&shared);
                    s.stamp = Some(stamp(&st));
                    s.write_error = None;
                }
                Err(e) => {
                    {
                        let mut s = lock(&shared);
                        if s.pending.is_none() {
                            s.pending = Some(bytes);
                        }
                        s.write_error = Some(js_error(&e));
                    }
                    ctx.request_repaint();
                    sleep(2000).await.ok();
                }
            }
            ctx.request_repaint();
            continue;
        }
        if lock(&shared).closed {
            return;
        }
        if tick.is_multiple_of(4) && outside_change(&handle, &shared).await {
            ctx.request_repaint();
        }
    }
}

/// Whether another program changed the file since our last read or write. If so, its bytes are
/// loaded into `shared` (and anything queued here is dropped), so the engine reloads them.
async fn outside_change(handle: &JsValue, shared: &SharedRef) -> bool {
    let Ok(st) = stat_file(handle).await else { return false };
    if Some(stamp(&st)) == lock(shared).stamp {
        return false;
    }
    let Ok(f) = read_file(handle).await else { return false };
    let bytes = js_bytes(&f);
    let mut s = lock(shared);
    s.stamp = Some(stamp(&f));
    s.bytes = bytes;
    s.pending = None;
    s.external = true;
    true
}

fn stamp(v: &JsValue) -> Stamp {
    (js_number(v, "modified"), js_number(v, "size"))
}

/// The `bytes` (a `Uint8Array`) of a file object from `fs.js`.
fn js_bytes(v: &JsValue) -> Vec<u8> {
    js_sys::Reflect::get(v, &"bytes".into()).map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default()
}

fn js_number(v: &JsValue, key: &str) -> f64 {
    js_sys::Reflect::get(v, &key.into()).ok().and_then(|x| x.as_f64()).unwrap_or(-1.0)
}

fn js_string(v: &JsValue, key: &str) -> String {
    js_sys::Reflect::get(v, &key.into()).ok().and_then(|x| x.as_string()).unwrap_or_else(|| "book".into())
}

fn js_error(e: &JsValue) -> String {
    js_sys::Reflect::get(e, &"message".into())
        .ok()
        .and_then(|m| m.as_string())
        .filter(|m| !m.is_empty())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}
