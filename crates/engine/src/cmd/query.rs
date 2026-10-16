use pecunia_model::register::{self, Filter};
use pecunia_model::{Account, AccountKind, AccountTree, Balances, Book, Money, ReviewKind, Txn, TxnStatus, catalogue};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(Query "book.info", "Book Info", [], None, "", always, info),
        cmd!(Query "accounts.list", "List Accounts", [], None,
            "group? (have|owe|goes|comes|equity), closed? (bool: include closed accounts), date? (balances at the end of this day; default today)",
            always, accounts),
        cmd!(Query "txns.list", "List Transactions", [], None,
            "Any of: text, account, review (bool), status, from, to (dates), import (number), limit (default 200), view (bool: use the app's current filter instead). Newest first.",
            always, txns),
        cmd!(Query "txn.get", "Get Transaction", [], None, "id? (default the selection)", always, get),
        cmd!(Query "review.list", "List Review", [], None, "tab? (duplicate|transfer|suggested|unknown), limit? (default 200)", always, review_list),
        cmd!(Query "rules.list", "List Payee Rules", [], None, "", always, rules),
        cmd!(Query "imports.list", "List Imports", [], None, "", always, imports),
        cmd!(Query "categories.catalogue", "Usual Categories", [], None, "The usual household categories `category.add` knows, and whether the book has them.", always, catalogue_list),
    ]
}

/// Money in answers: integer cents (`-6418` is −€ 64,18), the same as in the file and in reports.
pub fn money_json(m: Money) -> Json {
    json!(m.cents())
}

/// An account as JSON: its fields, where it sits and its balance as people see it.
pub fn account_json(book: &Book, tree: &AccountTree, balances: &Balances, a: &Account) -> Json {
    let mut v = json!({
        "id": a.id.0,
        "name": a.name,
        "label": book.account_label(a.id),
        "group": a.group,
        "kind": a.kind,
        "balance": money_json(balances.shown(tree, a.id)),
    });
    if let Some(p) = a.parent {
        v["parent"] = json!(tree.name(p));
    }
    if let Some(i) = &a.iban {
        v["iban"] = json!(i);
    }
    if !a.description.is_empty() {
        v["description"] = json!(a.description);
    }
    if let Some(g) = a.goal {
        v["goal"] = money_json(g);
    }
    if let Some(d) = a.goal_date {
        v["goalDate"] = json!(d);
    }
    if a.unassigned {
        v["unassigned"] = json!(true);
    }
    if a.closed {
        v["closed"] = json!(true);
    }
    v
}

/// A transaction as JSON for callers: when, who, from → to, the amount, its state and, with `full`, its postings.
pub fn txn_json(book: &Book, tree: &AccountTree, t: &Txn, full: bool) -> Json {
    let (from, to) = register::arrow(tree, t);
    let mut v = json!({
        "id": t.id.0,
        "date": t.date,
        "payee": t.payee,
        "from": from,
        "to": to,
        "amount": money_json(register::household_amount(tree, t)),
        "status": t.status,
    });
    if let Some(k) = book.review_kind(t) {
        v["review"] = json!(k);
    }
    if let Some(a) = t.account {
        v["account"] = json!(book.account_label(a));
    }
    if !t.note.is_empty() {
        v["note"] = json!(t.note);
    }
    if let Some(w) = &t.who {
        v["who"] = json!(w);
    }
    if full {
        v["kind"] = json!(t.kind);
        v["description"] = json!(t.description);
        v["payeeNorm"] = json!(t.payee_norm);
        if let Some(i) = &t.counterparty_iban {
            v["counterpartyIban"] = json!(i);
        }
        if let Some(b) = t.bank_amount {
            v["bankAmount"] = money_json(b);
        }
        v["postings"] = t
            .postings
            .iter()
            .map(|p| json!({ "account": book.account_label(p.account), "accountId": p.account.0, "amount": money_json(p.amount), "memo": p.memo }))
            .collect();
        v["guesses"] = t
            .guesses
            .iter()
            .map(|g| json!({ "account": book.account_label(g.account), "accountId": g.account.0, "probability": g.probability }))
            .collect();
        if !t.via.is_empty() {
            v["via"] = json!(t.via);
        }
        if t.transfer_likely {
            v["transferLikely"] = json!(true);
        }
        if let Some(d) = t.duplicate_of {
            v["duplicateOf"] = json!(d.0);
        }
        if let Some(i) = t.import {
            v["import"] = json!(i.0);
        }
        v["source"] = json!(t.source);
    }
    v
}

fn info(s: &mut Session, _: &Json) -> Result<Json> {
    let b = s.book();
    let tree = b.tree();
    let balances = b.balances_at(s.today());
    let physical: Vec<Json> = tree.all_physical().into_iter().filter(|a| !a.closed).map(|a| account_json(b, &tree, &balances, a)).collect();
    Ok(json!({
        "name": b.name(),
        "location": s.location(),
        "transactions": b.txn_count(),
        "toReview": b.review_count(),
        "accounts": physical,
        "categories": tree.categories().len(),
        "rules": b.rules().count(),
        "dates": b.date_range().map(|(a, z)| json!({ "first": a, "last": z })),
        "selection": s.selection.map(|id| id.0),
        "screen": s.view.screen,
    }))
}

