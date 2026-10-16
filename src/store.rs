// store.rs — the persistence seam: EventRow <-> Postgres, per-tenant clock. This is the one
// file every holdout gates on (H1, docs/committees/2026-07-03-scher-stacks-design.md).
// Generalizes api/src/main.rs's persist_society_pg/load_canon_pg *pattern* (lock-inside/
// persist-outside, ON CONFLICT DO NOTHING dedup, ORDER BY witnessed, seq on load) — but fixes
// the two wounds every light named: witnessed is per-tenant and Society-assigned (never a
// table-global MAX), and every query here is stack-scoped, structurally, not by convention.

use sqlx::PgPool;

use crate::error::StoreError;
use crate::wire::EventRow;

/// Mint the next per-tenant clock value, mirroring Society::insert's own ratchet
/// (scher-core lib.rs: `witnessed.unwrap_or(clock + 1)`, `clock = clock.max(witnessed)`) but
/// scoped per stack so one tenant's writes can never advance another tenant's frontier.
///
/// [ruling Fix A: "per-tenant clock, never a global MAX" — api/src/main.rs:838's own
/// table-global `SELECT COALESCE(MAX(witnessed),0) FROM canon.event` is the wound this scopes.]
async fn next_witnessed(pool: &PgPool, stack: &str, given: Option<i64>) -> sqlx::Result<i64> {
    let base: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(witnessed), 0) FROM stacks.event WHERE stack = $1")
            .bind(stack)
            .fetch_one(pool)
            .await?;
    Ok(match given {
        // An explicit witness may tie or exceed base; the store never invents a smaller one.
        Some(w) => base.max(w),
        // Un-witnessed rows get daylight from this tenant's own frontier, never a global one.
        None => base + 1,
    })
}

/// Lay one row into a stack. One atomic INSERT ... ON CONFLICT DO NOTHING; `rows_affected() == 1`
/// means genuine append (mirrors `Society::lay`'s own `true`/`false`), `== 0` means inert
/// (existing (stack, slug), no-op, no error — the append-only law's silence, on purpose).
///
/// [ruling Fix C: this replaces a separate SELECT-then-INSERT pre-check (H1's flagged,
/// non-blocking clock race, walk-plan B#6) with a single statement. A narrow race remains
/// between `next_witnessed`'s read and this INSERT — two concurrent un-witnessed lays in the
/// same stack could both compute `base + 1` and land at the same witnessed tick, `seq` breaking
/// the tie. That's an accepted, named tradeoff, not silent data loss or reordering; a stricter
/// fix (SELECT ... FOR UPDATE scoped to stack, or an advisory lock keyed by stack) is a
/// fast-follow, not a scaffold blocker.]
pub async fn lay_row(pool: &PgPool, stack: &str, row: &EventRow) -> Result<bool, StoreError> {
    let witnessed = next_witnessed(pool, stack, row.witnessed).await?;
    let result = sqlx::query(
        r#"INSERT INTO stacks.event (stack, slug, content, title, subject, object, witnessed)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (stack, slug) DO NOTHING"#,
    )
    .bind(stack)
    .bind(&row.slug)
    .bind(&row.content)
    .bind(&row.title)
    .bind(&row.subject)
    .bind(&row.object)
    .bind(witnessed)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Bulk-lay; each row laid independently (own witnessed assignment), mirroring `Society::lay_all`
/// closely enough for the scaffold — one HTTP response reports per-slug appended/inert.
pub async fn lay_rows(
    pool: &PgPool,
    stack: &str,
    rows: &[EventRow],
) -> Result<Vec<(String, bool)>, StoreError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let appended = lay_row(pool, stack, row).await?;
        out.push((row.slug.clone(), appended));
    }
    Ok(out)
}

/// Load a single stack's full row history into an in-memory Society, ordered by
/// `witnessed, seq` — the clock orders the story; `seq` is the tie-break, never the ordering
/// itself. Every row's persisted `witnessed` rides in via `.with_witnessed()` (through the
/// `wire::EventRow -> scher_core::EventRow` conversion) so `Society::insert` never re-mints it.
///
/// [scher-core-lib: "Build one Society per stack ... Society::seeded() from that tenant's rows
/// ordered by witnessed then insertion, with each EventRow carrying its persisted witnessed."]
pub async fn load_stack(pool: &PgPool, stack: &str) -> Result<scher_core::Society, StoreError> {
    let rows: Vec<EventRow> = sqlx::query_as(
        r#"SELECT slug, content, title, subject, object, witnessed
           FROM stacks.event WHERE stack = $1 ORDER BY witnessed, seq"#,
    )
    .bind(stack)
    .fetch_all(pool)
    .await?;

    let seed: Vec<scher_core::EventRow> = rows.into_iter().map(Into::into).collect();
    Ok(scher_core::Society::seeded(&seed))
}

/// Load a single row (for `GET /stacks/:stack/events/:slug`).
pub async fn get_row(pool: &PgPool, stack: &str, slug: &str) -> Result<EventRow, StoreError> {
    sqlx::query_as(
        r#"SELECT slug, content, title, subject, object, witnessed
           FROM stacks.event WHERE stack = $1 AND slug = $2"#,
    )
    .bind(stack)
    .bind(slug)
    .fetch_optional(pool)
    .await?
    .ok_or(StoreError::NotFound)
}

/// List all rows for a stack, wire-shaped, ordered by witnessed then seq — the same ordering
/// `load_stack` uses, so a REST client sees exactly the order the Society itself would replay.
pub async fn list_rows(pool: &PgPool, stack: &str) -> Result<Vec<EventRow>, StoreError> {
    let rows: Vec<EventRow> = sqlx::query_as(
        r#"SELECT slug, content, title, subject, object, witnessed
           FROM stacks.event WHERE stack = $1 ORDER BY witnessed, seq"#,
    )
    .bind(stack)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
