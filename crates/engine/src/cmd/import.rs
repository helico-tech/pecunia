//! Importing bank files (ADR 0004, 0005). The file comes as a `path` (desktop, CLI, MCP) or as `contentBase64` with a
//! `name` (the browser). The format is recognised from the content; the account from the IBAN in the file, or the
//! `account` param.

use base64::Engine as _;
use pecunia_model::{AccountId, CsvProfile, DetectedFormat, ImportId, ImportSource, IngestResult, ParsedStatement};
use serde_json::{Value as Json, json};

use super::*;
use crate::UiRequest;

/// Bank files larger than this are refused.
pub const MAX_BANK_FILE: usize = 64 * 1024 * 1024;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Edit "import.file", "Import Bank File…", ["File"], Some("⌘I"),
            "path (or contentBase64 and name), account? (default: the account with the IBAN in the file), profile? (a CSV layout name), dryRun? (bool). CSV of Dutch banks, CAMT.053 or MT940. Without a file the UI asks.",
            always, import_file),
        cmd!(Query "import.preview", "Preview Bank File", [], None,
            "path (or contentBase64 and name), account?, profile?. What the file is, which account it belongs to and what an import would do; for a CSV no layout fits, the first rows to map columns.",
            always, preview),
        cmd!(Edit "import.undo", "Undo an Import", [], None, "id (import number). Removes what that import brought that nobody checked since.", always, undo),
        cmd!(Edit "import.saveProfile", "Save CSV Layout", [], None,
            "profile: {name, bank, delimiter, has_header, skip_lines, date_column, date_format (yyyyMMdd|yyyy-MM-dd|dd-MM-yyyy), amount_column, decimal_comma, sign (SIGNED|INDICATOR), indicator_column, debit_values, name_column, counter_iban_column, description_columns, own_iban_column, external_id_column, encoding}",
            always, save_profile),
        cmd!(Edit "import.deleteProfile", "Delete CSV Layout", [], None, "name", always, delete_profile),
    ]
}

/// The bytes and the file name from the params, or `None` when the params name no file.
fn content(cmd: &str, p: &Json) -> Result<Option<(Vec<u8>, String)>> {
    if let Some(b64) = opt_string(cmd, p, "contentBase64")? {
        if b64.len() > MAX_BANK_FILE / 3 * 4 + 4 {
            return Err(bad(cmd, "the file is larger than 64 MB"));
        }
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| bad(cmd, format!("`contentBase64` is not base64: {e}")))?;
        let name = opt_string(cmd, p, "name")?.unwrap_or_else(|| "bank file".into());
        return Ok(Some((bytes, name)));
    }
    let Some(path) = opt_string(cmd, p, "path")?.filter(|x| !x.trim().is_empty()) else { return Ok(None) };
    let meta = std::fs::metadata(&path).map_err(|e| bad(cmd, format!("can't read {path}: {e}")))?;
    if meta.len() > MAX_BANK_FILE as u64 {
        return Err(bad(cmd, format!("{path} is larger than 64 MB")));
    }
    let bytes = std::fs::read(&path).map_err(|e| bad(cmd, format!("can't read {path}: {e}")))?;
    let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("bank file").to_string();
    Ok(Some((bytes, name)))
}

/// What a file is and what it holds.
struct Read {
    format: DetectedFormat,
    statement: ParsedStatement,
}

fn read(s: &Session, cmd: &str, p: &Json, bytes: &[u8]) -> Result<std::result::Result<Read, String>> {
    let profiles = &s.book().meta.profiles;
    let format = match opt_string(cmd, p, "profile")? {
        Some(name) => {
            let known = pecunia_formats::built_in_profiles().iter().chain(profiles.iter()).any(|x| x.name.eq_ignore_ascii_case(&name));
            if !known {
                return Err(bad(cmd, format!("no CSV layout named {}", short(&Json::String(name)))));
            }
            DetectedFormat::csv(name)
        }
        None => match pecunia_formats::detect(bytes, profiles) {
            Some(f) => f,
            None => return Ok(Err("No known bank file layout fits this file. Map its columns as a new CSV layout.".into())),
        },
    };
    match pecunia_formats::parse(bytes, &format, profiles) {
        Ok(statement) => Ok(Ok(Read { format, statement })),
        Err(e) => Err(bad(cmd, format!("{} can't be read as {}: {e}", "the file", format.label()))),
    }
}

/// The account a file belongs to: the `account` param, else the own account with an IBAN the file names.
fn target_account(s: &Session, cmd: &str, p: &Json, st: &ParsedStatement) -> Result<Option<AccountId>> {
    if let Some(a) = account_param(s, cmd, p, "account")? {
        return Ok(Some(a));
    }
    let tree = s.book().tree();
    let from_rows = st.rows.iter().filter_map(|r| r.own_iban.as_deref());
    Ok(st.account_ibans.iter().map(String::as_str).chain(from_rows).find_map(|i| tree.find_by_iban(i).filter(|a| a.kind.is_physical()).map(|a| a.id)))
}

