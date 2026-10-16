use std::path::Path;

use pecunia_model::Book;
use serde_json::{Value as Json, json};

use super::*;
use crate::UiRequest;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Plain "file.new", "New Book…", ["File"], Some("⇧⌘N"),
            "path (a new .pecunia file), name?, categories? (bool, default true: start with the usual household categories), sample? (bool: fill with the sample household)",
            always, new),
        cmd!(Plain "file.open", "Open Book…", ["File"], Some("⌘O"), "path", always, open),
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
    let book = new_book(p, &stem, s.clock())?;
    let store = pecunia_store::create(&path, &book)?;
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
    s.open_store(pecunia_store::open(&path)?)?;
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
