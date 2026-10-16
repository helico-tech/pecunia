#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! MCP server (L3): every engine command is a tool (`txn.create` → `txn_create`), served as
//! JSON-RPC 2.0 over stdio. Agents get exactly what the app's own UI and CLI get. Commands that
//! only steer the app's screen (`view.*`, `txn.select`) are left out.

use std::io::{self, BufRead, Write};

use pecunia_engine::{Session, command_specs, find_command};
use serde_json::{Value as Json, json};

const PROTOCOL: &str = "2025-06-18";

fn exposed(id: &str) -> bool {
    !id.starts_with("view.") && id != "txn.select"
}

pub fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn command_for_tool(name: &str) -> Option<&'static str> {
    command_specs().iter().map(|s| s.id).find(|id| exposed(id) && tool_name(id) == name)
}

pub fn tools() -> Vec<Json> {
    command_specs()
        .iter()
        .filter(|s| exposed(s.id))
        .map(|s| {
            let mut description = s.label.trim_end_matches('…').to_string();
            if !s.params.is_empty() {
                description.push_str(". Params: ");
                description.push_str(s.params);
            }
            json!({
                "name": tool_name(s.id),
                "description": description,
                "inputSchema": { "type": "object", "additionalProperties": true },
                "annotations": { "readOnlyHint": s.kind == pecunia_engine::cmd::Kind::Query },
            })
        })
        .collect()
}

/// Answers one JSON-RPC message. Notifications (no `id`) get no answer.
pub fn handle(session: &mut Session, msg: &Json) -> Option<Json> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Json::Null);
    let result = match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Json::as_str).unwrap_or(PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "pecunia", "version": env!("CARGO_PKG_VERSION") },
                "instructions": format!(
                    "Pecunia household book '{}' ({}). Accounts are referred to by number, name or path (`ING / Holiday`), transactions by number. Amounts you send are euros (12.50); amounts in answers are integer cents (-6418 is -64.18 euros). Start with book_info, review_list and txns_list; answer Review with txn_answer.",
                    session.book().name(),
                    session.location()
                ),
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Json::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match command_for_tool(name).and_then(find_command) {
                None => Err((-32602, format!("unknown tool {name}"))),
                Some(spec) => {
                    let r = session.execute_headless(spec.id, args);
                    Ok(match r {
                        Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v, "isError": false }),
                        Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
                    })
                }
            }
        }
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    })
}

/// Serves newline-delimited JSON-RPC until `input` ends.
pub fn serve(session: &mut Session, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Json>(&line) {
            Ok(msg) => handle(session, &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })),
        };
        if let Some(r) = reply {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, id: u64, method: &str, params: Json) -> Json {
        handle(s, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap()
    }

    #[test]
    fn lists_and_calls_tools() {
        let mut s = Session::scratch();
        let init = call(&mut s, 1, "initialize", json!({ "protocolVersion": "2025-03-26" }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let tools = call(&mut s, 2, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"txn_create") && names.contains(&"txns_list"));
        assert!(!names.iter().any(|n| n.starts_with("view_")));
        assert!(names.iter().all(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')));

        call(&mut s, 3, "tools/call", json!({ "name": "account_create", "arguments": { "kind": "cash", "name": "Wallet" } }));
        let r = call(
            &mut s,
            3,
            "tools/call",
            json!({ "name": "txn_create", "arguments": { "payee": "Market", "from": "Wallet", "to": "Groceries", "amount": "4,50" } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        assert_eq!(r["result"]["structuredContent"]["id"], 1);
        // Programmatic calls never open dialogs: a missing amount is an error.
        let r = call(&mut s, 4, "tools/call", json!({ "name": "txn_create", "arguments": { "payee": "Market" } }));
        assert_eq!(r["result"]["isError"], true);
        let r = call(&mut s, 5, "tools/call", json!({ "name": "nope", "arguments": {} }));
        assert!(r.get("error").is_some());
        assert!(handle(&mut s, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
    }

    #[test]
    fn serve_survives_garbage() {
        let mut s = Session::scratch();
        let input = b"not json\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n[1,2]\n";
        let mut out = Vec::new();
        serve(&mut s, &input[..], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("parse error"));
    }
}
