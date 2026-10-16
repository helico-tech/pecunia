use pecunia_model::{AccountKind, Group, Money, TxnId, TxnStatus};
use serde_json::json;

use super::*;

fn fixed_now() -> i64 {
    1_791_936_000_000 // 2026-10-14
}

fn session() -> Session {
    let mut s = Session::scratch();
    s.now = fixed_now;
    s
}

fn stored(s: &mut Session) -> Book {
    s.store.load().unwrap().book
}

fn with_accounts() -> Session {
    let mut s = session();
    s.execute("account.create", json!({"kind": "current", "name": "ING", "iban": "NL12INGB0001234567", "balance": "1.000,00"})).unwrap();
    s.execute("account.create", json!({"kind": "savings", "name": "Spaar"})).unwrap();
    s.execute("account.create", json!({"kind": "pot", "name": "Holiday", "parent": "Spaar", "goal": 2500})).unwrap();
    s.execute("account.create", json!({"kind": "cash", "name": "Wallet", "balance": 50})).unwrap();
    s
}

#[test]
fn accounts_transactions_autosave_and_undo() {
    let mut s = with_accounts();
    let tree = s.book().tree();
    let ing = s.book().resolve_account("ING").unwrap();
    assert_eq!(s.book().balances().shown(&tree, ing), Money(100_000), "the starting balance is booked");
    assert_eq!(stored(&mut s), *s.book(), "every edit is saved");

    let r =
        s.execute("txn.create", json!({"payee": "Market", "from": "Wallet", "to": "Groceries", "amount": "12,50", "date": "2026-10-01"})).unwrap();
    let id = TxnId(r["id"].as_u64().unwrap());
    assert_eq!(s.selection, Some(id));
    let got = s.execute("txn.get", json!({})).unwrap();
    assert_eq!((got["from"].as_str(), got["to"].as_str(), got["amount"].as_i64()), (Some("Wallet"), Some("Groceries"), Some(-1250)));

    s.execute("txn.update", json!({"note": "Cheese"})).unwrap();
    s.execute("txn.delete", json!({})).unwrap();
    assert!(s.book().txn(id).is_none());
    assert!(s.take_notices().iter().any(|n| n.undo));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.book().txn(id).unwrap().note, "Cheese");
    assert_eq!(stored(&mut s), *s.book(), "undo is saved too");
    s.execute("edit.redo", json!({})).unwrap();
    assert!(s.book().txn(id).is_none());
    assert_eq!(s.undo_label(), Some("Delete Transaction"));
}

#[test]
fn failed_commands_change_nothing() {
    let mut s = with_accounts();
    let before = s.book().clone();
    assert!(s.execute("txn.create", json!({"payee": "x", "from": "Wallet", "to": "Wallet", "amount": 1})).is_err());
    assert!(s.execute("txn.create", json!({"payee": "x", "from": "Wallet", "to": "Groceries", "amount": "1e5"})).is_err());
    assert!(s.execute("account.create", json!({"kind": "pot", "name": "x", "parent": "Groceries"})).is_err());
    assert!(s.execute("account.delete", json!({"id": "Not sorted yet"})).is_err());
    assert_eq!(*s.book(), before);
}

#[test]
fn headless_calls_never_open_dialogs() {
    let mut s = session();
    for (id, p) in [
        ("txn.create", json!({"payee": "x"})),
        ("account.create", json!({"kind": "pot"})),
        ("file.open", json!({})),
        ("file.new", json!({})),
        ("pot.move", json!({})),
    ] {
        assert!(s.execute_headless(id, p.clone()).is_err(), "{id}");
        s.execute(id, p).unwrap();
        assert!(matches!(s.take_ui_requests().first(), Some(UiRequest::Dialog(..))), "{id} asks the UI");
    }
}

#[test]
fn review_answers_learn_rules() {
    let mut s = Session::sample();
    s.now = fixed_now;
    let list = s.execute("review.list", json!({})).unwrap();
    let total = list["total"].as_u64().unwrap();
    assert!(total > 0);
    let unknown = s.execute("review.list", json!({"tab": "unknown"})).unwrap();
    let first = unknown["items"][0]["id"].as_u64().unwrap();
    s.execute("txn.answer", json!({"id": first, "to": "Pet Care", "always": true})).unwrap();
    assert!(s.take_notices().iter().any(|n| n.text.contains("New payee rule")));
    assert_eq!(s.book().txn(TxnId(first)).unwrap().status, TxnStatus::Reviewed);
    assert_eq!(s.execute("review.list", json!({})).unwrap()["total"].as_u64(), Some(total - 1));
    let rules = s.execute("rules.list", json!({})).unwrap();
    assert!(rules["rules"].as_array().unwrap().iter().any(|r| r["to"] == "Pet Care"));
}

