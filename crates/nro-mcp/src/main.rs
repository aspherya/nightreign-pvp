//! nro-mcp: the Claude-facing MCP server (stdio, JSON-RPC 2.0, one message per line). It holds no logic: every tool is one call
//! into `nro::ops`, the same functions the `nro` CLI runs. Screenshots are returned as images so Claude can look at them.
//!
//! Register it in Claude Code (`.mcp.json` in this repository does it):
//! `{"mcpServers": {"nightreign": {"command": "<repo>\\target\\release\\nro-mcp.exe"}}}`

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use nro::ops;

const PROTOCOL: &str = "2024-11-05";

fn tools() -> Value {
    let t = |name: &str, desc: &str, props: Value, req: &[&str]| json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props, "required": req}});
    let mod_dist = json!({"type": "string", "description": "Folder with the mod build to test (nrpvp.dll, nrpvp.toml, gamedata). Default: <nightreign-pvp>\\dist"});
    json!([
        t("env", "Pins, relevant processes (Steam, game, me3, anti-cheat) and the current session.", json!({}), &[]),
        t("preflight", "Stage a run folder and run every offline-safety check without launching. Returns each check, what it saw and how to fix a failure.", json!({"mod_dist": mod_dist}), &[]),
        t("launch", "Preflight, then launch the game offline through me3 with the staged mod (refuses when any check fails; there is no override). Waits for the mod console. world=true also presses through the title to the Roundtable. `set` writes top-level keys of the staged nrpvp.toml (e.g. {\"mode\": \"offline\", \"offline_bots\": 0}).",
          json!({"mod_dist": mod_dist, "world": {"type": "boolean"}, "set": {"type": "object"}}), &[]),
        t("status", "Session phase, game pid and the mod's `status` reply.", json!({}), &[]),
        t("console", "Send one line to the mod's developer console (`help` lists the commands) and return the reply.", json!({"line": {"type": "string"}}), &["line"]),
        t("navigate", "Scripted menu paths: title_to_world (title → Roundtable), expedition (Roundtable → expedition in offline mode, off the hawk).", json!({"to": {"type": "string", "enum": ["title_to_world", "expedition"]}}), &["to"]),
        t("keys", "Keyboard/mouse input to the game window (focuses it): \"W 2000ms; F; wait 1s; click 0.5 0.82; ALT\". Keys: letters, digits, ENTER ESC SPACE TAB UP DOWN LEFT RIGHT SHIFT CTRL ALT F1-F4. In game: F = interact / menu OK, Q = back, left click = attack. Needs `nro desk` running in a normal terminal.", json!({"seq": {"type": "string"}}), &["seq"]),
        t("screenshot", "Screenshot of the game window (returned as an image and saved in the run folder). Needs `nro desk`.", json!({"name": {"type": "string"}}), &[]),
        t("puppets", "Make n AFK puppet players (1..5) in the running session: real PlayerIns in the game's player list, recognised as players by the game and the mod (rivals p2..p6), can be hit, downed and bleed out. Offline mode, a world loaded (best in an expedition after landing).", json!({"n": {"type": "integer", "minimum": 1, "maximum": 5}}), &["n"]),
        t("puppet_ring", "Put every puppet on a circle around the player.", json!({"radius": {"type": "number"}}), &[]),
        t("puppet_front", "Put every puppet side by side in front of the player (facing measured with a short step forward), for hit tests.", json!({"dist": {"type": "number"}}), &[]),
        t("log", "Tail of the run's nrpvp.log, optionally only lines containing `filter`.", json!({"n": {"type": "integer"}, "filter": {"type": "string"}}), &[]),
        t("stop", "Kill the session's game and me3 (the game's own quit crashes on every run; this is the normal end).", json!({}), &[]),
        t("scenarios", "List scenarios/*.toml.", json!({}), &[]),
        t("run_scenario", "Run a scenario file end to end (stage, preflight, launch, steps, report.json, teardown) and return the report with PASS/FAIL/ABORTED/REFUSED.", json!({"file": {"type": "string"}, "mod_dist": mod_dist}), &["file"]),
    ])
}

