//! Modal dialogs: New transaction, New account, Edit account, Move money between pots, Split, Import a bank file (with
//! a column mapping for CSV layouts nobody knows yet), New book, Rename book. Each dialog only gathers input; the
//! engine command it runs does the work and the checking.

use egui::{Key, Modifiers, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use pecunia_engine::Session;
use pecunia_engine::model::{AccountId, AccountKind, Group, Money, TxnId};
use serde_json::{Value as Json, json};

use crate::icons::{self, Glyph};
use crate::pickers::{self, Choices};
use crate::theme::{self, Tokens};
use crate::{FileRequest, ImportFile, PecuniaApp, widgets};

pub enum Dialog {
    NewTxn(NewTxnForm),
    NewAccount(NewAccountForm),
    EditAccount(EditAccountForm),
    PotMove(PotMoveForm),
    Split(SplitForm),
    Import(ImportForm),
    NewBook(NewBookForm),
    Rename(RenameForm),
    Unlock(UnlockForm),
    SetPassword(SetPasswordForm),
}

fn account_of(s: &Session, p: &Json, key: &str) -> Option<AccountId> {
    match p.get(key) {
        Some(Json::Number(n)) => n.as_u64().map(AccountId).filter(|id| s.book().account(*id).is_some()),
        Some(Json::String(t)) => s.book().resolve_account(t),
        _ => None,
    }
}

fn str_of(p: &Json, key: &str) -> String {
    match p.get(key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub struct NewTxnForm {
    pub payee: String,
    pub from: Option<AccountId>,
    pub to: Option<AccountId>,
    pub amount: String,
    pub date: String,
    pub note: String,
    pub error: Option<String>,
    focused: bool,
}

impl NewTxnForm {
    pub fn from_params(s: &Session, p: &Json) -> NewTxnForm {
        NewTxnForm {
            payee: str_of(p, "payee"),
            from: account_of(s, p, "from"),
            to: account_of(s, p, "to"),
            amount: str_of(p, "amount"),
            date: if str_of(p, "date").is_empty() { s.today().to_string() } else { str_of(p, "date") },
            note: str_of(p, "note"),
            error: None,
            focused: false,
        }
    }
}

pub struct NewAccountForm {
    pub kind: AccountKind,
    pub name: String,
    pub parent: Option<AccountId>,
    pub group: Group,
    pub iban: String,
    pub balance: String,
    pub date: String,
    pub goal: String,
    pub error: Option<String>,
    focused: bool,
}

impl NewAccountForm {
    pub fn from_params(s: &Session, p: &Json) -> NewAccountForm {
        let kind =
            p.get("kind").and_then(Json::as_str).and_then(AccountKind::parse).filter(|k| *k != AccountKind::System).unwrap_or(AccountKind::Current);
        NewAccountForm {
            kind,
            name: str_of(p, "name"),
            parent: account_of(s, p, "parent"),
            group: p
                .get("group")
                .and_then(Json::as_str)
                .and_then(Group::parse)
                .filter(|g| matches!(g, Group::Goes | Group::Comes))
                .unwrap_or(Group::Goes),
            iban: str_of(p, "iban"),
            balance: String::new(),
            date: s.today().to_string(),
            goal: String::new(),
            error: None,
            focused: false,
        }
    }

    fn params(&self) -> Json {
        let mut p = json!({ "kind": self.kind.id(), "name": self.name.trim() });
        if let Some(par) = self.parent.filter(|_| matches!(self.kind, AccountKind::Pot | AccountKind::Category)) {
            p["parent"] = json!(par.0);
        }
        if self.kind == AccountKind::CategoryGroup {
            p["group"] = json!(self.group.id());
        }
        if self.kind.is_physical() && !self.iban.trim().is_empty() {
            p["iban"] = json!(self.iban.trim());
        }
        if (self.kind.is_physical() || self.kind == AccountKind::Pot) && !self.balance.trim().is_empty() {
            p["balance"] = json!(self.balance.trim());
            p["balanceDate"] = json!(self.date.trim());
        }
        if self.kind == AccountKind::Pot && !self.goal.trim().is_empty() {
            p["goal"] = json!(self.goal.trim());
        }
        p
    }
}

pub struct EditAccountForm {
    pub id: AccountId,
    pub kind: AccountKind,
    pub name: String,
    pub iban: String,
    pub description: String,
    pub goal: String,
    pub goal_date: String,
    pub balance: String,
    pub balance_date: String,
    /// The starting balance and date when the dialog opened; saving sets it only when it changed.
    balance_was: (String, String),
    pub value: String,
    pub closed: bool,
    pub error: Option<String>,
}

impl EditAccountForm {
    pub fn from_params(s: &Session, p: &Json) -> Option<EditAccountForm> {
        let id = account_of(s, p, "id")?;
        let a = s.book().account(id)?.clone();
        if a.system.is_some() {
            return None;
        }
        // The current starting balance, if there is one.
        let tree = s.book().tree();
        let leaf = tree.resolve_leaf(id).ok();
        let opening =
            s.book().txns().find(|t| t.kind == pecunia_engine::model::TxnKind::Opening && t.postings.iter().any(|p| Some(p.account) == leaf));
        let (balance, balance_date) = match opening {
            Some(o) => {
                let on_leaf: Money = o.postings.iter().filter(|p| Some(p.account) == leaf).map(|p| p.amount).sum();
                (Money(on_leaf.cents() * a.group.display_sign()).format_plain(), o.date.to_string())
            }
            None => (String::new(), s.today().to_string()),
        };
        Some(EditAccountForm {
            id,
            kind: a.kind,
            name: a.name.clone(),
            iban: a.iban.clone().unwrap_or_default(),
            description: a.description.clone(),
            goal: a.goal.map(|g| g.format_plain()).unwrap_or_default(),
            goal_date: a.goal_date.map(|d| d.to_string()).unwrap_or_default(),
            balance_was: (balance.clone(), balance_date.clone()),
            balance,
            balance_date,
            value: String::new(),
            closed: a.closed,
            error: None,
        })
    }
}

pub struct PotMoveForm {
    pub from: Option<AccountId>,
    pub to: Option<AccountId>,
    pub amount: String,
    pub date: String,
    pub note: String,
    pub error: Option<String>,
}

impl PotMoveForm {
    pub fn from_params(s: &Session, p: &Json) -> PotMoveForm {
        PotMoveForm {
            from: account_of(s, p, "from"),
            to: account_of(s, p, "to"),
            amount: str_of(p, "amount"),
            date: s.today().to_string(),
            note: String::new(),
            error: None,
        }
    }
}

pub struct SplitForm {
    pub id: TxnId,
    pub total: Money,
    pub parts: Vec<(Option<AccountId>, String)>,
    pub error: Option<String>,
}

impl SplitForm {
    pub fn from_params(s: &Session, p: &Json) -> Option<SplitForm> {
        let id = p.get("id").and_then(Json::as_u64).map(TxnId).or(s.selection)?;
        let txn = s.book().txn(id)?;
        let tree = s.book().tree();
        let (_, counter) = pecunia_engine::model::ledger::own_and_counter(&tree, txn).ok()?;
        let total: Money = counter.iter().map(|p| p.amount).sum();
        let mut parts: Vec<(Option<AccountId>, String)> = counter
            .iter()
            .map(|p| {
                let unsorted = s.book().account(p.account).is_some_and(|a| a.system.is_some());
                (if unsorted { None } else { Some(p.account) }, p.amount.format_plain())
            })
            .collect();
        if parts.len() < 2 {
            parts.push((None, String::new()));
        }
        Some(SplitForm { id, total, parts, error: None })
    }
}

/// A bank file being imported: what the preview said, and the choices made.
pub struct ImportForm {
    pub file: ImportFile,
    pub preview: Option<Json>,
    pub account: Option<AccountId>,
    pub error: Option<String>,
    pub mapping: Mapping,
}

/// Columns of a CSV no built-in layout fits.
#[derive(Default)]
pub struct Mapping {
    pub name: String,
    pub date: String,
    pub date_format: String,
    pub amount: String,
    pub payee: String,
    pub description: String,
    pub iban: String,
    pub decimal_comma: bool,
}

impl ImportForm {
    pub fn new(s: &mut Session, file: ImportFile) -> ImportForm {
        let mut f = ImportForm {
            file,
            preview: None,
            account: None,
            error: None,
            mapping: Mapping { date_format: "yyyy-MM-dd".into(), decimal_comma: true, ..Default::default() },
        };
        f.refresh(s);
        f
    }

    fn params(&self) -> Json {
        let mut p = self.file.params();
        if let Some(a) = self.account {
            p["account"] = json!(a.0);
        }
        p
    }

    /// Runs `import.preview` again (after a new account choice or a saved layout).
    pub fn refresh(&mut self, s: &mut Session) {
        match s.execute("import.preview", self.params()) {
            Ok(v) => {
                if self.account.is_none() {
                    self.account = v.get("accountId").and_then(Json::as_u64).map(AccountId);
                }
                if let Some(h) = v["csv"]["headers"].as_array() {
                    let names: Vec<String> = h.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                    let find =
                        |words: &[&str]| names.iter().find(|n| words.iter().any(|w| n.to_lowercase().contains(w))).cloned().unwrap_or_default();
                    if self.mapping.date.is_empty() {
                        self.mapping.date = find(&["date", "datum"]);
                        self.mapping.amount = find(&["amount", "bedrag"]);
                        self.mapping.payee = find(&["name", "naam", "tegenpartij"]);
                        self.mapping.description = find(&["description", "omschrijving", "mededeling"]);
                        self.mapping.name = self.file.name().split('.').next().unwrap_or("My bank").to_string();
                    }
                }
                self.preview = Some(v);
                self.error = None;
            }
            Err(e) => {
                self.preview = None;
                self.error = Some(e.to_string());
            }
        }
    }
}

#[derive(Default)]
pub struct NewBookForm {
    pub name: String,
    pub categories: bool,
    pub sample: bool,
    pub password: String,
    pub repeat: String,
    pub error: Option<String>,
    focused: bool,
    touched: bool,
}

/// Asks for the password of a protected book: one opened by path (desktop) or one the host read itself (browser),
/// which gets the password back through [`crate::UiState::unlock_answer`].
pub struct UnlockForm {
    pub path: Option<String>,
    pub name: String,
    pub password: String,
    pub error: Option<String>,
    focused: bool,
}

impl UnlockForm {
    pub fn from_params(p: &Json) -> UnlockForm {
        let path = p.get("path").and_then(Json::as_str).map(str::to_string);
        let name = p
            .get("name")
            .and_then(Json::as_str)
            .map(str::to_string)
            .or_else(|| path.as_deref().map(|x| std::path::Path::new(x).file_name().and_then(|n| n.to_str()).unwrap_or(x).to_string()))
            .unwrap_or_else(|| "This book".into());
        UnlockForm { path, name, password: String::new(), error: p.get("error").and_then(Json::as_str).map(str::to_string), focused: false }
    }
}

#[derive(Default)]
pub struct SetPasswordForm {
    pub password: String,
    pub repeat: String,
    pub error: Option<String>,
    focused: bool,
}

/// Checks a new password and its repeat; `None` when they are fine.
fn password_problem(password: &str, repeat: &str) -> Option<String> {
    if password.chars().count() < 8 {
        Some("Use at least 8 characters.".into())
    } else if password != repeat {
        Some("The two passwords are not the same.".into())
    } else {
        None
    }
}

/// A one-line password box (the text shows as dots).
fn password_box(ui: &mut Ui, buf: &mut String, hint: &str, t: &Tokens) -> egui::Response {
    egui::Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.border_strong))
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| ui.add(egui::TextEdit::singleline(buf).password(true).frame(egui::Frame::NONE).hint_text(hint).desired_width(f32::INFINITY)))
        .inner
}