#[test]
fn queries_and_views() {
    let mut s = Session::sample();
    s.now = fixed_now;
    let info = s.execute("book.info", json!({})).unwrap();
    assert_eq!(info["name"], "Demo household");
    assert!(info["toReview"].as_u64().unwrap() > 0);
    let accounts = s.execute("accounts.list", json!({"group": "have"})).unwrap();
    assert!(accounts["accounts"].as_array().unwrap().iter().all(|a| a["group"] == "have"));
    let ah = s.execute("txns.list", json!({"text": "albert heijn", "limit": 3})).unwrap();
    assert!(ah["total"].as_u64().unwrap() > 10);
    assert_eq!(ah["transactions"].as_array().unwrap().len(), 3);
    let ing = s.execute("txns.list", json!({"account": "ING Betaalrekening", "limit": 1})).unwrap();
    assert!(ing["transactions"][0]["balance"].is_i64(), "one account has a running balance");
    s.execute("view.filter", json!({"account": "Groceries"})).unwrap();
    assert_eq!(s.view.screen, Screen::Transactions);
    assert!(s.journal().iter().all(|(id, _)| !id.starts_with("txns.")), "queries are not journaled");
    s.execute("view.report", json!({"report": "netWorth"})).unwrap();
    assert_eq!(s.view.screen, Screen::Reports);
}

#[test]
fn pots_balances_and_values() {
    let mut s = with_accounts();
    s.execute("account.setBalance", json!({"id": "Spaar", "amount": 3000, "date": "2026-01-01"})).unwrap();
    s.execute("pot.move", json!({"from": "Spaar", "to": "Spaar / Holiday", "amount": 500, "date": "2026-02-01"})).unwrap();
    let b = s.book();
    let tree = b.tree();
    let holiday = b.resolve_account("Spaar / Holiday").unwrap();
    assert_eq!(b.balances().shown(&tree, holiday), Money(50_000));
    assert_eq!(b.balances().shown(&tree, b.resolve_account("Spaar").unwrap()), Money(300_000));
    s.execute("account.create", json!({"kind": "property", "name": "House", "balance": 400000, "balanceDate": "2026-01-01"})).unwrap();
    s.execute("account.setValue", json!({"id": "House", "amount": 410000, "date": "2026-06-01"})).unwrap();
    let b = s.book();
    assert_eq!(b.balances().shown(&b.tree(), b.resolve_account("House").unwrap()), Money(41_000_000));
    s.execute("category.add", json!({"name": "pet care"})).unwrap();
    assert_eq!(s.book().account(s.book().resolve_account("Pet Care").unwrap()).map(|a| a.group), Some(Group::Goes));
    assert_eq!(s.book().validate(), Ok(()));
}

#[test]
fn file_book_reloads_external_changes_and_survives_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home.pecunia");
    let path_s = path.to_str().unwrap();
    let mut app = session();
    app.execute("file.new", json!({"path": path_s, "name": "Home"})).unwrap();
    app.execute("account.create", json!({"kind": "cash", "name": "Wallet"})).unwrap();

    // An agent edits the same file through its own session (CLI / MCP).
    let mut agent = Session::open(path_s).unwrap();
    agent.now = fixed_now;
    agent.execute("txn.create", json!({"payee": "Bakery", "from": "Wallet", "to": "Groceries", "amount": 3})).unwrap();

    assert!(app.poll_external());
    assert_eq!(app.book().txn_count(), 1);
    assert!(!app.can_undo(), "undo history doesn't survive an external reload");
    app.execute("txn.update", json!({"id": 1, "note": "Bread"})).unwrap();
    let on_disk = Session::open(path_s).unwrap();
    assert_eq!(on_disk.book().txn(TxnId(1)).unwrap().note, "Bread");
    assert!(app.execute("file.new", json!({"path": path_s})).is_err(), "never overwrites a file");
}

#[test]
fn sample_is_valid() {
    let s = Session::sample();
    assert_eq!(s.book().validate(), Ok(()));
    assert!(s.book().accounts().any(|a| a.kind == AccountKind::Pot && a.goal.is_some()));
}

