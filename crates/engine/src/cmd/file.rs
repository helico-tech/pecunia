use std::path::Path;

use pecunia_model::Book;
use serde_json::{Value as Json, json};

use super::*;
use crate::UiRequest;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Plain "file.new", "New Book…", ["File"], Some("⇧⌘N"),
            "path (a new .pecunia file), name?, password? (protect the file, ADR 0007; at least 8 characters), categories? (bool, default true: start with the usual household categories), sample? (bool: fill with the made-up demo household)",
            always, new),
        cmd!(Plain "file.open", "Open Book…", ["File"], Some("⌘O"), "path, password? (for a protected book)", always, open),
        cmd!(Plain "file.setPassword", "Protect With a Password…", ["File"], None,
            "password (at least 8 characters). Encrypts the file (or changes its password). There is no way back without the password.",
            is_file, set_password),
        cmd!(Plain "file.removePassword", "Remove the Password", ["File"], None, "Stores the book unencrypted again.", is_encrypted, remove_password),
        cmd!(Plain "file.save", "Save", ["File"], Some("⌘S"), "Saving is automatic; this retries a failed save.", always, save),
        cmd!(Plain "file.reload", "Reload", ["File"], None, "Reads the book again from where it is stored.", always, reload),
        cmd!(Edit "book.rename", "Rename Book", [], None, "name", always, rename),
    ]
}

fn new(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "file.new";
    let Some(path) = opt_string(C, p, "path")?.filter(|x| !x.trim().is_empty()) else {
        s.request(UiRequest::Dialog("newBook".into(), p.clone()));
        return Ok(json!({ "dialog": "newBook" }));
    };
    let stem = Path::new(&path).file_stem().and_then(|x| x.to_str()).unwrap_or("Household").to_string();
    let password = password_param(C, p)?;
    let book = new_book(p, &stem, s.clock())?;
    let store = pecunia_store::create(&path, &book, password.as_deref())?;
    s.replace(Some(store), book);
    s.selection = None;
    Ok(json!({ "location": s.location(), "name": s.book().name() }))
}

/// The book `file.new` creates from its params (`name?`, `categories?`, `sample?`); `default_name` is used when no
/// name is given. Apps that create the file themselves (the web app) use this too.
pub fn new_book(p: &Json, default_name: &str, now: i64) -> Result<Book> {
    const C: &str = "file.new";
    let name = opt_string(C, p, "name")?.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| default_name.to_string());
    if bool_param(p, "sample").unwrap_or(false) {
        let mut b = crate::sample::book(now);
        b.rename(&name)?;
        return Ok(b);
    }
    if bool_param(p, "categories").unwrap_or(true) { Ok(Book::with_starter_categories(&name)?) } else { Ok(Book::new(&name)?) }
}

fn open(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "file.open";
    let Some(path) = opt_string(C, p, "path")?.filter(|x| !x.trim().is_empty()) else {
        s.request(UiRequest::Dialog("open".into(), json!({})));
        return Ok(json!({ "dialog": "open" }));
    };
    let password = opt_string(C, p, "password")?.filter(|x| !x.is_empty());
    match s.open_store(pecunia_store::open(&path, password.as_deref())?) {
        Err(crate::EngineError::Store(pecunia_store::StoreError::Locked)) => {
            s.request(UiRequest::Dialog("unlock".into(), json!({ "path": path })));
            return Ok(json!({ "dialog": "unlock" }));
        }
        r => r?,
    }
    Ok(json!({ "location": s.location(), "name": s.book().name(), "transactions": s.book().txn_count() }))
}

fn save(s: &mut Session, _: &Json) -> Result<Json> {
    s.flush()?;
    Ok(json!({ "saved": true, "location": s.location() }))
}

fn reload(s: &mut Session, _: &Json) -> Result<Json> {
    let loaded = s.store.load()?;
    s.replace(None, loaded.book);
    Ok(json!({ "transactions": s.book().txn_count() }))
}

fn rename(s: &mut Session, p: &Json) -> Result<Json> {
    let name = req_string("book.rename", p, "name")?;
    s.book_mut().rename(&name)?;
    Ok(json!({ "name": s.book().name() }))
}

fn is_file(s: &Session) -> std::result::Result<(), String> {
    if s.store_path().is_some() { Ok(()) } else { Err("this book is not a file".into()) }
}

fn is_encrypted(s: &Session) -> std::result::Result<(), String> {
    if s.encrypted() { Ok(()) } else { Err("this book has no password".into()) }
}

/// The `password` param of a new or re-protected book: at least 8 characters.
fn password_param(cmd: &str, p: &Json) -> Result<Option<String>> {
    match opt_string(cmd, p, "password")?.filter(|x| !x.is_empty()) {
        Some(pw) if pw.chars().count() < 8 => Err(bad(cmd, "a password has at least 8 characters")),
        Some(pw) if pw.chars().count() > 1024 => Err(bad(cmd, "that password is too long")),
        other => Ok(other),
    }
}

fn set_password(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "file.setPassword";
    let password = password_param(C, p)?.ok_or_else(|| bad(C, "missing `password`"))?;
    // Everything must be saved first: the store writes the book now, under the new password.
    s.flush()?;
    let book = s.book().clone();
    s.store_mut().set_password(&book, Some(&password))?;
    s.notify("The book is protected with a password. Keep it safe: without it, nobody can open the book, not even you.", false);
    Ok(json!({ "encrypted": true }))
}

fn remove_password(s: &mut Session, _: &Json) -> Result<Json> {
    s.flush()?;
    let book = s.book().clone();
    s.store_mut().set_password(&book, None)?;
    s.notify("The password is removed; the book is stored unencrypted.", false);
    Ok(json!({ "encrypted": false }))
}
