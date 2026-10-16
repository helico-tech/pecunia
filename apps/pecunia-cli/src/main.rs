#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Pecunia on the command line. Every subcommand is a thin wrapper over engine commands, so the CLI, the app and MCP
//! behave the same.

use std::io::{self, Write};
use std::process::ExitCode;

use pecunia_engine::model::Money;
use pecunia_engine::store::StoreError;
use pecunia_engine::{EngineError, Session, command_specs};
use serde_json::{Map, Value as Json, json};

const USAGE: &str = "\
pecunia-cli — a household book in a .pecunia file

USAGE:
  pecunia-cli init <file> [--name NAME] [--no-categories] [--sample] [--encrypt]
  pecunia-cli accounts <file> [--json]
  pecunia-cli add-account <file> --kind K --name NAME [--parent P] [--iban IBAN] [--balance EUROS] [--date D]
                          (kinds: current, savings, cash, property, credit_card, loan, pot, category_group, category)
  pecunia-cli import <file> <bank-file> [--account A] [--dry-run]   (CSV of Dutch banks, CAMT.053, MT940)
  pecunia-cli list <file> [--account A] [--text T] [--from D] [--to D] [--review] [--limit N] [--json]
  pecunia-cli show <file> <ID> [--json]
  pecunia-cli add <file> --payee P --from A --to B --amount EUROS [--date D] [--note N]
  pecunia-cli review <file> [--tab duplicate|transfer|suggested|unknown] [--json]
  pecunia-cli answer <file> <ID> <CATEGORY-OR-ACCOUNT> [--always]
  pecunia-cli report <file> overview|net-worth|income|cash-flow|fixed-costs|bank-check [--from D] [--to D]
  pecunia-cli run <file> --cmd 'id={json}'...     (any command; see `commands`)
  pecunia-cli commands [--json]
  pecunia-cli password <file> (--new-password-file F | --remove)
  pecunia-cli mcp <file>                          (MCP server on stdio)

Amounts you give are euros (12.50 or 12,50); dates are YYYY-MM-DD. JSON answers give amounts in cents.
A protected book is opened with the password in PECUNIA_PASSWORD or in the file --password-file names
(`init --encrypt` protects a new book with that password; `password` sets, changes or removes it).
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(m)) => {
            eprintln!("{m}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Error(m)) => {
            eprintln!("error: {m}");
            ExitCode::from(1)
        }
    }
}

enum Failure {
    Usage(String),
    Error(String),
}

fn err(e: impl std::fmt::Display) -> Failure {
    Failure::Error(e.to_string())
}

/// `--flag value` pairs and bare flags, after the positional arguments.
struct Opts {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

const BARE: &[&str] = &["--sample", "--json", "--review", "--always", "--dry-run", "--no-categories", "--encrypt", "--remove"];

fn parse(args: &[String]) -> Result<Opts, Failure> {
    let mut o = Opts { positional: Vec::new(), flags: Vec::new() };
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if let Some(name) = a.strip_prefix("--") {
            if BARE.contains(&a.as_str()) {
                o.flags.push((name.to_string(), None));
            } else {
                let v = args.get(i + 1).cloned().ok_or_else(|| Failure::Usage(format!("{a} needs a value")))?;
                o.flags.push((name.to_string(), Some(v)));
                i += 1;
            }
        } else {
            o.positional.push(a.clone());
        }
        i += 1;
    }
    Ok(o)
}

impl Opts {
    fn get(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }
    fn all(&self, name: &str) -> Vec<&str> {
        self.flags.iter().filter(|(n, _)| n == name).filter_map(|(_, v)| v.as_deref()).collect()
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
    fn pos(&self, i: usize, what: &str) -> Result<&str, Failure> {
        self.positional.get(i).map(String::as_str).ok_or_else(|| Failure::Usage(format!("missing {what}")))
    }
    /// The flags in `keys` as JSON params (flag `--from` → param `from`).
    fn params(&self, keys: &[(&str, &str)]) -> Map<String, Json> {
        keys.iter().filter_map(|(flag, key)| self.get(flag).map(|v| (key.to_string(), json!(v)))).collect()
    }
}

/// The password from `--password-file` (its first line) or `PECUNIA_PASSWORD`.
fn password(o: &Opts) -> Result<Option<String>, Failure> {
    if let Some(f) = o.get("password-file") {
        let text = std::fs::read_to_string(f).map_err(|e| Failure::Error(format!("can't read {f}: {e}")))?;
        return Ok(text.lines().next().map(str::to_string).filter(|p| !p.is_empty()));
    }
    Ok(std::env::var("PECUNIA_PASSWORD").ok().filter(|p| !p.is_empty()))
}

fn open(path: &str, o: &Opts) -> Result<Session, Failure> {
    let pw = password(o)?;
    let mut s = Session::open_with(path, pw.as_deref()).map_err(|e| match e {
        EngineError::Store(StoreError::Locked) => {
            Failure::Error(format!("{path} is protected with a password: set PECUNIA_PASSWORD or pass --password-file"))
        }
        e => err(e),
    })?;
    for n in s.take_notices() {
        eprintln!("{}", n.text);
    }
    Ok(s)
}

fn exec(s: &mut Session, id: &str, params: Json) -> Result<Json, Failure> {
    s.execute_headless(id, params).map_err(err)
}

fn print_json(v: &Json) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
}