pub struct RenameForm {
    pub name: String,
    pub focused: bool,
}

enum Outcome {
    Keep,
    Close,
    Run(Vec<(&'static str, Json)>),
    File(FileRequest),
    /// Save a CSV layout, then preview the file again.
    SaveProfile(Json),
    /// Preview again (the account changed).
    Refresh,
    /// The host app opens the file it read itself with this password.
    Unlocked(String),
}

pub fn show(app: &mut PecuniaApp, ctx: &egui::Context, t: &Tokens) {
    let Some(mut dialog) = app.ui.dialog.take() else { return };
    let frame = egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border_strong))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(20))
        .shadow(egui::epaint::Shadow { offset: [0, 12], blur: 36, spread: 0, color: t.shadow });
    // One id per kind of dialog: egui remembers a window's size by id, and a small dialog must not keep a big one's.
    let kind = std::mem::discriminant(&dialog);
    let modal =
        egui::Modal::new(egui::Id::new(("dialog", kind))).frame(frame).backdrop_color(egui::Color32::from_black_alpha(if t.dark { 120 } else { 60 }));
    let session = &app.session;
    let in_browser = app.ui.books_in_browser;
    // A dialog taller than the screen (a phone, a long import preview) scrolls.
    let tall = (ctx.content_rect().height() - 80.0).max(200.0);
    let resp = modal.show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .id_salt(kind)
            .max_height(tall)
            .show(ui, |ui| {
                let outcome = match &mut dialog {
                    Dialog::NewTxn(f) => new_txn(session, ui, f, t),
                    Dialog::NewAccount(f) => new_account(session, ui, f, t),
                    Dialog::EditAccount(f) => edit_account(session, ui, f, t),
                    Dialog::PotMove(f) => pot_move(session, ui, f, t),
                    Dialog::Split(f) => split(session, ui, f, t),
                    Dialog::Import(f) => import(session, ui, f, t),
                    Dialog::NewBook(f) => new_book(ui, f, in_browser, t),
                    Dialog::Rename(f) => rename(ui, f, t),
                    Dialog::Unlock(f) => unlock(ui, f, t),
                    Dialog::SetPassword(f) => set_password(session, ui, f, t),
                };
                // Room for the buttons' bottom edge inside the scroll area.
                ui.add_space(2.0);
                outcome
            })
            .inner
    });
    let outcome = if resp.should_close() { Outcome::Close } else { resp.inner };
    match outcome {
        Outcome::Keep => app.ui.dialog = Some(dialog),
        Outcome::Close => {}
        Outcome::File(r) => app.ui.file_request = Some(r),
        Outcome::Unlocked(pw) => app.ui.unlock_answer = Some(pw),
        Outcome::Refresh => {
            if let Dialog::Import(f) = &mut dialog {
                f.refresh(&mut app.session);
            }
            app.ui.dialog = Some(dialog);
        }
        Outcome::SaveProfile(profile) => {
            match app.session.run("import.saveProfile", json!({ "profile": profile })) {
                Ok(_) => {
                    if let Dialog::Import(f) = &mut dialog {
                        f.refresh(&mut app.session);
                    }
                }
                Err(e) => set_error(&mut dialog, e),
            }
            app.ui.dialog = Some(dialog);
            app.route();
        }
        Outcome::Run(cmds) => {
            for (id, p) in cmds {
                if let Err(e) = app.session.run(id, p) {
                    set_error(&mut dialog, e);
                    app.ui.dialog = Some(dialog);
                    app.route();
                    return;
                }
            }
            app.route();
        }
    }
}