fn s(a: &Map<String, Value>, k: &str) -> Option<String> {
    a.get(k).and_then(|v| v.as_str()).map(String::from)
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn call(name: &str, a: &Map<String, Value>) -> Result<Value, String> {
    let text = |v: Value| json!([{"type": "text", "text": if let Value::String(s) = &v { s.clone() } else { serde_json::to_string_pretty(&v).unwrap() }}]);
    let r = match name {
        "env" => ops::env(),
        "preflight" => ops::preflight(s(a, "mod_dist"), None),
        "launch" => {
            let mut set = BTreeMap::new();
            if let Some(Value::Object(m)) = a.get("set") {
                for (k, v) in m {
                    let tv =
                        toml::Value::try_from(v.clone()).map_err(|e| format!("set.{k}: {e}"))?;
                    set.insert(k.clone(), tv);
                }
            }
            ops::launch(
                s(a, "mod_dist"),
                set,
                a.get("world").and_then(|v| v.as_bool()).unwrap_or(false),
                None,
            )
        }
        "status" => ops::status(),
        "console" => ops::console(&s(a, "line").ok_or("line is required")?),
        "navigate" => ops::navigate(&s(a, "to").ok_or("to is required")?),
        "keys" => ops::keys(&s(a, "seq").ok_or("seq is required")?),
        "screenshot" => {
            let v = ops::screenshot(&s(a, "name").unwrap_or_else(|| "shot".into()))?;
            let line = v.as_str().unwrap_or_default().to_string();
            let path = line.split(" (").next().unwrap_or_default().to_string();
            let png = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            return Ok(
                json!([{"type": "image", "data": b64(&png), "mimeType": "image/png"}, {"type": "text", "text": line}]),
            );
        }
        "puppets" => ops::puppets(a.get("n").and_then(|v| v.as_u64()).unwrap_or(2) as usize),
        "puppet_ring" => {
            ops::puppet_ring(a.get("radius").and_then(|v| v.as_f64()).unwrap_or(1.3) as f32)
        }
        "puppet_front" => {
            ops::puppet_front(a.get("dist").and_then(|v| v.as_f64()).unwrap_or(1.2) as f32)
        }
        "log" => ops::log_tail(
            a.get("n").and_then(|v| v.as_u64()).unwrap_or(40) as usize,
            s(a, "filter").as_deref(),
        ),
        "stop" => ops::stop(),
        "scenarios" => ops::scenarios(None),
        "run_scenario" => {
            ops::run_scenario(&s(a, "file").ok_or("file is required")?, s(a, "mod_dist"))
        }
        other => Err(format!("unknown tool {other}")),
    }?;
    Ok(text(r))
}

fn main() {
    nro::sys::dpi_aware();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result = match method {
            "initialize" => Some(Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "nightreign-orchestrator", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Drives an offline nightreign-pvp developer game: launch (safety-checked), console, input, screenshots, AFK puppet players, scenarios. Start `nro desk` in a normal terminal for keys and screenshots.",
            }))),
            "tools/list" => Some(Ok(json!({"tools": tools()}))),
            "tools/call" => {
                let p = msg.get("params").cloned().unwrap_or(Value::Null);
                let name = p
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = p
                    .get("arguments")
                    .and_then(|v| v.as_object())
                    .cloned()
                    .unwrap_or_default();
                Some(Ok(match call(&name, &args) {
                    Ok(content) => json!({"content": content}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                }))
            }
            "ping" => Some(Ok(json!({}))),
            _ if id.is_none() => None, // notifications (initialized, cancelled)
            _ => Some(Err(
                json!({"code": -32601, "message": format!("method not found: {method}")}),
            )),
        };
        if let (Some(id), Some(r)) = (id, result) {
            let reply = match r {
                Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
            };
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn every_tool_has_a_schema() {
        for t in tools().as_array().unwrap() {
            assert!(t["name"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }
}
