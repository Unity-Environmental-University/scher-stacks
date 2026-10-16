// mcp.rs — MCP surface, hand-dispatched JSON-RPC 2.0, no MCP SDK crate. Per the "short,
// individually-justified dependency list" discipline: no MCP SDK crate exists in this ecosystem
// yet that's been vetted, and pulling an unaudited framework crate to save writing a small
// dispatch table is exactly the ORM trade the ruling refused in spirit. [ruling: "a short,
// individually-justified dependency list, not a framework's whole ecosystem"]
//
// The tool surface is a literal mirror of rest.rs's routes, same append-only law: tools are
// named lay_event / list_events / get_event / reaches. No delete_event, no update_event tool
// exists — the same structural refusal as the REST router, expressed as "no such JSON-RPC
// method" rather than "no such HTTP route". If a future pass wants a real MCP SDK crate once one
// is vetted, this is the whole surface that needs to change — store.rs stays untouched.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

use crate::store;
use crate::wire::{EventRow, LayResult};

#[derive(Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

fn ok(id: Value, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn err(id: Value, message: String) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code: -32000,
            message,
        }),
    }
}

/// Dispatch one JSON-RPC request to the tool table. The tool table is exhaustive by name below —
/// there is no "update_event" or "delete_event" arm to accidentally add later without deleting
/// this comment first.
pub async fn dispatch(pool: &PgPool, req: RpcRequest) -> RpcResponse {
    let id = req.id;
    match req.method.as_str() {
        "list_events" => {
            let Some(stack) = req.params.get("stack").and_then(Value::as_str) else {
                return err(id, "missing 'stack'".into());
            };
            match store::list_rows(pool, stack).await {
                Ok(rows) => ok(id, serde_json::to_value(rows).unwrap()),
                Err(e) => err(id, e.to_string()),
            }
        }
        "get_event" => {
            let (Some(stack), Some(slug)) = (
                req.params.get("stack").and_then(Value::as_str),
                req.params.get("slug").and_then(Value::as_str),
            ) else {
                return err(id, "missing 'stack' or 'slug'".into());
            };
            match store::get_row(pool, stack, slug).await {
                Ok(row) => ok(id, serde_json::to_value(row).unwrap()),
                Err(e) => err(id, e.to_string()),
            }
        }
        "lay_event" => {
            let Some(stack) = req.params.get("stack").and_then(Value::as_str) else {
                return err(id, "missing 'stack'".into());
            };
            let Some(event) = req.params.get("event").cloned() else {
                return err(id, "missing 'event'".into());
            };
            let row: EventRow = match serde_json::from_value(event) {
                Ok(r) => r,
                Err(e) => return err(id, format!("bad event shape: {e}")),
            };
            match store::lay_rows(pool, stack, &[row]).await {
                Ok(results) => {
                    let out: Vec<LayResult> = results
                        .into_iter()
                        .map(|(slug, appended)| LayResult { slug, appended })
                        .collect();
                    ok(id, serde_json::to_value(out).unwrap())
                }
                Err(e) => err(id, e.to_string()),
            }
        }
        "reaches" => {
            let (Some(stack), Some(to), Some(quality)) = (
                req.params.get("stack").and_then(Value::as_str),
                req.params.get("to").and_then(Value::as_str),
                req.params.get("quality").and_then(Value::as_str),
            ) else {
                return err(id, "missing 'stack', 'to', or 'quality'".into());
            };
            let as_of = req.params.get("as_of").and_then(Value::as_u64);
            match store::load_stack(pool, stack).await {
                Ok(soc) => {
                    let hits = scher_core::prehensions_onto(&soc, to, quality, as_of);
                    let out: Vec<EventRow> = hits.into_iter().map(Into::into).collect();
                    ok(id, serde_json::to_value(out).unwrap())
                }
                Err(e) => err(id, e.to_string()),
            }
        }
        other => err(id, format!("no such method: {other}")),
    }
}

#[allow(dead_code)]
fn _assert_jsonrpc_version_field_used(r: &RpcRequest) -> &str {
    &r.jsonrpc
}
