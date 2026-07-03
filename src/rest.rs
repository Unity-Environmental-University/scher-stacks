// rest.rs — Axum router + handlers. Every route is a GET (read) or POST (append). No PUT, no
// PATCH, no DELETE — not omitted by oversight, refused structurally: these verbs are never
// matched by the router at all, so there is no handler to accidentally wire up later.
// [ruling H2, non-negotiable: "the append-only law made structural in the verb set: no PUT, no
// PATCH, no DELETE exists on the wire at all."]

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::StoreError;
use crate::store;
use crate::wire::{EventRow, LayResult};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

/// Builds the REST routes over `AppState`. Left stateful (`Router<AppState>`, no `.with_state`
/// call here) so `main.rs` can `.merge()` this with the MCP route and apply `.with_state` once,
/// at the top level, for the whole app — one pool, one state type, one place it's supplied.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stacks/:stack/events", get(list_events).post(lay_event))
        .route("/stacks/:stack/events/:slug", get(get_event))
        .route("/stacks/:stack/reaches", get(reaches))
}

async fn list_events(
    State(state): State<AppState>,
    Path(stack): Path<String>,
) -> Result<Json<Vec<EventRow>>, StoreError> {
    let rows = store::list_rows(&state.pool, &stack).await?;
    Ok(Json(rows))
}

async fn get_event(
    State(state): State<AppState>,
    Path((stack, slug)): Path<(String, String)>,
) -> Result<Json<EventRow>, StoreError> {
    let row = store::get_row(&state.pool, &stack, &slug).await?;
    Ok(Json(row))
}

/// Accepts a single EventRow or a batch (array), mirroring `lay`/`lay_all`. Returns which slugs
/// were genuinely appended vs inert — the HTTP-surface expression of "lay of a colliding slug is
/// inert, no error" (Society::lay's own bool, surfaced straight through, no invented 409).
#[derive(Deserialize)]
#[serde(untagged)]
enum LayBody {
    One(EventRow),
    Many(Vec<EventRow>),
}

async fn lay_event(
    State(state): State<AppState>,
    Path(stack): Path<String>,
    Json(body): Json<LayBody>,
) -> Result<Json<Vec<LayResult>>, StoreError> {
    let rows = match body {
        LayBody::One(r) => vec![r],
        LayBody::Many(rs) => rs,
    };
    let results = store::lay_rows(&state.pool, &stack, &rows).await?;
    Ok(Json(
        results
            .into_iter()
            .map(|(slug, appended)| LayResult { slug, appended })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct ReachesQuery {
    pub to: String,
    pub quality: String,
    pub as_of: Option<i64>,
}

/// The gift-channel ask: `prehensions_onto` served over HTTP. Loads the tenant's Society (one
/// Society per stack, never one shared Society filtered after the fact — the kernel light's
/// named hazard) and reads `prehensions_onto(soc, to, quality, as_of)`.
/// [ruling: "the generic reaches(soc, from, to, quality, as_of) extraction ... is directly what
/// scher-stacks wants, generalized to being served persistently — prehensions_onto over HTTP."]
async fn reaches(
    State(state): State<AppState>,
    Path(stack): Path<String>,
    Query(q): Query<ReachesQuery>,
) -> Result<Json<Vec<EventRow>>, StoreError> {
    let soc = store::load_stack(&state.pool, &stack).await?;
    let as_of = q.as_of.map(|t| t as u64);
    let hits = scher_core::prehensions_onto(&soc, &q.to, &q.quality, as_of);
    Ok(Json(hits.into_iter().map(Into::into).collect()))
}