fn set_error(d: &mut Dialog, e: String) {
    let slot = match d {
        Dialog::NewTxn(f) => &mut f.error,
        Dialog::NewAccount(f) => &mut f.error,
        Dialog::EditAccount(f) => &mut f.error,
        Dialog::PotMove(f) => &mut f.error,
        Dialog::Split(f) => &mut f.error,
        Dialog::Import(f) => &mut f.error,
        Dialog::Unlock(f) => &mut f.error,
        Dialog::SetPassword(f) => &mut f.error,
        Dialog::NewBook(f) => &mut f.error,
        Dialog::Rename(_) => return,
    };
    *slot = Some(e);
}

fn title_bar(ui: &mut Ui, text: &str, t: &Tokens) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text).font(theme::bold(15.0)).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button_sized(ui, Glyph::Close, "Close (Esc)", 24.0, false, t).clicked() {
                close = true;
            }
        });
    });
    close
}

fn field(ui: &mut Ui, label: &str, t: &Tokens) {
    ui.label(egui::RichText::new(label).size(12.0).color(t.text_dim));
}

fn text_box(ui: &mut Ui, buf: &mut String, hint: &str, t: &Tokens) -> egui::Response {
    egui::Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.border_strong))
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| ui.add(egui::TextEdit::singleline(buf).frame(egui::Frame::NONE).hint_text(hint).desired_width(f32::INFINITY)))
        .inner
}

fn error_line(ui: &mut Ui, e: &Option<String>, t: &Tokens) {
    if let Some(e) = e {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(e).color(t.danger).size(12.5));
    }
}

/// Cancel and the main button, right-aligned. Returns (main clicked, cancel clicked).
fn buttons(ui: &mut Ui, main: &str, t: &Tokens) -> (bool, bool) {
    let mut out = (false, false);
    ui.add_space(16.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        out.0 = widgets::primary_button(ui, main, t).clicked();
        out.1 = widgets::secondary_button(ui, "Cancel", t).clicked();
    });
    out
}