fn accounts(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "accounts.list";
    let group = match opt_string(C, p, "group")? {
        Some(g) => Some(pecunia_model::Group::parse(&g).ok_or_else(|| bad(C, "`group` is have, owe, goes, comes or equity"))?),
        None => None,
    };
    let closed = bool_param(p, "closed").unwrap_or(false);
    let date = date_param(s, C, p, "date")?.unwrap_or_else(|| s.today());
    let b = s.book();
    let tree = b.tree();
    let balances = b.balances_at(date);
    let rows: Vec<Json> = tree
        .walk_depth()
        .into_iter()
        .filter(|(a, _)| group.is_none_or(|g| a.group == g) && (closed || !a.closed))
        .map(|(a, depth)| {
            let mut v = account_json(b, &tree, &balances, a);
            v["depth"] = json!(depth);
            v
        })
        .collect();
    Ok(json!({ "date": date, "accounts": rows }))
}

fn txns(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "txns.list";
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(200).min(100_000) as usize;
    let filter = if bool_param(p, "view").unwrap_or(false) {
        s.view.filter.clone()
    } else {
        Filter {
            text: opt_string(C, p, "text")?.unwrap_or_default().chars().take(200).collect(),
            account: account_param(s, C, p, "account")?,
            review: bool_param(p, "review").unwrap_or(false),
            status: match opt_string(C, p, "status")? {
                Some(t) => Some(TxnStatus::parse(&t).ok_or_else(|| bad(C, "`status` is auto, to_review or reviewed"))?),
                None => None,
            },
            from: date_param(s, C, p, "from")?,
            to: date_param(s, C, p, "to")?,
            import: p.get("import").and_then(Json::as_u64).map(pecunia_model::ImportId),
        }
    };
    let b = s.book();
    let tree = b.tree();
    let rows = register::rows(b, &tree, &filter);
    let total = rows.len();
    let out: Vec<Json> = rows
        .iter()
        .take(limit)
        .map(|r| {
            let mut v = txn_json(b, &tree, &r.txn, false);
            if filter.account.is_some() {
                v["amount"] = money_json(r.amount);
            }
            if let Some(bal) = r.balance {
                v["balance"] = money_json(bal);
            }
            v
        })
        .collect();
    Ok(json!({ "total": total, "transactions": out }))
}

fn get(s: &mut Session, p: &Json) -> Result<Json> {
    let id = target(s, "txn.get", p)?;
    let b = s.book();
    let t = b.txn(id).ok_or_else(|| bad("txn.get", "no such transaction"))?;
    Ok(txn_json(b, &b.tree(), t, true))
}

fn review_list(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "review.list";
    let tab = match opt_string(C, p, "tab")? {
        Some(t) => Some(ReviewKind::parse(&t).ok_or_else(|| bad(C, "`tab` is duplicate, transfer, suggested or unknown"))?),
        None => None,
    };
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(200).min(100_000) as usize;
    let b = s.book();
    let tree = b.tree();
    let items = b.review();
    let count = |k: ReviewKind| items.iter().filter(|i| i.kind == k).count();
    let counts: serde_json::Map<String, Json> = ReviewKind::ALL.iter().map(|k| (k.id().to_string(), json!(count(*k)))).collect();
    let rows: Vec<Json> = items
        .iter()
        .filter(|i| tab.is_none_or(|t| i.kind == t))
        .take(limit)
        .filter_map(|i| {
            let t = b.txn(i.id)?;
            let mut v = txn_json(b, &tree, t, true);
            v["review"] = json!(i.kind);
            v["history"] = i.history.iter().map(|(a, n)| json!({ "account": b.account_label(*a), "count": n })).collect();
            Some(v)
        })
        .collect();
    Ok(json!({ "total": items.len(), "counts": counts, "items": rows }))
}

fn rules(s: &mut Session, _: &Json) -> Result<Json> {
    let b = s.book();
    let rows: Vec<Json> = b
        .rules()
        .map(|r| {
            json!({
                "id": r.id.0,
                "payee": r.payee_prefix,
                "iban": r.iban,
                "to": b.account_label(r.account),
                "origin": r.origin,
                "mixed": r.mixed,
                "uses": r.uses,
            })
        })
        .collect();
    Ok(json!({ "rules": rows }))
}

fn imports(s: &mut Session, _: &Json) -> Result<Json> {
    let b = s.book();
    let rows: Vec<Json> = b
        .imports()
        .into_iter()
        .map(|(i, left)| {
            json!({
                "id": i.id.0,
                "account": b.account_label(i.account),
                "file": i.file,
                "format": i.format,
                "at": i.at,
                "rows": i.rows,
                "new": i.new,
                "known": i.known,
                "duplicates": i.duplicates,
                "transfers": i.transfers,
                "transactions": left,
            })
        })
        .collect();
    Ok(json!({ "imports": rows }))
}

fn catalogue_list(s: &mut Session, _: &Json) -> Result<Json> {
    let b = s.book();
    let rows: Vec<Json> = catalogue::ENTRIES
        .iter()
        .map(|e| {
            let have = b.accounts().any(|a| a.kind == AccountKind::Category && a.name.eq_ignore_ascii_case(e.name));
            json!({ "name": e.name, "group": e.group_name, "kind": e.group, "description": e.description, "inBook": have })
        })
        .collect();
    Ok(json!({ "categories": rows }))
}
