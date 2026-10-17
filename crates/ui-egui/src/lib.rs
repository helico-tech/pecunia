#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! The Pecunia egui front end (L4). Thin: screens read [`Session`] state (and the pure model functions over it) and
//! act only through [`PecuniaApp::run`], which dispatches engine commands by id. Colours come from [`theme::Tokens`];
//! icons are drawn in code ([`icons`]). Nothing below this crate depends on egui.

pub mod accounts;
pub mod detail;
pub mod dialogs;
pub mod icons;
pub mod lists;
pub mod overview;
pub mod palette;
pub mod pickers;
pub mod pots;
pub mod register;
pub mod reports;
pub mod review;
pub mod sidebar;
pub mod theme;
pub mod widgets;

use egui::{Key, KeyboardShortcut, Modifiers};
use pecunia_engine::model::TxnId;
use pecunia_engine::{Screen, Session, UiRequest};
use serde_json::{Value as Json, json};

use crate::theme::Tokens;

/// Below this width (in points) the window gets the phone layout: one thing at a time.
pub const COMPACT_WIDTH: f32 = 720.0;

/// Whether the window is narrow enough for the phone layout.
pub fn is_compact(ctx: &egui::Context) -> bool {
    ctx.content_rect().width() < COMPACT_WIDTH
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn id(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<ThemeMode> {
        [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark].into_iter().find(|m| m.id() == s)
    }
}

/// A file dialog the host app should show (the UI crate has no native dialogs).
#[derive(Clone, Debug, PartialEq)]
pub enum FileRequest {
    Open,
    /// Ask where to save a new book, then run `file.new` with these params plus `path`.
    New(Json),
    /// Ask for a bank file, then call [`PecuniaApp::open_import`] with it.
    Import,
    /// Ask where to save a copy of the book, then run `file.saveCopy` with `path` (the browser downloads it).
    SaveCopy,
}

/// A bank file the host app picked: a path (desktop) or the bytes (browser).
#[derive(Clone, Debug, PartialEq)]
pub enum ImportFile {
    Path(String),
    Content { name: String, base64: String },
}

impl ImportFile {
    /// The params `import.preview` and `import.file` take for this file.
    pub fn params(&self) -> Json {
        match self {
            ImportFile::Path(p) => json!({ "path": p }),
            ImportFile::Content { name, base64 } => json!({ "name": name, "contentBase64": base64 }),
        }
    }

    pub fn name(&self) -> String {
        match self {
            ImportFile::Path(p) => std::path::Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p).to_string(),
            ImportFile::Content { name, .. } => name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub undo: bool,
    pub error: bool,
    pub until: f64,
}

pub struct UiState {
    pub theme: ThemeMode,
    /// The palette in use this frame.
    pub dark: bool,
    pub sidebar: bool,
    pub dialog: Option<dialogs::Dialog>,
    pub palette: Option<palette::Palette>,
    pub toasts: Vec<Toast>,
    pub buffers: detail::Buffers,
    pub focus_search: bool,
    /// The search box text (sent to `view.filter` as it changes).
    pub search: String,
    pub file_request: Option<FileRequest>,
    /// Recently opened books (the app fills and keeps this).
    pub recent: Vec<String>,
    pub time: f64,
    last_poll: f64,
    styled: Option<bool>,
    fonts_ready: bool,
    /// Keep the selected row in view after keyboard navigation.
    pub scroll_to_selection: bool,
    /// A link the host app shows in the status bar: (label, URL).
    pub footer_link: Option<(String, String)>,
    /// The host keeps books in the browser's storage instead of files (the web app without file access).
    pub books_in_browser: bool,
    /// Show the DEMO banner over a demo book (a host with its own banner turns this off).
    pub demo_banner: bool,
    /// The password typed in the Unlock dialog for a file the host read itself (the browser).
    pub unlock_answer: Option<String>,
    /// The phone layout (set every frame from the window width, see [`COMPACT_WIDTH`]).
    pub compact: bool,
    /// Phone layout: the menu (the sidebar) fills the screen.
    pub menu: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            theme: ThemeMode::System,
            dark: false,
            sidebar: true,
            dialog: None,
            palette: None,
            toasts: Vec::new(),
            buffers: detail::Buffers::default(),
            focus_search: false,
            search: String::new(),
            file_request: None,
            recent: Vec::new(),
            time: 0.0,
            last_poll: 0.0,
            styled: None,
            fonts_ready: false,
            scroll_to_selection: false,
            footer_link: None,
            books_in_browser: false,
            demo_banner: true,
            unlock_answer: None,
            compact: false,
            menu: false,
        }
    }
}