/// Sets a dialog's width: `w`, or less on a narrow screen.
fn fit(ui: &mut Ui, w: f32) {
    let room = ui.ctx().content_rect().width() - 64.0;
    ui.set_width(w.min(room).max(200.0));
}

/// Two fields side by side; on a phone, one below the other.
fn two_columns(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    if crate::is_compact(ui.ctx()) {
        left(ui);
        ui.add_space(8.0);
        right(ui);
        return;
    }
    let w = (ui.available_width() - 12.0) / 2.0;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(w);
            left(ui);
        });
        ui.vertical(|ui| {
            ui.set_width(w);
            right(ui);
        });
    });
}

fn new_txn(s: &Session, ui: &mut Ui, f: &mut NewTxnForm, t: &Tokens) -> Outcome {
    fit(ui, 520.0);
    if title_bar(ui, "New transaction", t) {
        return Outcome::Close;
    }
    ui.label(egui::RichText::new("For money no bank file shows: cash, a card you don't import, a gift.").size(12.5).color(t.text_dim));
    ui.add_space(12.0);
    let submit = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    field(ui, "Payee", t);
    let payee = text_box(ui, &mut f.payee, "e.g. Market, Bakery", t);
    if !f.focused {
        payee.request_focus();
        f.focused = true;
    }
    ui.add_space(8.0);
    two_columns(
        ui,
        |ui| {
            field(ui, "Amount (euros)", t);
            text_box(ui, &mut f.amount, "12,50", t);
        },
        |ui| {
            field(ui, "Date", t);
            text_box(ui, &mut f.date, "YYYY-MM-DD", t);
        },
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        field(ui, "From", t);
        if let Some(a) = pickers::pill(ui, s.book(), Choices::Any, f.from, "Wallet, an account…", t) {
            f.from = Some(a);
        }
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
        icons::glyph(ui.painter(), r.center(), 12.0, Glyph::Arrow, t.text_faint);
        field(ui, "To", t);
        if let Some(a) = pickers::pill(ui, s.book(), Choices::Any, f.to, "Groceries, a pot…", t) {
            f.to = Some(a);
        }
    });
    ui.add_space(8.0);
    field(ui, "Note", t);
    text_box(ui, &mut f.note, "Optional", t);
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Add transaction", t);
    if cancel {
        return Outcome::Close;
    }
    let enter = ui.input(|i| i.key_pressed(Key::Enter)) && !ui.ctx().text_edit_focused();
    if go || submit || enter {
        let (Some(from), Some(to)) = (f.from, f.to) else {
            f.error = Some("Choose where the money comes from and where it goes.".into());
            return Outcome::Keep;
        };
        return Outcome::Run(vec![(
            "txn.create",
            json!({ "payee": f.payee, "from": from.0, "to": to.0, "amount": f.amount, "date": f.date.trim(), "note": f.note }),
        )]);
    }
    Outcome::Keep
}

const NEW_KINDS: [AccountKind; 9] = [
    AccountKind::Current,
    AccountKind::Savings,
    AccountKind::Cash,
    AccountKind::CreditCard,
    AccountKind::Loan,
    AccountKind::Property,
    AccountKind::Pot,
    AccountKind::Category,
    AccountKind::CategoryGroup,
];

fn new_account(s: &Session, ui: &mut Ui, f: &mut NewAccountForm, t: &Tokens) -> Outcome {
    fit(ui, 560.0);
    if title_bar(ui, "New account", t) {
        return Outcome::Close;
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        for k in NEW_KINDS {
            let on = f.kind == k;
            let group = k.physical_group().unwrap_or(if k == AccountKind::Pot { Group::Have } else { Group::Goes });
            let g = ui.painter().layout_no_wrap(k.label().to_string(), theme::body(13.0), t.text);
            let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 36.0, 30.0), Sense::click());
            widgets::a11y(&resp, k.label());
            let (fill, stroke) = if on {
                (t.accent_soft, t.accent)
            } else if resp.hovered() {
                (t.hover, t.border_strong)
            } else {
                (t.surface, t.border_strong)
            };
            ui.painter().rect(rect, 7.0, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
            icons::account(ui.painter(), pos2(rect.left() + 15.0, rect.center().y), 14.0, k, group, t);
            ui.painter().galley(pos2(rect.left() + 27.0, rect.center().y - g.size().y / 2.0), g, t.text);
            if resp.clicked() {
                f.kind = k;
                f.parent = None;
                f.focused = false;
            }
        }
    });
    ui.add_space(12.0);
    field(ui, "Name", t);
    let hint = match f.kind {
        AccountKind::Current => "e.g. ING Betaalrekening",
        AccountKind::Savings => "e.g. Oranje Spaarrekening",
        AccountKind::Pot => "e.g. Holiday",
        AccountKind::Category => "e.g. Groceries",
        AccountKind::CategoryGroup => "e.g. Daily life",
        _ => "Name",
    };
    let name = text_box(ui, &mut f.name, hint, t);
    if !f.focused {
        name.request_focus();
        f.focused = true;
    }
    ui.add_space(8.0);
    match f.kind {
        AccountKind::Pot => {
            ui.horizontal(|ui| {
                field(ui, "Part of", t);
                let have: Vec<AccountId> = s.book().tree().physical_accounts(Group::Have).iter().map(|a| a.id).collect();
                if let Some(a) = pickers::pill(ui, s.book(), Choices::Physical, f.parent, "Choose an account", t) {
                    if have.contains(&a) {
                        f.parent = Some(a);
                        f.error = None;
                    } else {
                        f.error = Some("Pots belong to an account with money you have.".into());
                    }
                }
            });
            ui.add_space(8.0);
            two_columns(
                ui,
                |ui| {
                    field(ui, "Goal (optional, euros)", t);
                    text_box(ui, &mut f.goal, "2.500", t);
                },
                |ui| {
                    field(ui, "Already in it (optional)", t);
                    text_box(ui, &mut f.balance, "0,00", t);
                },
            );
        }
        AccountKind::Category => {
            ui.horizontal(|ui| {
                field(ui, "In group", t);
                if let Some(a) = pickers::pill(ui, s.book(), Choices::CategoryGroups, f.parent, "Choose a group", t) {
                    f.parent = Some(a);
                }
            });
        }
        AccountKind::CategoryGroup => {
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.group, Group::Goes, "Where money goes");
                ui.radio_value(&mut f.group, Group::Comes, "Where money comes from");
            });
        }
        k => {
            if matches!(k, AccountKind::Current | AccountKind::Savings | AccountKind::CreditCard) {
                field(ui, "IBAN (imports find the account by it)", t);
                text_box(ui, &mut f.iban, "NL00 BANK 0123 4567 89", t);
                ui.add_space(8.0);
            }
            two_columns(
                ui,
                |ui| {
                    field(ui, if k.physical_group() == Some(Group::Owe) { "What you owe now (euros)" } else { "Balance (euros)" }, t);
                    text_box(ui, &mut f.balance, "0,00", t);
                },
                |ui| {
                    field(ui, "On date", t);
                    text_box(ui, &mut f.date, "YYYY-MM-DD", t);
                },
            );
            ui.label(egui::RichText::new("Use the balance on the day before your first imported transaction.").size(11.5).color(t.text_faint));
        }
    }
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, &format!("Add {}", f.kind.label().to_lowercase()), t);
    if cancel {
        return Outcome::Close;
    }
    if go || ui.input(|i| i.key_pressed(Key::Enter)) {
        if f.name.trim().is_empty() {
            f.error = Some("Give it a name first.".into());
            return Outcome::Keep;
        }
        return Outcome::Run(vec![("account.create", f.params())]);
    }
    Outcome::Keep
}

