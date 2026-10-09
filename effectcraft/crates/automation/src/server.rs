//! MCP server: JSON-RPC 2.0, one message per line (the MCP stdio transport). Implements
//! `initialize`, `ping`, `tools/list` and `tools/call`; notifications are accepted and ignored.

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::autosave::AutoSave;
use crate::tools::{self, Reply};
use crate::{Backend, Error, base64};

/// Protocol revisions we speak, newest first. We answer with the client's if we know it.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "EffectCraft is an After Effects-class motion graphics compositor. Everything is an engine command: list_commands discovers ids and params, execute_command runs them (undoable). Typical flow: execute_command comp.new -> execute_command layer.newText / layer.newSolid (returns the layer id) -> set_property / add_keyframe -> execute_command effect.apply -> render_frame to look at the result. Inspect with get_project, get_comp and get_layer (every property node carries its `path`, e.g. `transform/position`, `effects/#1/blurriness`). Times are seconds. In bridge mode (app started with `--control <port>`) screenshot and the ui_* tools show and operate the live window.";

// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub struct McpServer {
    backend: Backend,
    autosave: Option<AutoSave>,
}

impl McpServer {
    pub fn new(backend: Backend) -> Self {
        Self { backend, autosave: None }
    }

    /// Opt in to durable headless checkpoints in a separate folder per server.
    /// `root` is the platform config directory; settings are read by the caller.
    pub fn with_autosave(mut self, root: &Path) -> crate::Result<Self> {
        let s = self.backend.session().ok_or_else(|| crate::Error::BadArgs("--autosave is for headless MCP; the bridged app owns its auto-saves".into()))?;
        s.autosave.background = false;
        let autosave = AutoSave::new(root, &s.prefs).map_err(|e| crate::Error::Other(format!("MCP auto-save: {e}")))?;
        s.autosave_folder_override = Some(autosave.folder().to_path_buf());
        self.autosave = Some(autosave);
        Ok(self)
    }

    fn autosave_info(&self) -> Value {
        self.autosave.as_ref().map(AutoSave::info).unwrap_or_else(|| json!({"enabled": false}))
    }

    fn checkpoint(&mut self, finish: bool) -> std::io::Result<()> {
        match (&mut self.autosave, self.backend.session()) {
            (Some(a), Some(s)) => a.checkpoint(s, finish),
            _ => Ok(()),
        }
    }

    pub fn backend(&mut self) -> &mut Backend {
        &mut self.backend
    }

    /// Handle one decoded message (request, notification or batch); `None` when no reply is due.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        if let Value::Array(batch) = msg {
            if batch.is_empty() {
                return Some(error(Value::Null, INVALID_REQUEST, "empty batch"));
            }
            let out: Vec<Value> = batch.iter().filter_map(|m| self.handle(m)).collect();
            return (!out.is_empty()).then_some(Value::Array(out));
        }
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response from the client (we send no requests) or garbage.
            return msg
                .get("id")
                .filter(|_| msg.get("result").is_none() && msg.get("error").is_none())
                .map(|id| error(id.clone(), INVALID_REQUEST, "missing `method`"));
        };
        let Some(id) = msg.get("id").cloned() else {
            return None; // notification (notifications/initialized, notifications/cancelled, ...)
        };
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        Some(match self.request(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, m)) => error(id, code, &m),
        })
    }

    /// Handle one line of input; `None` when no reply is due.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle(&msg)?,
            Err(e) => error(Value::Null, PARSE_ERROR, &format!("parse error: {e}")),
        };
        Some(reply.to_string())
    }

    /// Serve until EOF on `input`.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        let served = (|| {
            for line in input.lines() {
                if let Some(reply) = self.handle_line(&line?) {
                    output.write_all(reply.as_bytes())?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
            }
            Ok(())
        })();
        match (served, self.checkpoint(true)) {
            (r, Ok(())) => r,
            (Ok(()), Err(e)) => Err(e),
            (Err(transport), Err(save)) => Err(std::io::Error::other(format!("{transport}; {save}"))),
        }
    }

    /// Serve on stdin/stdout (the MCP stdio transport).
    pub fn serve_stdio(&mut self) -> std::io::Result<()> {
        self.serve(std::io::stdin().lock(), std::io::stdout().lock())
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
                let mode = if self.backend.is_bridge() { "bridge" } else { "headless" };
                let instructions = if self.autosave.is_some() {
                    format!("{INSTRUCTIONS} Headless auto-save and recovery: {}", self.autosave_info())
                } else {
                    INSTRUCTIONS.to_string()
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "effectcraft", "title": format!("EffectCraft ({mode})"), "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions,
                    "_meta": {"effectcraftAutoSave": self.autosave_info()},
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let tools: Vec<Value> = tools::available(self.backend.is_bridge()).map(|t| t.descriptor()).collect();
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                if tools::available(self.backend.is_bridge()).all(|t| t.name != name) {
                    let hint = if tools::find(name).is_some() { format!(" ({})", crate::backend::NEED_BRIDGE) } else { String::new() };
                    return Err((INVALID_PARAMS, format!("unknown tool `{name}`{hint}")));
                }
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let mut result = call_result(tools::run(&mut self.backend, name, &args));
                // Also checkpoint a partially successful tool that returned an error. The
                // project is durable before the client receives its reply (even if it kills
                // the process immediately afterwards).
                if let Err(e) = self.checkpoint(false) {
                    let _ = writeln!(std::io::stderr(), "{e}");
                    if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
                        content.push(json!({"type": "text", "text": format!("Warning: {e}. Use save_project with a writable path before disconnecting.")}));
                    }
                }
                if self.autosave.is_some()
                    && let Some(obj) = result.as_object_mut()
                {
                    let meta = obj.entry("_meta").or_insert_with(|| json!({}));
                    if let Some(meta) = meta.as_object_mut() {
                        meta.insert("effectcraftAutoSave".into(), self.autosave_info());
                    }
                }
                if self.autosave.is_some()
                    && name == "get_project"
                    && result["isError"] == false
                    && let Some(text) = result.get_mut("content").and_then(Value::as_array_mut).and_then(|c| c.first_mut()).and_then(|c| c.get_mut("text"))
                    && let Some(summary) = text.as_str().and_then(|t| serde_json::from_str::<Value>(t).ok())
                {
                    let mut summary = summary;
                    summary["autosave"] = self.autosave_info();
                    *text = Value::String(summary.to_string());
                }
                Ok(result)
            }
            // Advertised as absent, but answer politely for clients that probe anyway.
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            m => Err((METHOD_NOT_FOUND, format!("method not found: {m}"))),
        }
    }
}

/// A `tools/call` result: tool failures are reported in-band (`isError`) so the model can react.
pub fn call_result(r: Result<Reply, Error>) -> Value {
    match r {
        Ok(Reply::Json(v)) => json!({"content": [{"type": "text", "text": v.to_string()}], "isError": false}),
        Ok(Reply::Image { png, info }) => json!({
            "content": [
                {"type": "image", "data": base64::encode(&png), "mimeType": "image/png"},
                {"type": "text", "text": info.to_string()},
            ],
            "isError": false,
        }),
        Err(e) => json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true}),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