pub struct PecuniaApp {
    pub session: Session,
    pub ui: UiState,
}

impl PecuniaApp {
    pub fn new(session: Session) -> PecuniaApp {
        PecuniaApp { session, ui: UiState::default() }
    }

    /// Whether the fonts are bound, so a host may draw with them (the first frame only sets them up).
    pub fn fonts_ready(&self) -> bool {
        self.ui.fonts_ready
    }

    pub fn tokens(&self) -> Tokens {
        Tokens::get(self.ui.dark)
    }

    /// Runs an engine command and routes what it asks for (dialogs, notices) to the UI. Errors become a toast; the
    /// return value says whether it worked.
    pub fn run(&mut self, id: &str, params: Json) -> bool {
        self.run_value(id, params).is_some()
    }

    /// Like [`PecuniaApp::run`], returning the command's answer.
    pub fn run_value(&mut self, id: &str, params: Json) -> Option<Json> {
        let r = self.session.run(id, params);
        let out = match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(e, false, true);
                None
            }
        };
        self.route();
        out
    }

    pub fn route(&mut self) {
        for req in self.session.take_ui_requests() {
            match req {
                UiRequest::Dialog(name, params) => self.open_dialog(&name, params),
            }
        }
        for n in self.session.take_notices() {
            self.toast(n.text, n.undo, n.error);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, undo: bool, error: bool) {
        let text = text.into();
        self.ui.toasts.retain(|t| t.text != text);
        self.ui.toasts.push(Toast { text, undo, error, until: self.ui.time + if error { 8.0 } else { 5.0 } });
        if self.ui.toasts.len() > 3 {
            self.ui.toasts.remove(0);
        }
    }

    /// Opens a dialog by name: `newTxn`, `newAccount`, `editAccount` (`id`), `potMove`, `split` (`id`), `import`,
    /// `newBook`, `rename`, `open`, `saveCopy` (native file dialogs).
    pub fn open_dialog(&mut self, name: &str, params: Json) {
        use dialogs::Dialog;
        let d = match name {
            "open" => {
                self.ui.file_request = Some(FileRequest::Open);
                return;
            }
            "import" => {
                self.ui.file_request = Some(FileRequest::Import);
                return;
            }
            "saveCopy" => {
                self.ui.file_request = Some(FileRequest::SaveCopy);
                return;
            }
            "newTxn" => Dialog::NewTxn(dialogs::NewTxnForm::from_params(&self.session, &params)),
            "newAccount" => Dialog::NewAccount(dialogs::NewAccountForm::from_params(&self.session, &params)),
            "editAccount" => match dialogs::EditAccountForm::from_params(&self.session, &params) {
                Some(f) => Dialog::EditAccount(f),
                None => return,
            },
            "potMove" => Dialog::PotMove(dialogs::PotMoveForm::from_params(&self.session, &params)),
            "split" => match dialogs::SplitForm::from_params(&self.session, &params) {
                Some(f) => Dialog::Split(f),
                None => return,
            },
            "newBook" => Dialog::NewBook(dialogs::NewBookForm::default()),
            "rename" => Dialog::Rename(dialogs::RenameForm { name: self.session.book().name().to_string(), focused: false }),
            "unlock" => Dialog::Unlock(dialogs::UnlockForm::from_params(&params)),
            "setPassword" => Dialog::SetPassword(dialogs::SetPasswordForm::default()),
            other => {
                log::warn!("unknown dialog {other}");
                return;
            }
        };
        self.ui.dialog = Some(d);
    }

    /// Shows the import dialog for a bank file the host picked.
    pub fn open_import(&mut self, file: ImportFile) {
        self.ui.dialog = Some(dialogs::Dialog::Import(dialogs::ImportForm::new(&mut self.session, file)));
    }

    pub fn select(&mut self, id: Option<TxnId>) {
        if self.session.selection != id {
            self.run("txn.select", json!({ "id": id.map(|i| i.0) }));
        }
    }

    pub fn go(&mut self, screen: Screen) {
        self.run("view.screen", json!({ "screen": screen.id() }));
    }

    /// Shows the register of one account, pot or category.
    pub fn show_account(&mut self, id: pecunia_engine::model::AccountId) {
        self.ui.search.clear();
        self.run("view.filter", json!({ "replace": true, "account": id.0 }));
    }

    /// New transaction, with the account on screen as where the money comes from.
    pub fn new_txn(&mut self) {
        let mut p = json!({});
        if let Some(a) = self.session.view.filter.account.filter(|_| self.session.view.screen == Screen::Transactions) {
            p["from"] = json!(a.0);
        }
        self.open_dialog("newTxn", p);
    }

    /// Called every frame before `ui` (and while the window is hidden): reload external changes, follow the system
    /// theme.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.ui.time = ctx.input(|i| i.time);
        if self.ui.time - self.ui.last_poll > 1.0 {
            self.ui.last_poll = self.ui.time;
            if self.session.poll_external() {
                self.ui.buffers = detail::Buffers::default();
            }
            self.route();
            ctx.request_repaint_after(std::time::Duration::from_millis(1100));
        }
        let dark = match self.ui.theme {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => ctx.system_theme() == Some(egui::Theme::Dark),
        };
        if self.ui.styled != Some(dark) {
            if self.ui.styled.is_none() {
                ctx.set_fonts(theme::font_definitions());
            }
            theme::apply(ctx, dark);
            self.ui.styled = Some(dark);
            self.ui.dark = dark;
        }
        self.ui.toasts.retain(|t| t.until > self.ui.time);
        if !self.ui.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k: Key| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let cmd_shift = |k: Key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        let consume = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));
        let modal = self.ui.dialog.is_some() || self.ui.palette.is_some();
        let typing = ctx.text_edit_focused();

        if consume(cmd(Key::K)) {
            self.ui.palette = if self.ui.palette.is_some() { None } else { Some(palette::Palette::default()) };
            return;
        }
        if modal {
            return;
        }
        let screens = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
        let ids = ["view.overview", "view.review", "view.transactions", "view.pots", "view.reports", "view.accounts"];
        for (k, id) in screens.into_iter().zip(ids) {
            if consume(cmd(k)) {
                self.run(id, json!({}));
                return;
            }
        }
        if consume(cmd_shift(Key::N)) {
            self.run("file.new", json!({}));
        } else if consume(cmd(Key::N)) {
            self.new_txn();
        } else if consume(cmd(Key::I)) {
            self.run("import.file", json!({}));
        } else if consume(cmd(Key::O)) {
            self.run("file.open", json!({}));
        } else if consume(cmd(Key::S)) {
            if self.run("file.save", json!({})) {
                self.toast("All changes are saved", false, false);
            }
        } else if consume(cmd(Key::F)) {
            self.ui.focus_search = true;
            if !matches!(self.session.view.screen, Screen::Transactions) {
                self.go(Screen::Transactions);
            }
        } else if consume(cmd(Key::Backslash)) {
            self.ui.sidebar = !self.ui.sidebar;
        }
        if typing {
            return;
        }
        if consume(cmd_shift(Key::Z)) {
            self.run("edit.redo", json!({}));
        } else if consume(cmd(Key::Z)) {
            self.run("edit.undo", json!({}));
        } else if self.session.selection.is_some() && (consume(cmd(Key::Backspace)) || consume(KeyboardShortcut::new(Modifiers::NONE, Key::Delete))) {
            self.run("txn.delete", json!({}));
        } else if self.session.selection.is_some() && consume(cmd(Key::Enter)) {
            self.run("txn.confirm", json!({}));
        } else if consume(KeyboardShortcut::new(Modifiers::NONE, Key::Escape)) {
            self.select(None);
        } else if consume(KeyboardShortcut::new(Modifiers::NONE, Key::ArrowDown)) || consume(KeyboardShortcut::new(Modifiers::NONE, Key::J)) {
            self.step_selection(1);
        } else if consume(KeyboardShortcut::new(Modifiers::NONE, Key::ArrowUp)) || consume(KeyboardShortcut::new(Modifiers::NONE, Key::K)) {
            self.step_selection(-1);
        }
    }

    /// The transactions on screen, in order, for ↑/↓.
    fn nav_order(&self) -> Vec<TxnId> {
        match self.session.view.screen {
            Screen::Transactions => register::nav_order(self),
            Screen::Review => review::nav_order(self),
            _ => Vec::new(),
        }
    }

    /// Moves the selection through the rows on screen.
    fn step_selection(&mut self, delta: i32) {
        let order = self.nav_order();
        if order.is_empty() {
            return;
        }
        let pos = self.session.selection.and_then(|s| order.iter().position(|i| *i == s));
        let next = match pos {
            None if delta > 0 => 0,
            None => order.len() - 1,
            Some(p) => (p as i64 + delta as i64).clamp(0, order.len() as i64 - 1) as usize,
        };
        if let Some(id) = order.get(next).copied() {
            self.select(Some(id));
            self.ui.scroll_to_selection = true;
        }
    }

    /// Whether this screen shows the detail pane for the selection.
    fn shows_detail(&self) -> bool {
        matches!(self.session.view.screen, Screen::Transactions | Screen::Review | Screen::Overview | Screen::Pots)
            && self.session.selection.is_some_and(|id| self.session.book().txn(id).is_some())
    }

    /// Draws the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.ui.styled.is_none() {
            self.logic(&ctx);
        }
        // Fonts set this frame are bound from the next one; drawing with the bold family before that would fail, so the
        // first frame only paints the background.
        if !self.ui.fonts_ready {
            self.ui.fonts_ready = true;
            ui.painter().rect_filled(ui.max_rect(), 0.0, self.tokens().bg);
            ctx.request_repaint();
            return;
        }
        self.shortcuts(&ctx);
        let t = self.tokens();
        self.ui.compact = is_compact(&ctx);
        if !self.ui.compact {
            self.ui.menu = false;
        }

        egui::Panel::bottom("status")
            .resizable(false)
            .exact_size(26.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(12, 0)))
            .show(ui, |ui| self.status_bar(ui, &t));
        if self.ui.demo_banner && self.session.is_demo() {
            egui::Panel::top("demo")
                .resizable(false)
                .frame(egui::Frame::new().fill(t.warning_soft).inner_margin(egui::Margin::symmetric(14, 8)))
                .show(ui, |ui| self.demo_banner(ui, &t));
        }
        if self.ui.compact {
            self.compact_ui(ui, &t);
            dialogs::show(self, &ctx, &t);
            palette::show(self, &ctx, &t);
            self.toasts(&ctx, &t);
            return;
        }
        if self.ui.sidebar {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(248.0)
                .size_range(210.0..=340.0)
                .frame(egui::Frame::new().fill(t.sidebar))
                .show(ui, |ui| sidebar::show(self, ui, &t));
        }
        // A tablet: with the sidebar and the detail pane, the list would be too narrow; the detail takes its place.
        let room = ctx.content_rect().width() - if self.ui.sidebar { 248.0 } else { 0.0 } - 420.0;
        let detail_instead = self.shows_detail() && room < 440.0;
        if self.shows_detail() && !detail_instead {
            egui::Panel::right("detail")
                .resizable(true)
                .default_size(420.0)
                .size_range(360.0..=640.0)
                .frame(egui::Frame::new().fill(t.bg))
                .show(ui, |ui| detail::show(self, ui, &t));
        }
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(t.bg)).show(ui, |ui| {
            if detail_instead {
                detail::show(self, ui, &t);
            } else {
                self.screen(ui, &t);
            }
        });

        dialogs::show(self, &ctx, &t);
        palette::show(self, &ctx, &t);
        self.toasts(&ctx, &t);
    }

    fn screen(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        match self.session.view.screen {
            Screen::Overview => overview::show(self, ui, t),
            Screen::Transactions => register::show(self, ui, t),
            Screen::Review => review::show(self, ui, t),
            Screen::Pots => pots::show(self, ui, t),
            Screen::Reports => reports::show(self, ui, t),
            Screen::Accounts => accounts::show(self, ui, t),
            Screen::Rules => lists::rules(self, ui, t),
            Screen::Imports => lists::imports(self, ui, t),
        }
    }

    /// The phone layout: a top bar, then one view: the menu, the selected transaction, or the screen.
    fn compact_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        egui::Panel::top("compact-top")
            .resizable(false)
            .exact_size(48.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(6, 0)))
            .show(ui, |ui| self.top_bar(ui, t));
        let detail = !self.ui.menu && self.shows_detail();
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(if self.ui.menu { t.sidebar } else { t.bg })).show(ui, |ui| {
            if self.ui.menu {
                sidebar::show(self, ui, t);
            } else if detail {
                detail::show(self, ui, t);
            } else {
                self.screen(ui, t);
            }
        });
    }

    /// Phone layout: the menu button, the book, and New transaction.
    fn top_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let full = ui.max_rect();
        ui.painter().line_segment([full.left_bottom(), full.right_bottom()], egui::Stroke::new(1.0, t.border));
        let mut toggle = false;
        let mut new = false;
        ui.horizontal_centered(|ui| {
            let (g, tip) = if self.ui.menu { (icons::Glyph::Close, "Close the menu") } else { (icons::Glyph::Menu, "Menu") };
            toggle = widgets::icon_button_sized(ui, g, tip, 40.0, false, t).clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                new = widgets::icon_button_sized(ui, icons::Glyph::Plus, "New transaction", 40.0, false, t).clicked();
                let demo = self.session.is_demo();
                let room = ui.available_width() - if demo { 54.0 } else { 6.0 };
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let name = widgets::elide(ui, self.session.book().name(), theme::bold(15.0), room);
                    ui.label(egui::RichText::new(name).font(theme::bold(15.0)).color(t.text));
                    if demo {
                        widgets::demo_badge(ui, 11.0, t);
                    }
                });
            });
        });
        if toggle {
            self.ui.menu = !self.ui.menu;
        }
        if new {
            self.ui.menu = false;
            self.new_txn();
        }
    }

    /// The DEMO banner: what this book is, and how to start a real one.
    fn demo_banner(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            widgets::demo_badge(ui, 12.0, t);
            let text = match (self.session.store_path().is_some(), self.ui.compact) {
                (true, false) => "This book is filled with a made-up household. Its money and payees are not real.",
                (true, true) => "A made-up household. Its money is not real.",
                (false, false) => "You are looking at a made-up household to try Pecunia. Nothing here is real, and nothing is saved.",
                (false, true) => "A made-up household to try Pecunia. Nothing is saved.",
            };
            ui.label(egui::RichText::new(text).size(13.0).color(t.text));
            if self.session.store_path().is_none() {
                if ui.button("Start your own book…").clicked() {
                    action = Some("file.new");
                }
                if ui.button("Open a book…").clicked() {
                    action = Some("file.open");
                }
            }
        });
        if let Some(a) = action {
            self.run(a, json!({}));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        ui.horizontal_centered(|ui| {
            let (dot, text) = match &self.session.save_error {
                Some(e) => (t.danger, format!("Not saved: {e}")),
                None if self.session.is_demo() && self.session.store_path().is_none() && self.session.location() == "memory" => {
                    (t.warning, "Not saved: a demo in memory".to_string())
                }
                None if self.session.encrypted() => (t.positive, "Saved · encrypted".to_string()),
                None => (t.positive, "Saved".to_string()),
            };
            let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, dot);
            ui.label(egui::RichText::new(text).size(11.5).color(if self.session.save_error.is_some() { t.danger } else { t.text_dim }));
            if self.ui.compact {
                return;
            }
            if self.session.store_path().is_some() || !self.session.is_demo() {
                ui.label(egui::RichText::new(self.session.location()).size(11.5).color(t.text_faint)).on_hover_text("Where this book is stored");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = self.session.book();
                let waiting = b.review_count();
                let text = format!("{} transactions · {} to review", b.txn_count(), waiting);
                ui.label(egui::RichText::new(text).size(11.5).color(t.text_dim));
                if let Some((label, url)) = &self.ui.footer_link {
                    ui.add_space(8.0);
                    ui.hyperlink_to(egui::RichText::new(label).size(11.5).color(t.text_dim), url);
                }
            });
        });
    }

    fn toasts(&mut self, ctx: &egui::Context, t: &Tokens) {
        if self.ui.toasts.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let mut undo = false;
        let mut close = None;
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(screen.center().x, screen.bottom() - 40.0))
            .show(ctx, |ui| {
                for (i, toast) in self.ui.toasts.iter().enumerate() {
                    egui::Frame::new()
                        .fill(if toast.error { t.danger_soft } else { t.surface })
                        .stroke(egui::Stroke::new(1.0, if toast.error { t.danger } else { t.border_strong }))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(14, 8))
                        .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow })
                        .show(ui, |ui| {
                            ui.set_max_width(560.0f32.min(screen.width() - 40.0));
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&toast.text).color(if toast.error { t.danger } else { t.text }));
                                if toast.undo && ui.link(egui::RichText::new("Undo").color(t.accent).strong()).clicked() {
                                    undo = true;
                                    close = Some(i);
                                }
                                if widgets::icon_button_sized(ui, icons::Glyph::Close, "", 20.0, false, t).clicked() {
                                    close = Some(i);
                                }
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        if let Some(i) = close
            && i < self.ui.toasts.len()
        {
            self.ui.toasts.remove(i);
        }
        if undo {
            self.run("edit.undo", json!({}));
        }
    }
}