fn edit_account(s: &Session, ui: &mut Ui, f: &mut EditAccountForm, t: &Tokens) -> Outcome {
    fit(ui, 520.0);
    let Some(a) = s.book().account(f.id).cloned() else { return Outcome::Close };
    if title_bar(ui, &format!("{} · {}", a.name, a.kind.label()), t) {
        return Outcome::Close;
    }
    ui.add_space(8.0);
    field(ui, "Name", t);
    if a.unassigned {
        ui.label("Unassigned (the money in the account no pot holds)");
    } else {
        text_box(ui, &mut f.name, "Name", t);
    }
    ui.add_space(8.0);
    if a.kind.is_physical() && a.kind != AccountKind::Cash {
        field(ui, "IBAN", t);
        text_box(ui, &mut f.iban, "NL00 BANK 0123 4567 89", t);
        ui.add_space(8.0);
    }
    if a.kind == AccountKind::Category {
        field(ui, "What belongs here", t);
        text_box(ui, &mut f.description, "e.g. Supermarkets: Albert Heijn, Jumbo", t);
        ui.add_space(8.0);
    }
    if a.kind == AccountKind::Pot && !a.unassigned {
        two_columns(
            ui,
            |ui| {
                field(ui, "Goal (euros)", t);
                text_box(ui, &mut f.goal, "None", t);
            },
            |ui| {
                field(ui, "Goal date", t);
                text_box(ui, &mut f.goal_date, "YYYY-MM-DD", t);
            },
        );
        ui.add_space(8.0);
    }
    let physical_or_pot = a.kind.is_physical() || a.kind == AccountKind::Pot;
    if physical_or_pot {
        two_columns(
            ui,
            |ui| {
                field(ui, "Starting balance (euros)", t);
                text_box(ui, &mut f.balance, "0,00", t);
            },
            |ui| {
                field(ui, "On date", t);
                text_box(ui, &mut f.balance_date, "YYYY-MM-DD", t);
            },
        );
        ui.add_space(8.0);
    }
    if a.kind == AccountKind::Property {
        field(ui, "New value today (euros, optional)", t);
        text_box(ui, &mut f.value, "e.g. 410.000", t);
        ui.add_space(8.0);
    }
    if !a.unassigned {
        ui.checkbox(&mut f.closed, "Closed (keeps its history, leaves the lists)");
    }
    error_line(ui, &f.error, t);
    let mut out = Outcome::Keep;
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if !a.unassigned && widgets::secondary_button(ui, "Delete…", t).on_hover_text("Only an account without transactions can go").clicked() {
            out = Outcome::Run(vec![("account.delete", json!({ "id": f.id.0 }))]);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = widgets::primary_button(ui, "Save", t).clicked();
            if widgets::secondary_button(ui, "Cancel", t).clicked() {
                out = Outcome::Close;
            }
            if save {
                let mut p = json!({ "id": f.id.0, "closed": f.closed });
                if !a.unassigned {
                    p["name"] = json!(f.name);
                }
                if a.kind.is_physical() && a.kind != AccountKind::Cash {
                    p["iban"] = if f.iban.trim().is_empty() { Json::Null } else { json!(f.iban) };
                }
                if a.kind == AccountKind::Category {
                    p["description"] = json!(f.description);
                }
                if a.kind == AccountKind::Pot && !a.unassigned {
                    p["goal"] = if f.goal.trim().is_empty() { Json::Null } else { json!(f.goal) };
                    p["goalDate"] = if f.goal_date.trim().is_empty() { Json::Null } else { json!(f.goal_date.trim()) };
                }
                let mut cmds = vec![("account.update", p)];
                if physical_or_pot && (f.balance.trim(), f.balance_date.trim()) != (f.balance_was.0.trim(), f.balance_was.1.trim()) {
                    let amount = if f.balance.trim().is_empty() { "0".to_string() } else { f.balance.clone() };
                    cmds.push(("account.setBalance", json!({ "id": f.id.0, "amount": amount, "date": f.balance_date.trim() })));
                }
                if a.kind == AccountKind::Property && !f.value.trim().is_empty() {
                    cmds.push(("account.setValue", json!({ "id": f.id.0, "amount": f.value })));
                }
                out = Outcome::Run(cmds);
            }
        });
    });
    out
}