/// Cents in an answer as `€ 1.234,56`.
fn money(v: &Json) -> String {
    v.as_i64().map(|c| Money(c).format()).unwrap_or_default()
}

fn s<'a>(v: &'a Json, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn run(args: &[String]) -> Result<(), Failure> {
    let Some((sub, rest)) = args.split_first() else { return Err(Failure::Usage("no command given".into())) };
    let o = parse(rest)?;
    let mut out = io::stdout().lock();
    match sub.as_str() {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        "init" => {
            let path = o.pos(0, "<file>")?;
            let mut sess = Session::scratch();
            let mut p = json!({ "path": path, "sample": o.has("sample"), "categories": !o.has("no-categories") });
            if o.has("encrypt") {
                let pw = password(&o)?.ok_or_else(|| Failure::Usage("--encrypt needs PECUNIA_PASSWORD or --password-file".into()))?;
                p["password"] = json!(pw);
            }
            if let Some(n) = o.get("name") {
                p["name"] = json!(n);
            }
            let r = exec(&mut sess, "file.new", p)?;
            println!("Created {} ({})", s(&r, "location"), s(&r, "name"));
            Ok(())
        }
        "accounts" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let r = exec(&mut sess, "accounts.list", json!({}))?;
            if o.has("json") {
                print_json(&r);
                return Ok(());
            }
            for a in r["accounts"].as_array().into_iter().flatten() {
                let depth = a["depth"].as_u64().unwrap_or(0) as usize;
                let name = format!("{}{}", "  ".repeat(depth), s(a, "name"));
                let _ = writeln!(out, "{:>4}  {:<40} {:>16}  {}", a["id"], name, money(&a["balance"]), s(a, "kind"));
            }
            Ok(())
        }
        "add-account" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let p = o.params(&[
                ("kind", "kind"),
                ("name", "name"),
                ("parent", "parent"),
                ("iban", "iban"),
                ("balance", "balance"),
                ("date", "balanceDate"),
                ("group", "group"),
            ]);
            let r = exec(&mut sess, "account.create", Json::Object(p))?;
            println!("{} {}", r["id"], s(&r, "name"));
            Ok(())
        }
        "import" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let mut p = json!({ "path": o.pos(1, "<bank-file>")?, "dryRun": o.has("dry-run") });
            if let Some(a) = o.get("account") {
                p["account"] = json!(a);
            }
            if let Some(pr) = o.get("profile") {
                p["profile"] = json!(pr);
            }
            let r = exec(&mut sess, "import.file", p)?;
            if o.has("json") {
                print_json(&r);
                return Ok(());
            }
            let verb = if o.has("dry-run") { "Would import" } else { "Imported" };
            println!(
                "{verb} {} rows ({}) into {}: {} new ({} sorted, {} to review), {} already known, {} transfers matched, {} possible duplicates",
                r["rows"],
                s(&r, "format"),
                s(&r, "account"),
                r["new"],
                r["sorted"],
                r["toReview"],
                r["known"],
                r["transfers"],
                r["duplicates"]
            );
            Ok(())
        }
        "list" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let mut p = o.params(&[("account", "account"), ("text", "text"), ("from", "from"), ("to", "to"), ("status", "status")]);
            p.insert("review".into(), json!(o.has("review")));
            p.insert("limit".into(), json!(o.get("limit").and_then(|l| l.parse::<u64>().ok()).unwrap_or(100)));
            let r = exec(&mut sess, "txns.list", Json::Object(p))?;
            if o.has("json") {
                print_json(&r);
                return Ok(());
            }
            let rows = r["transactions"].as_array().cloned().unwrap_or_default();
            for t in &rows {
                let arrow = format!("{} → {}", s(t, "from"), s(t, "to"));
                let bal = if t["balance"].is_null() { String::new() } else { money(&t["balance"]) };
                let flag = t["review"].as_str().map(|k| format!("  [{k}]")).unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{:>5} {} {:<34.34} {:<38.38} {:>14} {:>14}{flag}",
                    t["id"],
                    s(t, "date"),
                    s(t, "payee"),
                    arrow,
                    money(&t["amount"]),
                    bal
                );
            }
            let _ = writeln!(out, "{} of {} transactions", rows.len(), r["total"]);
            Ok(())
        }
        "show" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let r = exec(&mut sess, "txn.get", json!({ "id": o.pos(1, "<ID>")? }))?;
            if o.has("json") {
                print_json(&r);
                return Ok(());
            }
            println!("#{} {}  {}", r["id"], s(&r, "date"), s(&r, "payee"));
            println!("{} → {}  {}", s(&r, "from"), s(&r, "to"), money(&r["amount"]));
            println!("status {}{}", s(&r, "status"), r["review"].as_str().map(|k| format!(" · in Review: {k}")).unwrap_or_default());
            if !s(&r, "via").is_empty() {
                println!("via {}", s(&r, "via"));
            }
            for p in r["postings"].as_array().into_iter().flatten() {
                println!("  {:<40} {:>14}", s(p, "account"), money(&p["amount"]));
            }
            for g in r["guesses"].as_array().into_iter().flatten() {
                println!("  guess: {} ({:.0}%)", s(g, "account"), g["probability"].as_f64().unwrap_or(0.0) * 100.0);
            }
            if !s(&r, "note").is_empty() {
                println!("\n{}", s(&r, "note"));
            }
            Ok(())
        }
        "add" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let p = o.params(&[
                ("payee", "payee"),
                ("from", "from"),
                ("to", "to"),
                ("amount", "amount"),
                ("date", "date"),
                ("note", "note"),
                ("who", "who"),
            ]);
            for need in ["from", "to", "amount"] {
                if !p.contains_key(need) {
                    return Err(Failure::Usage(format!("add needs --{need}")));
                }
            }
            let r = exec(&mut sess, "txn.create", Json::Object(p))?;
            println!("{}", r["id"]);
            Ok(())
        }
        "review" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let mut p = json!({});
            if let Some(t) = o.get("tab") {
                p["tab"] = json!(t);
            }
            let r = exec(&mut sess, "review.list", p)?;
            if o.has("json") {
                print_json(&r);
                return Ok(());
            }
            for t in r["items"].as_array().into_iter().flatten() {
                let guess = t["guesses"].get(0).map(|g| format!("  guess: {}", s(g, "account"))).unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{:>5} {} {:<11} {:<36.36} {:>14}{guess}",
                    t["id"],
                    s(t, "date"),
                    s(t, "review"),
                    s(t, "payee"),
                    money(&t["amount"])
                );
            }
            let _ = writeln!(out, "{} waiting", r["total"]);
            Ok(())
        }
        "answer" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let p = json!({ "id": o.pos(1, "<ID>")?, "to": o.pos(2, "<CATEGORY-OR-ACCOUNT>")?, "always": o.has("always") });
            let r = exec(&mut sess, "txn.answer", p)?;
            println!("#{} → {}", r["id"], s(&r, "to"));
            if let Some(rule) = r["rule"]["created"].as_str() {
                println!("new payee rule: {rule}");
            }
            Ok(())
        }
        "report" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            let id = match o.pos(1, "<report>")? {
                "overview" => "report.overview",
                "net-worth" => "report.netWorth",
                "income" => "report.incomeExpenses",
                "cash-flow" => "report.cashFlow",
                "fixed-costs" => "report.fixedCosts",
                "bank-check" => "report.bankCheck",
                other => return Err(Failure::Usage(format!("unknown report {other}"))),
            };
            let r = exec(&mut sess, id, Json::Object(o.params(&[("from", "from"), ("to", "to"), ("date", "date")])))?;
            print_json(&r);
            Ok(())
        }
        "run" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            for c in o.all("cmd") {
                let (id, params) = match c.split_once('=') {
                    Some((id, js)) => (id, serde_json::from_str(js).map_err(|e| Failure::Usage(format!("{id}: params are not JSON: {e}")))?),
                    None => (c, json!({})),
                };
                let r = exec(&mut sess, id, params)?;
                println!("{r}");
            }
            Ok(())
        }
        "commands" => {
            let sess = Session::scratch();
            if o.has("json") {
                let v: Vec<_> = command_specs().iter().map(|c| c.info(&sess)).collect();
                print_json(&serde_json::to_value(v).unwrap_or_default());
            } else {
                for c in command_specs() {
                    let _ = writeln!(out, "{:<26} {:<30} {}", c.id, c.label.trim_end_matches('…'), c.params);
                }
            }
            Ok(())
        }
        "password" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            if o.has("remove") {
                exec(&mut sess, "file.removePassword", json!({}))?;
                println!("The password is removed; the book is stored unencrypted.");
                return Ok(());
            }
            let f = o.get("new-password-file").ok_or_else(|| Failure::Usage("password needs --new-password-file or --remove".into()))?;
            let text = std::fs::read_to_string(f).map_err(|e| Failure::Error(format!("can't read {f}: {e}")))?;
            let new = text.lines().next().unwrap_or("").to_string();
            exec(&mut sess, "file.setPassword", json!({ "password": new }))?;
            println!("The book is protected. Keep the password safe: without it, nobody can open the book.");
            Ok(())
        }
        "mcp" => {
            let mut sess = open(o.pos(0, "<file>")?, &o)?;
            pecunia_mcp::serve(&mut sess, io::stdin().lock(), io::stdout().lock()).map_err(err)
        }
        other => Err(Failure::Usage(format!("unknown command {other}"))),
    }
}