/// Every command, called with junk, returns an error or succeeds; it never panics, and the book stays valid.
#[test]
fn hostile_params_never_panic() {
    let junk = [
        json!(null),
        json!({}),
        json!([]),
        json!("text"),
        json!(42),
        json!({"id": -1}),
        json!({"id": 1.5}),
        json!({"id": "99999999999999999999999999"}),
        json!({"id": {"nested": [1, 2, 3]}}),
        json!({"id": 1, "payee": 5, "from": [], "to": {}, "amount": true, "date": 7}),
        json!({"id": 1, "payee": "x".repeat(100_000)}),
        json!({"id": 1, "amount": "999999999999999999999", "from": "ING Betaalrekening", "to": "Groceries"}),
        json!({"id": 1, "amount": -5, "from": "Wallet", "to": "Groceries", "date": "2026-02-30"}),
        json!({"id": 1, "to": "Not sorted yet", "always": "yes"}),
        json!({"id": 1, "to": "ING Betaalrekening"}),
        json!({"id": "ING Betaalrekening", "name": "", "closed": true, "parent": "Groceries"}),
        json!({"id": "Groceries", "kind": "pot", "parent": "ICS Creditcard", "goal": "1e9"}),
        json!({"kind": "category_group", "name": "x", "group": "have"}),
        json!({"parts": [{"account": "Groceries", "amount": 1}], "keep": "maybe"}),
        json!({"parts": "nope", "pot": "Holiday", "ids": [1, "x", null]}),
        json!({"screen": "nowhere", "report": 3, "tab": [], "from": "2027-01-01", "to": "2020-01-01"}),
        json!({"payee": "", "iban": "", "to": "Groceries"}),
        json!({"text": "\u{0}\u{FFFF}", "account": "nope", "status": "maybe", "limit": -1}),
        json!({"path": 5, "name": [], "sample": "yes"}),
        json!({"id": 1, "amount": 0, "date": "today"}),
        json!({"name": "x".repeat(5000), "description": "y".repeat(50_000)}),
    ];
    for spec in command_specs() {
        // File commands with a path would touch the disk; they get junk without paths.
        for params in &junk {
            if spec.id.starts_with("file.") && params.get("path").is_some_and(|p| p.is_string()) {
                continue;
            }
            let mut s = Session::sample();
            s.now = fixed_now;
            s.selection = Some(TxnId(1));
            let _ = s.execute(spec.id, params.clone());
            assert_eq!(s.book().validate(), Ok(()), "{} with {params}", spec.id);
        }
    }
}

#[test]
fn command_ids_are_unique_and_documented() {
    let specs = command_specs();
    for (i, a) in specs.iter().enumerate() {
        assert!(!a.label.is_empty() && a.id.contains('.'), "{}", a.id);
        assert!(specs.iter().skip(i + 1).all(|b| b.id != a.id), "duplicate {}", a.id);
    }
}

#[test]
fn reports_answer_in_cents() {
    let mut s = Session::sample();
    s.now = fixed_now;
    let o = s.execute("report.overview", json!({})).unwrap();
    assert!(o["netWorth"]["net_worth"].is_i64(), "{}", o["netWorth"]);
    assert!(o["monthEnds"].as_array().is_some_and(|m| m.len() == 12));
    let ie = s.execute("report.incomeExpenses", json!({"from": "2026-09-01", "to": "2026-09-30"})).unwrap();
    assert!(ie["total_income"].as_i64().is_some_and(|n| (345_000..346_000).contains(&n)), "salary plus a little interest");
    assert!(s.execute("report.incomeExpenses", json!({"from": "2026-09-30", "to": "2026-09-01"})).is_err());
    let fixed = s.execute("report.fixedCosts", json!({})).unwrap();
    assert!(fixed["items"].as_array().is_some_and(|i| i.len() >= 5), "{fixed}");
    let check = s.execute("report.bankCheck", json!({})).unwrap();
    assert!(check["accounts"].as_array().is_some_and(|a| !a.is_empty()));
}

fn bank_file(format: pecunia_formats::samples::BankFileFormat) -> Vec<u8> {
    use pecunia_formats::samples::{BankFileAccount, BankFileTx, write};
    let account = BankFileAccount { key: "ing".into(), iban: "NL12INGB0001234567".into() };
    let tx = |seq: u32, day: u32, cents: i64, name: &str, desc: &str| BankFileTx {
        seq,
        date: pecunia_model::NaiveDate::from_ymd_opt(2026, 9, day).unwrap(),
        amount: Money(cents),
        name: name.into(),
        iban: None,
        description: desc.into(),
        reference: format!("REF{seq}"),
    };
    let txs = [
        tx(1, 2, -5210, "Albert Heijn 1427", "Boodschappen"),
        tx(2, 25, 345_000, "Werkgever BV", "Salaris"),
        tx(3, 28, -1399, "Netflix", "Abonnement"),
    ];
    write(format, &account, &txs, Money(100_000))
}