fn pot_move(s: &Session, ui: &mut Ui, f: &mut PotMoveForm, t: &Tokens) -> Outcome {
    fit(ui, 480.0);
    if title_bar(ui, "Move money between pots", t) {
        return Outcome::Close;
    }
    ui.label(egui::RichText::new("Pots split the money of one account; the bank sees nothing of this.").size(12.5).color(t.text_dim));
    ui.add_space(12.0);
    let tree = s.book().tree();
    let account = f.from.and_then(|a| tree.physical_of(a));
    ui.horizontal(|ui| {
        field(ui, "From", t);
        if let Some(a) = pickers::pill(ui, s.book(), Choices::Any, f.from, "A pot or account", t) {
            f.from = Some(a);
        }
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 14.0), Sense::hover());
        icons::glyph(ui.painter(), r.center(), 12.0, Glyph::Arrow, t.text_faint);
        field(ui, "To", t);
        let choices = match account {
            Some(acc) => Choices::PotsOf(acc),
            None => Choices::Any,
        };
        if let Some(a) = pickers::pill(ui, s.book(), choices, f.to, "Another pot", t) {
            f.to = Some(a);
        }
    });
    ui.add_space(8.0);
    two_columns(
        ui,
        |ui| {
            field(ui, "Amount (euros)", t);
            text_box(ui, &mut f.amount, "100,00", t);
        },
        |ui| {
            field(ui, "Date", t);
            text_box(ui, &mut f.date, "YYYY-MM-DD", t);
        },
    );
    ui.add_space(8.0);
    field(ui, "Note", t);
    text_box(ui, &mut f.note, "Optional", t);
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Move money", t);
    if cancel {
        return Outcome::Close;
    }
    if go {
        let (Some(from), Some(to)) = (f.from, f.to) else {
            f.error = Some("Choose both pots.".into());
            return Outcome::Keep;
        };
        return Outcome::Run(vec![("pot.move", json!({ "from": from.0, "to": to.0, "amount": f.amount, "date": f.date.trim(), "note": f.note }))]);
    }
    Outcome::Keep
}

fn split(s: &Session, ui: &mut Ui, f: &mut SplitForm, t: &Tokens) -> Outcome {
    fit(ui, 540.0);
    if title_bar(ui, "Split transaction", t) {
        return Outcome::Close;
    }
    ui.label(
        egui::RichText::new(format!(
            "Divide {} over several categories or accounts (a mortgage payment: interest and repayment).",
            f.total.abs().format()
        ))
        .size(12.5)
        .color(t.text_dim),
    );
    ui.add_space(12.0);
    let mut remove = None;
    let mut sum = Money::ZERO;
    for (i, (acc, amount)) in f.parts.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            if let Some(a) = pickers::pill(ui, s.book(), Choices::Counter, *acc, "Choose…", t) {
                *acc = Some(a);
            }
            ui.add(egui::TextEdit::singleline(amount).hint_text("0,00").desired_width(110.0));
            if widgets::icon_button(ui, Glyph::Trash, "Remove this part", t).clicked() {
                remove = Some(i);
            }
        });
        sum += Money::parse(amount).unwrap_or_default();
    }
    if let Some(i) = remove
        && f.parts.len() > 1
    {
        f.parts.remove(i);
    }
    if ui.link("+ Add a part").clicked() {
        let left = f.total - sum;
        f.parts.push((None, if left.is_zero() { String::new() } else { left.format_plain() }));
    }
    ui.add_space(6.0);
    let left = f.total - sum;
    let col = if left.is_zero() { t.positive } else { t.warning };
    ui.label(
        egui::RichText::new(if left.is_zero() { "The parts add up.".to_string() } else { format!("Still to divide: {}", left.format()) })
            .size(12.5)
            .color(col),
    );
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Split", t);
    if cancel {
        return Outcome::Close;
    }
    if go {
        let mut parts = Vec::new();
        for (acc, amount) in &f.parts {
            let Some(acc) = acc else {
                f.error = Some("Choose an account or category for every part.".into());
                return Outcome::Keep;
            };
            parts.push(json!({ "account": acc.0, "amount": amount }));
        }
        return Outcome::Run(vec![("txn.split", json!({ "id": f.id.0, "parts": parts }))]);
    }
    Outcome::Keep
}