/// UI-only commands for the palette and the native menu (`ui.*`); engine commands are listed by the engine registry.
pub const UI_COMMANDS: &[(&str, &str, Option<&str>)] = &[
    ("ui.palette", "Command Palette", Some("⌘K")),
    ("ui.search", "Search Transactions", Some("⌘F")),
    ("ui.toggleSidebar", "Show or Hide Sidebar", Some("⌘\\")),
    ("ui.rename", "Rename Book…", None),
    ("ui.newAccount", "New Account…", None),
    ("ui.newPot", "New Pot…", None),
    ("ui.newCategory", "New Category…", None),
    ("ui.potMove", "Move Money Between Pots…", None),
    ("ui.theme.system", "Appearance: System", None),
    ("ui.theme.light", "Appearance: Light", None),
    ("ui.theme.dark", "Appearance: Dark", None),
];

impl PecuniaApp {
    /// Runs a command by id, UI-only (`ui.*`) or engine. The native menu and the palette use it.
    pub fn run_any(&mut self, id: &str, params: Json) -> bool {
        match id {
            "ui.palette" => self.ui.palette = Some(palette::Palette::default()),
            "ui.search" => {
                self.ui.focus_search = true;
                self.go(Screen::Transactions);
            }
            "ui.toggleSidebar" if self.ui.compact => self.ui.menu = !self.ui.menu,
            "ui.toggleSidebar" => self.ui.sidebar = !self.ui.sidebar,
            "ui.rename" => self.open_dialog("rename", json!({})),
            "ui.newAccount" => self.open_dialog("newAccount", json!({})),
            "ui.newPot" => self.open_dialog("newAccount", json!({ "kind": "pot" })),
            "ui.newCategory" => self.open_dialog("newAccount", json!({ "kind": "category" })),
            "ui.potMove" => self.open_dialog("potMove", params),
            "file.setPassword" if params.get("password").is_none() => self.open_dialog("setPassword", json!({})),
            "ui.split" => self.open_dialog("split", params),
            "ui.editAccount" => self.open_dialog("editAccount", params),
            "txn.create" if params.as_object().is_none_or(|o| o.is_empty()) => self.new_txn(),
            other => {
                if let Some(mode) = other.strip_prefix("ui.theme.").and_then(ThemeMode::parse) {
                    // `logic` restyles on the next frame when the mode gives another palette.
                    self.ui.theme = mode;
                    return true;
                }
                return self.run(other, params);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
