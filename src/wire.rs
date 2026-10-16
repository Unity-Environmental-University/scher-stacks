// wire.rs — hand-written wire structs mirroring scher_core::EventRow field-for-field.
// No OpenAPI generator, no derive-magic beyond serde's mechanical (de)serialization.
// [ruling: "hand-written wire structs mirroring EventRow field-for-field, no OpenAPI generator"]

use serde::{Deserialize, Serialize};

/// The wire shape of an EventRow, as it crosses HTTP/MCP or a Postgres row. Field-for-field
/// with scher_core::EventRow, plus nothing extra — tenancy (`stack`) is carried alongside this
/// struct by the route/query, never as a field on it, mirroring the kernel's own refusal to let
/// EventRow itself carry a tenant field.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventRow {
    pub slug: String,
    pub content: String,
    pub title: Option<String>,
    pub subject: Option<String>,
    pub object: Option<String>,
    /// Society-assigned. `None` on the way IN means "let the store's per-tenant clock assign
    /// one"; always `Some` on the way OUT (every persisted row has already been laid).
    pub witnessed: Option<i64>,
    /// The frame that laid the row, as the consumer names it. `None` when nobody said.
    #[serde(default)]
    pub laid_by: Option<String>,
}

impl From<EventRow> for scher_core::EventRow {
    fn from(w: EventRow) -> Self {
        let mut row = if let (Some(s), Some(o)) = (w.subject.as_deref(), w.object.as_deref()) {
            scher_core::EventRow::edge(&w.slug, &w.content, s, o)
        } else {
            scher_core::EventRow::node(&w.slug, &w.content)
        };
        row.title = w.title;
        if let Some(t) = w.witnessed {
            row = row.with_witnessed(t as u64);
        }
        // laid_by stays on the wire and in Postgres only: the pinned scher-core predates its
        // own laid_by (66c0af0), and nothing the store reads through a Society needs it.
        row
    }
}

impl From<&scher_core::EventRow> for EventRow {
    fn from(r: &scher_core::EventRow) -> Self {
        EventRow {
            slug: r.slug.clone(),
            content: r.content.clone(),
            title: r.title.clone(),
            subject: r.subject.clone(),
            object: r.object.clone(),
            witnessed: r.witnessed.map(|t| t as i64),
            laid_by: None,
        }
    }
}

/// Response to a lay: which slugs genuinely appended vs stayed inert (already existed). This is
/// `Society::lay`'s own `bool` surfaced straight through — no invented 409, no error type the
/// kernel doesn't have. [scher-core-lib: "Surface lay's bool as the HTTP-level idempotency
/// signal I already give you, don't invent an error type I don't have."]
#[derive(Debug, Serialize)]
pub struct LayResult {
    pub slug: String,
    pub appended: bool,
}