fn import(s: &Session, ui: &mut Ui, f: &mut ImportForm, t: &Tokens) -> Outcome {
    fit(ui, 600.0);
    if title_bar(ui, &format!("Import {}", f.file.name()), t) {
        return Outcome::Close;
    }
    ui.add_space(6.0);
    let Some(p) = f.preview.clone() else {
        error_line(ui, &f.error, t);
        let (_, cancel) = buttons(ui, "Close", t);
        return if cancel { Outcome::Close } else { Outcome::Keep };
    };
    if p["recognised"] == json!(false) {
        return mapping(ui, f, &p, t);
    }
    let fact = |ui: &mut Ui, label: &str, value: String| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(130.0, 22.0), Sense::hover());
            ui.painter().text(pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, label, theme::body(12.5), t.text_dim);
            ui.label(egui::RichText::new(value).size(13.0).color(t.text));
        });
    };
    fact(ui, "Format", p["format"].as_str().unwrap_or("").to_string());
    fact(ui, "Rows", format!("{} from {} to {}", p["rows"], p["first"].as_str().unwrap_or("?"), p["last"].as_str().unwrap_or("?")));
    if let Some(ibans) = p["ibans"].as_array().filter(|a| !a.is_empty()) {
        fact(ui, "Account in file", ibans.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(", "));
    }
    let mut out = Outcome::Keep;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(130.0, 28.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, "Import into", theme::body(12.5), t.text_dim);
        if let Some(a) = pickers::pill(ui, s.book(), Choices::Physical, f.account, "Choose an account", t) {
            f.account = Some(a);
            out = Outcome::Refresh;
        }
        if f.account.is_none() && widgets::secondary_button(ui, "New account…", t).clicked() {
            let iban = p["ibans"].get(0).and_then(Json::as_str).unwrap_or("").to_string();
            f.error = Some(format!(
                "Add the account first (Accounts → New account){}, then import again.",
                if iban.is_empty() { String::new() } else { format!(" with IBAN {iban}") }
            ));
        }
    });
    if let Some(r) = p.get("import").filter(|r| r.is_object()) {
        ui.add_space(10.0);
        egui::Frame::new().fill(t.sidebar).corner_radius(8).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let n = |k: &str| r[k].as_u64().unwrap_or(0);
            ui.label(
                egui::RichText::new(format!("{} new transaction{}", n("new"), if n("new") == 1 { "" } else { "s" }))
                    .font(theme::bold(14.0))
                    .color(t.text),
            );
            ui.label(
                egui::RichText::new(format!("{} sorted by your rules and history · {} for you to review", n("sorted"), n("toReview")))
                    .size(12.5)
                    .color(t.text_dim),
            );
            let mut more = Vec::new();
            if n("known") > 0 {
                more.push(format!("{} already in the book", n("known")));
            }
            if n("transfers") > 0 {
                more.push(format!("{} matched to the other side of a transfer", n("transfers")));
            }
            if n("duplicates") > 0 {
                more.push(format!("{} possible duplicates", n("duplicates")));
            }
            if !more.is_empty() {
                ui.label(egui::RichText::new(more.join(" · ")).size(12.5).color(t.text_dim));
            }
        });
    }
    for w in p["warnings"].as_array().into_iter().flatten().take(3) {
        ui.label(egui::RichText::new(w.as_str().unwrap_or("")).size(11.5).color(t.warning));
    }
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Import", t);
    if cancel {
        return Outcome::Close;
    }
    if go {
        if f.account.is_none() {
            f.error = Some("Choose the account this file belongs to.".into());
            return Outcome::Keep;
        }
        return Outcome::Run(vec![("import.file", f.params()), ("view.transactions", json!({}))]);
    }
    out
}

/// A CSV no layout fits: the first rows, and which column is what.
fn mapping(ui: &mut Ui, f: &mut ImportForm, p: &Json, t: &Tokens) -> Outcome {
    ui.label(egui::RichText::new(p["message"].as_str().unwrap_or("")).size(12.5).color(t.text_dim));
    ui.add_space(8.0);
    let headers: Vec<String> =
        p["csv"]["headers"].as_array().map(|h| h.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if headers.is_empty() {
        error_line(ui, &Some("This file has no columns Pecunia can read.".into()), t);
        let (_, cancel) = buttons(ui, "Close", t);
        return if cancel { Outcome::Close } else { Outcome::Keep };
    }
    egui::ScrollArea::horizontal().max_height(140.0).show(ui, |ui| {
        egui::Grid::new("csv-preview").striped(true).show(ui, |ui| {
            for h in &headers {
                ui.label(egui::RichText::new(h).font(theme::bold(11.5)));
            }
            ui.end_row();
            for row in p["csv"]["rows"].as_array().into_iter().flatten().take(5) {
                for cell in row.as_array().into_iter().flatten() {
                    let text: String = cell.as_str().unwrap_or("").chars().take(30).collect();
                    ui.label(egui::RichText::new(text).size(11.5));
                }
                ui.end_row();
            }
        });
    });
    ui.add_space(10.0);
    let m = &mut f.mapping;
    let column = |ui: &mut Ui, label: &str, value: &mut String, optional: bool| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(140.0, 24.0), Sense::hover());
            ui.painter().text(pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, label, theme::body(12.5), t.text_dim);
            egui::ComboBox::from_id_salt(label).selected_text(if value.is_empty() { "—".to_string() } else { value.clone() }).width(220.0).show_ui(
                ui,
                |ui| {
                    if optional {
                        ui.selectable_value(value, String::new(), "—");
                    }
                    for h in &headers {
                        ui.selectable_value(value, h.clone(), h);
                    }
                },
            );
        });
    };
    column(ui, "Date", &mut m.date, false);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(140.0, 24.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, "Date looks like", theme::body(12.5), t.text_dim);
        for (fmt, ex) in [("yyyy-MM-dd", "2026-10-16"), ("dd-MM-yyyy", "16-10-2026"), ("yyyyMMdd", "20261016")] {
            ui.radio_value(&mut m.date_format, fmt.to_string(), ex);
        }
    });
    column(ui, "Amount", &mut m.amount, false);
    ui.horizontal(|ui| {
        ui.add_space(144.0);
        ui.checkbox(&mut m.decimal_comma, "Amounts use a decimal comma (12,50)");
    });
    column(ui, "Other party", &mut m.payee, true);
    column(ui, "Description", &mut m.description, true);
    column(ui, "Other party's IBAN", &mut m.iban, true);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(140.0, 24.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, "Save layout as", theme::body(12.5), t.text_dim);
        ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(220.0));
    });
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Save layout and preview", t);
    if cancel {
        return Outcome::Close;
    }
    if go {
        if m.date.is_empty() || m.amount.is_empty() || m.name.trim().is_empty() {
            f.error = Some("Choose the date and amount columns, and name the layout.".into());
            return Outcome::Keep;
        }
        let delimiter = p["csv"]["delimiter"].as_str().unwrap_or(",").to_string();
        let opt = |s: &String| if s.is_empty() { Json::Null } else { json!(s) };
        let description: Vec<&String> = [&m.description].into_iter().filter(|d| !d.is_empty()).collect();
        return Outcome::SaveProfile(json!({
            "name": m.name.trim(),
            "bank": m.name.trim(),
            "delimiter": delimiter,
            "has_header": true,
            "date_column": m.date,
            "date_format": m.date_format,
            "amount_column": m.amount,
            "decimal_comma": m.decimal_comma,
            "name_column": opt(&m.payee),
            "counter_iban_column": opt(&m.iban),
            "description_columns": description,
        }));
    }
    Outcome::Keep
}

