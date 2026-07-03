// main.rs — binary entrypoint: config, pool, router, MCP listener, serve. One Rust binary
// serving REST + MCP over one Postgres pool, per the ruling
// (docs/committees/2026-07-03-scher-stacks-design.md, penelope-gen4 repo).

mod config;
mod error;
mod mcp;
mod provenance;
mod rest;
mod store;
mod wire;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    let cfg = config::Config::from_env();

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await
        .expect("failed to connect to Postgres");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    let state = rest::AppState { pool: pool.clone() };

    // One pool, two surfaces: REST (rest.rs) and MCP (mcp.rs), same store.rs functions
    // underneath. MCP is exposed here as a plain JSON-RPC POST endpoint (hand-rolled dispatch,
    // no MCP SDK crate) — the same binary, same pool, no separate process. Both surfaces share
    // one `AppState`, so the router carries a single consistent state type throughout, applied
    // once at the top level.
    let app = Router::new()
        .merge(rest::router())
        .route("/mcp", post(mcp_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr)
        .await
        .expect("failed to bind");
    tracing_stub(&cfg.bind_addr);
    axum::serve(listener, app).await.expect("server error");
}

async fn mcp_handler(
    State(state): State<rest::AppState>,
    Json(req): Json<mcp::RpcRequest>,
) -> Json<mcp::RpcResponse> {
    Json(mcp::dispatch(&state.pool, req).await)
}

/// A stand-in for real structured logging (tracing/tracing-subscriber deferred — not in the
/// ruling, would be the first line cut if the dependency list is judged too long for a first
/// cut). Kept as a named, visible seam rather than a silent eprintln scattered elsewhere.
fn tracing_stub(bind_addr: &str) {
    eprintln!("scher-stacks listening on {bind_addr}");
}