#[test]
fn imports_bank_files_once() {
    use base64::Engine as _;
    let mut s = with_accounts();
    let bytes = bank_file(pecunia_formats::samples::BankFileFormat::IngCsv);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let preview = s.execute("import.preview", json!({"contentBase64": b64, "name": "ing.csv"})).unwrap();
    assert_eq!(preview["recognised"], true);
    assert_eq!(preview["account"], "ING", "found by the IBAN in the file");
    assert_eq!(preview["import"]["new"], 3);
    assert_eq!(s.book().txn_count(), 2, "a preview changes nothing (two starting balances)");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ing.csv");
    std::fs::write(&path, &bytes).unwrap();
    let r = s.execute("import.file", json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!((r["new"].as_u64(), r["toReview"].as_u64()), (Some(3), Some(3)));
    assert_eq!(s.view.filter.import.map(|i| i.0), r["import"].as_u64(), "the register shows what came in");
    let again = s.execute("import.file", json!({"contentBase64": b64, "name": "ing.csv"})).unwrap();
    assert_eq!((again["new"].as_u64(), again["known"].as_u64()), (Some(0), Some(3)));
    s.execute("import.undo", json!({"id": r["import"]})).unwrap();
    assert_eq!(s.book().txn_count(), 2);

    let mt940 = bank_file(pecunia_formats::samples::BankFileFormat::Mt940);
    let r = s.execute("import.file", json!({"contentBase64": base64::engine::general_purpose::STANDARD.encode(&mt940), "name": "x.sta"})).unwrap();
    assert_eq!(r["format"], "MT940");
    assert!(!s.book().meta.balances.is_empty(), "MT940 states a closing balance");

    let junk = s
        .execute("import.preview", json!({"contentBase64": base64::engine::general_purpose::STANDARD.encode(b"a;b;c\n1;2;3\n"), "name": "x.csv"}))
        .unwrap();
    assert_eq!(junk["recognised"], false);
    assert_eq!(junk["csv"]["headers"], json!(["a", "b", "c"]));
    assert!(s.execute("import.file", json!({"contentBase64": "!!!"})).is_err());
    assert_eq!(s.book().validate(), Ok(()));
}

#[test]
fn password_protected_books() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret.pecunia");
    let path_s = path.to_str().unwrap();
    let mut s = session();
    assert!(s.execute("file.new", json!({"path": path_s, "password": "short"})).is_err(), "at least 8 characters");
    s.execute("file.new", json!({"path": path_s, "name": "Secret", "password": "correct horse"})).unwrap();
    assert!(s.encrypted());
    s.execute("account.create", json!({"kind": "cash", "name": "Wallet"})).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("Wallet"), "the file holds no readable data");

    // Opening without the password asks for it; headless callers get an error that says what to pass.
    let mut other = session();
    other.execute("file.open", json!({"path": path_s})).unwrap();
    assert!(matches!(other.take_ui_requests().first(), Some(UiRequest::Dialog(n, _)) if n == "unlock"));
    assert!(other.execute_headless("file.open", json!({"path": path_s})).unwrap_err().to_string().contains("password"));
    assert!(other.execute("file.open", json!({"path": path_s, "password": "wrong password"})).is_err());
    other.execute("file.open", json!({"path": path_s, "password": "correct horse"})).unwrap();
    assert!(other.book().resolve_account("Wallet").is_some());
    assert!(Session::open_with(path_s, Some("correct horse")).is_ok());

    // Edits from the other session are picked up with the kept key, no password needed.
    other.execute("account.create", json!({"kind": "savings", "name": "Spaar"})).unwrap();
    assert!(s.poll_external());
    assert!(s.book().resolve_account("Spaar").is_some());

    s.execute("file.removePassword", json!({})).unwrap();
    assert!(!s.encrypted());
    assert!(std::fs::read_to_string(&path).unwrap().contains("Wallet"));
    assert!(s.execute("file.removePassword", json!({})).is_err(), "nothing to remove");
    s.execute("file.setPassword", json!({"password": "battery staple"})).unwrap();
    assert!(Session::open_with(path_s, Some("battery staple")).is_ok());
    assert!(Session::open_with(path_s, Some("correct horse")).is_err());
    assert!(Session::scratch().execute("file.setPassword", json!({"password": "12345678"})).is_err(), "a book in memory has no file to protect");
    let journal = format!("{:?}", s.journal());
    assert!(!journal.contains("correct horse") && !journal.contains("battery staple"), "no passwords in the journal: {journal}");
}

#[test]
fn the_sample_is_a_demo() {
    let s = Session::sample();
    assert!(s.is_demo());
    assert_eq!(s.book().name(), "Demo household");
    assert!(!session().is_demo());
}