/// `in_browser`: the host keeps books in the browser's storage, not in files (the web app on a phone).
fn new_book(ui: &mut Ui, f: &mut NewBookForm, in_browser: bool, t: &Tokens) -> Outcome {
    fit(ui, 440.0);
    if !f.touched {
        f.categories = true;
        f.touched = true;
    }
    if title_bar(ui, "New book", t) {
        return Outcome::Close;
    }
    let about = if in_browser {
        "The book is kept in this browser, on this device only. Nothing goes to a server. Save a copy now and then: the book menu has Save a copy."
    } else {
        "A book is one file on your computer. Nothing goes to a server; keep it somewhere private."
    };
    ui.label(egui::RichText::new(about).size(12.5).color(t.text_dim));
    ui.add_space(12.0);
    field(ui, "Name", t);
    let name = text_box(ui, &mut f.name, "e.g. Our household", t);
    if !f.focused {
        name.request_focus();
        f.focused = true;
    }
    ui.add_space(8.0);
    ui.checkbox(&mut f.categories, "Start with the usual household categories");
    ui.checkbox(&mut f.sample, "Fill it with the made-up demo household to try things out");
    ui.add_space(10.0);
    field(ui, "Password (optional)", t);
    password_box(ui, &mut f.password, "Leave empty for no password", t);
    if !f.password.is_empty() {
        ui.add_space(6.0);
        password_box(ui, &mut f.repeat, "Type it again", t);
        ui.label(egui::RichText::new("The book is encrypted. Without the password nobody can open it, not even you.").size(11.5).color(t.warning));
    }
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, if in_browser { "Create book" } else { "Choose location…" }, t);
    if cancel {
        return Outcome::Close;
    }
    if go || ui.input(|i| i.key_pressed(Key::Enter)) {
        if !f.password.is_empty()
            && let Some(problem) = password_problem(&f.password, &f.repeat)
        {
            f.error = Some(problem);
            return Outcome::Keep;
        }
        let name = if f.name.trim().is_empty() { "Household".to_string() } else { f.name.trim().to_string() };
        let mut p = json!({ "name": name, "categories": f.categories, "sample": f.sample });
        if !f.password.is_empty() {
            p["password"] = json!(f.password);
        }
        return Outcome::File(FileRequest::New(p));
    }
    Outcome::Keep
}

fn rename(ui: &mut Ui, f: &mut RenameForm, t: &Tokens) -> Outcome {
    fit(ui, 400.0);
    if title_bar(ui, "Rename book", t) {
        return Outcome::Close;
    }
    ui.add_space(8.0);
    field(ui, "Name", t);
    let name = text_box(ui, &mut f.name, "Name", t);
    if !f.focused {
        name.request_focus();
        f.focused = true;
    }
    let (go, cancel) = buttons(ui, "Save", t);
    if cancel {
        return Outcome::Close;
    }
    if go || ui.input(|i| i.key_pressed(Key::Enter)) {
        return Outcome::Run(vec![("book.rename", json!({ "name": f.name }))]);
    }
    Outcome::Keep
}

fn unlock(ui: &mut Ui, f: &mut UnlockForm, t: &Tokens) -> Outcome {
    fit(ui, 420.0);
    if title_bar(ui, "Password needed", t) {
        return Outcome::Close;
    }
    ui.label(egui::RichText::new(format!("{} is protected with a password.", f.name)).size(12.5).color(t.text_dim));
    ui.add_space(10.0);
    let r = password_box(ui, &mut f.password, "Password", t);
    if !f.focused {
        r.request_focus();
        f.focused = true;
    }
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, "Open", t);
    if cancel {
        return Outcome::Close;
    }
    if (go || ui.input(|i| i.key_pressed(Key::Enter))) && !f.password.is_empty() {
        let pw = std::mem::take(&mut f.password);
        return match &f.path {
            Some(path) => Outcome::Run(vec![("file.open", json!({ "path": path, "password": pw }))]),
            None => Outcome::Unlocked(pw),
        };
    }
    Outcome::Keep
}

fn set_password(s: &Session, ui: &mut Ui, f: &mut SetPasswordForm, t: &Tokens) -> Outcome {
    fit(ui, 440.0);
    let change = s.encrypted();
    if title_bar(ui, if change { "Change the password" } else { "Protect with a password" }, t) {
        return Outcome::Close;
    }
    ui.label(
        egui::RichText::new(
            "The file is encrypted with this password. Without it nobody can open the book, not even you: there is no way to get it back.",
        )
        .size(12.5)
        .color(t.text_dim),
    );
    ui.add_space(10.0);
    field(ui, "New password", t);
    let r = password_box(ui, &mut f.password, "At least 8 characters", t);
    if !f.focused {
        r.request_focus();
        f.focused = true;
    }
    ui.add_space(6.0);
    password_box(ui, &mut f.repeat, "Type it again", t);
    error_line(ui, &f.error, t);
    let (go, cancel) = buttons(ui, if change { "Change password" } else { "Protect" }, t);
    if cancel {
        return Outcome::Close;
    }
    if go || ui.input(|i| i.key_pressed(Key::Enter)) {
        if let Some(problem) = password_problem(&f.password, &f.repeat) {
            f.error = Some(problem);
            return Outcome::Keep;
        }
        return Outcome::Run(vec![("file.setPassword", json!({ "password": f.password }))]);
    }
    Outcome::Keep
}
