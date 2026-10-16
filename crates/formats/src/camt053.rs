//! CAMT.053 (ISO 20022 bank-to-customer statement, XML). From budget.rs `files/camt053.rs`.
//!
//! Every booked entry becomes a transaction; the bank's own reference (AcctSvcrRef, else EndToEndId) is the external
//! id, so re-imports dedup exactly. roxmltree refuses a DTD, so there is no XXE and no entity expansion.

use chrono::NaiveDate;
use pecunia_model::{Money, ParsedStatement, RawTransaction, StatementUnreadable};
use roxmltree::{Document, Node};

use crate::{find_iban, push_unique};

pub(crate) fn parse(content: &[u8]) -> Result<ParsedStatement, StatementUnreadable> {
    let invalid = |e: &dyn std::fmt::Display| StatementUnreadable::new(format!("not valid XML: {e}"));
    let text = std::str::from_utf8(content).map_err(|e| invalid(&e))?;
    if nesting_depth(text) > MAX_DEPTH {
        return Err(StatementUnreadable::new(format!("elements are nested more than {MAX_DEPTH} deep: this is not a CAMT.053 statement")));
    }
    let doc = Document::parse(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|e| invalid(&e))?;
    let stmts: Vec<Node> = doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Stmt").collect();
    if stmts.is_empty() {
        return Err(StatementUnreadable::new("no <Stmt> found: this is not a CAMT.053 statement"));
    }
    let mut statement = ParsedStatement::default();
    for stmt in stmts {
        let own = path(stmt, &["Acct", "Id", "IBAN"]).map(|n| text_of(n).trim().to_string());
        if let Some(own) = &own {
            push_unique(&mut statement.account_ibans, own.clone());
        }
        for (i, ntry) in children(stmt, "Ntry").enumerate() {
            let entry = |reason: &str| StatementUnreadable::new(format!("entry {} {reason}", i + 1));
            let status = child(ntry, "Sts").map(|s| child(s, "Cd").map_or_else(|| text_of(s), text_of));
            if status.is_some_and(|s| s.trim() != "BOOK") {
                continue;
            }
            let amount_text = child(ntry, "Amt").map(text_of).ok_or_else(|| entry("has no amount"))?;
            let debit = child(ntry, "CdtDbtInd").is_some_and(|n| text_of(n).trim() == "DBIT");
            let amount = Money::parse(&amount_text).map_err(|e| entry(&format!("has an unreadable amount: {e}")))?;
            let amount = if debit { -amount.abs() } else { amount.abs() };
            // The date as written (Dt, or the first 10 characters of DtTm); the raw line keeps it as is.
            let date = child(ntry, "BookgDt")
                .or_else(|| child(ntry, "ValDt"))
                .and_then(|d| child(d, "Dt").map(text_of).or_else(|| child(d, "DtTm").map(|t| text_of(t).chars().take(10).collect())))
                .ok_or_else(|| entry("has no booking date"))?;
            let day = NaiveDate::parse_from_str(&date.chars().take(10).collect::<String>(), "%Y-%m-%d")
                .map_err(|_| entry(&format!("has an unreadable date '{}'", date.chars().take(40).collect::<String>())))?;
            let tx = path(ntry, &["NtryDtls", "TxDtls"]);
            let parties = tx.and_then(|t| child(t, "RltdPties"));
            let party = parties.and_then(|p| child(p, if debit { "Cdtr" } else { "Dbtr" }));
            let name = party.and_then(|p| child(p, "Pty").or(Some(p))).and_then(|p| child(p, "Nm")).map(|n| text_of(n).trim().to_string());
            let iban =
                parties.and_then(|p| path(p, &[if debit { "CdtrAcct" } else { "DbtrAcct" }, "Id", "IBAN"])).map(|n| text_of(n).trim().to_string());
            let remittance = match tx.and_then(|t| child(t, "RmtInf")) {
                Some(rmt) => children(rmt, "Ustrd").map(|u| text_of(u).trim().to_string()).collect::<Vec<_>>().join(" "),
                None => child(ntry, "AddtlNtryInf").map(|n| text_of(n).trim().to_string()).unwrap_or_default(),
            };
            let reference = child(ntry, "AcctSvcrRef")
                .map(|n| text_of(n).trim().to_string())
                .or_else(|| tx.and_then(|t| path(t, &["Refs", "EndToEndId"])).map(|n| text_of(n).trim().to_string()).filter(|r| r != "NOTPROVIDED"));
            // budget.rs (and Kotlin before it) wrote a missing value as "null"; kept so import hashes stay the same.
            let or_null = |s: &Option<String>| s.clone().unwrap_or_else(|| "null".into());
            statement.rows.push(RawTransaction {
                date: day,
                amount,
                counterparty_iban: iban.or_else(|| find_iban(&remittance)),
                raw_line: format!("camt|{}|{date}|{}|{}|{remittance}|{}", or_null(&own), amount.cents(), or_null(&name), or_null(&reference)),
                counterparty_name: name,
                external_id: reference,
                own_iban: own.clone(),
                who: None,
                description: remittance,
            });
        }
    }
    Ok(statement)
}

/// A CAMT.053 statement nests about a dozen deep. roxmltree recurses once per level, so a hostile file nested thousands
/// deep would overflow the stack (1 MB in the browser); it is refused before it is parsed.
pub(crate) const MAX_DEPTH: usize = 128;

/// The deepest element nesting in `text`, from a quick scan of the markup. It skips comments, CDATA, processing
/// instructions and quoted attribute values, so it never counts less than roxmltree will nest.
pub(crate) fn nesting_depth(text: &str) -> usize {
    let bytes = text.as_bytes();
    let find = |from: usize, pattern: &[u8]| -> usize {
        bytes.get(from..).and_then(|rest| rest.windows(pattern.len()).position(|w| w == pattern)).map_or(bytes.len(), |p| from + p + pattern.len())
    };
    let (mut depth, mut deepest, mut i) = (0usize, 0usize, 0usize);
    while i < bytes.len() {
        let rest = bytes.get(i..).unwrap_or_default();
        if !rest.starts_with(b"<") {
            i += 1;
        } else if rest.starts_with(b"<!--") {
            i = find(i + 4, b"-->");
        } else if rest.starts_with(b"<![CDATA[") {
            i = find(i + 9, b"]]>");
        } else if rest.starts_with(b"<?") {
            i = find(i + 2, b"?>");
        } else if rest.starts_with(b"<!") {
            i = find(i + 2, b">");
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            i = find(i + 2, b">");
        } else {
            // An opening tag runs to the first `>` outside quotes; `/>` closes it at once.
            let (mut quote, mut previous) = (None, 0u8);
            i += 1;
            while let Some(&c) = bytes.get(i) {
                i += 1;
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None if c == b'"' || c == b'\'' => quote = Some(c),
                    None if c == b'>' => break,
                    None => {}
                }
                previous = c;
            }
            if previous != b'/' {
                depth += 1;
                deepest = deepest.max(depth);
            }
        }
    }
    deepest
}

/// Child elements by local name (any namespace).
fn children<'a, 'i>(node: Node<'a, 'i>, local: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| n.is_element() && n.tag_name().name() == local)
}

fn child<'a, 'i>(node: Node<'a, 'i>, local: &'static str) -> Option<Node<'a, 'i>> {
    children(node, local).next()
}

fn path<'a, 'i>(node: Node<'a, 'i>, locals: &[&'static str]) -> Option<Node<'a, 'i>> {
    locals.iter().try_fold(node, |n, local| child(n, local))
}

/// All text below `node`, like the DOM's `textContent`.
fn text_of(node: Node) -> String {
    node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect()
}