fn result_json(s: &Session, r: &IngestResult, format: &DetectedFormat, account: AccountId, st: &ParsedStatement) -> Json {
    json!({
        "account": s.book().account_label(account),
        "accountId": account.0,
        "format": format.label(),
        "rows": r.rows,
        "new": r.sorted + r.to_review,
        "sorted": r.sorted,
        "toReview": r.to_review,
        "known": r.known,
        "transfers": r.transfers,
        "duplicates": r.duplicates,
        "skipped": r.skipped,
        "first": r.first,
        "last": r.last,
        "import": r.import.map(|i| i.0),
        "warnings": st.warnings.iter().take(20).collect::<Vec<_>>(),
    })
}

fn import_file(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "import.file";
    let Some((bytes, name)) = content(C, p)? else {
        s.request(UiRequest::Dialog("import".into(), json!({})));
        return Ok(json!({ "dialog": "import" }));
    };
    let read = read(s, C, p, &bytes)?.map_err(|why| bad(C, why))?;
    let account = target_account(s, C, p, &read.statement)?
        .ok_or_else(|| bad(C, "the file names no IBAN of yours; say which account it belongs to with `account`"))?;
    let dry = bool_param(p, "dryRun").unwrap_or(false);
    let source = ImportSource { file: name, format: read.format.label(), balances: read.statement.closing_balances.clone() };
    let now = s.clock();
    let r = s.book_mut().ingest(account, &read.statement.rows, read.format.source_kind(), &source, now, dry)?;
    if !dry {
        s.view.filter = pecunia_model::register::Filter { import: r.import, ..Default::default() };
        let text = format!(
            "Imported {} new transaction{} into {}; {} to review",
            r.new.len(),
            if r.new.len() == 1 { "" } else { "s" },
            s.book().account_label(account),
            r.to_review
        );
        s.notify(text, !r.new.is_empty());
    }
    Ok(result_json(s, &r, &read.format, account, &read.statement))
}

fn preview(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "import.preview";
    let (bytes, name) = content(C, p)?.ok_or_else(|| bad(C, "missing `path` (or `contentBase64` and `name`)"))?;
    let read = match read(s, C, p, &bytes)? {
        Ok(r) => r,
        Err(why) => {
            let csv = pecunia_formats::preview_csv(&bytes, 8);
            return Ok(json!({
                "file": name,
                "recognised": false,
                "message": why,
                "csv": csv.map(|c| json!({ "delimiter": c.delimiter.to_string(), "headers": c.headers, "rows": c.rows })),
            }));
        }
    };
    let account = target_account(s, C, p, &read.statement)?;
    let mut out = json!({
        "file": name,
        "recognised": true,
        "format": read.format.label(),
        "profile": read.format.profile,
        "ibans": read.statement.account_ibans,
        "rows": read.statement.rows.len(),
        "first": read.statement.rows.iter().map(|r| r.date).min(),
        "last": read.statement.rows.iter().map(|r| r.date).max(),
        "warnings": read.statement.warnings.iter().take(20).collect::<Vec<_>>(),
    });
    if let Some(account) = account {
        let mut copy = s.book().clone();
        let source = ImportSource { file: name, format: read.format.label(), balances: Vec::new() };
        let r = copy.ingest(account, &read.statement.rows, read.format.source_kind(), &source, s.clock(), true)?;
        out["import"] = result_json(s, &r, &read.format, account, &read.statement);
        out["account"] = json!(s.book().account_label(account));
        out["accountId"] = json!(account.0);
    }
    Ok(out)
}

fn undo(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "import.undo";
    let id = p.get("id").and_then(Json::as_u64).map(ImportId).ok_or_else(|| bad(C, "missing `id` (an import number)"))?;
    let n = s.book_mut().undo_import(id)?;
    s.notify(format!("Removed {n} transaction{} of that import", if n == 1 { "" } else { "s" }), true);
    Ok(json!({ "removed": n }))
}

fn save_profile(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "import.saveProfile";
    let v = p.get("profile").cloned().ok_or_else(|| bad(C, "missing `profile`"))?;
    let profile: CsvProfile = serde_json::from_value(v).map_err(|e| bad(C, format!("`profile`: {e}")))?;
    if pecunia_formats::built_in_profiles().iter().any(|b| b.name.eq_ignore_ascii_case(&profile.name)) {
        return Err(bad(C, "that name belongs to a built-in layout; pick another"));
    }
    let name = profile.name.clone();
    s.book_mut().save_profile(profile)?;
    Ok(json!({ "saved": name }))
}

fn delete_profile(s: &mut Session, p: &Json) -> Result<Json> {
    let name = req_string("import.deleteProfile", p, "name")?;
    s.book_mut().delete_profile(&name)?;
    Ok(json!({ "deleted": name }))
}
