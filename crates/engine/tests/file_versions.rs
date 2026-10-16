//! Every file version this app has ever written must keep opening (ADR 0006). The fixtures under `fixtures/v<N>/`
//! are frozen: never edit them; a new file version adds a new folder (`cargo test -p pecunia-engine --test
//! file_versions -- --ignored write_fixtures` writes the current version's, once).

use std::path::PathBuf;

use pecunia_engine::Session;
use pecunia_engine::store::VERSION;
use serde_json::json;

/// The password of the encrypted fixtures. Test data only.
const PASSWORD: &str = "pecunia-fixture";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture: (version, file, password).
fn all() -> Vec<(u32, PathBuf, Option<&'static str>)> {
    let mut out = Vec::new();
    for v in 1..=VERSION {
        let dir = fixtures().join(format!("v{v}"));
        out.push((v, dir.join("demo.pecunia"), None));
        out.push((v, dir.join("demo-encrypted.pecunia"), Some(PASSWORD)));
    }
    out
}

#[test]
fn every_old_file_opens_without_repairs() {
    for (v, path, password) in all() {
        let p = path.to_str().unwrap_or_default();
        assert!(path.exists(), "no fixture {p}: write one for file version {v}");
        let mut s = Session::open_with(p, password).unwrap_or_else(|e| panic!("{p}: {e}"));
        let repairs: Vec<String> = s.take_notices().into_iter().map(|n| n.text).filter(|t| t.starts_with("Repaired")).collect();
        assert!(repairs.is_empty(), "{p}: {repairs:?}");
        let b = s.book();
        assert_eq!(b.validate(), Ok(()), "{p}");
        assert!(b.meta.demo && b.txn_count() > 250 && b.rules().count() > 5, "{p} lost data");
        assert_eq!(s.encrypted(), password.is_some(), "{p}");
        // And the queries still work on it.
        assert!(s.execute("report.overview", json!({})).is_ok(), "{p}");
    }
}

#[test]
fn encrypted_fixtures_need_their_password() {
    for (_, path, password) in all() {
        if password.is_some() {
            assert!(Session::open(path.to_str().unwrap_or_default()).is_err());
        }
    }
}

/// Writes the fixtures of the current file version, when they don't exist yet. Run once per new version.
#[test]
#[ignore]
fn write_fixtures() {
    let dir = fixtures().join(format!("v{VERSION}"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, password) in [("demo.pecunia", None), ("demo-encrypted.pecunia", Some(PASSWORD))] {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        let mut s = Session::scratch();
        s.now = || 1_791_936_000_000; // 2026-10-14, so the fixture is the same every time
        let mut p = json!({ "path": path.to_str().unwrap(), "name": "Demo household", "sample": true });
        if let Some(pw) = password {
            p["password"] = json!(pw);
        }
        s.execute("file.new", p).unwrap();
    }
}
