# scher-stacks scaffold design (file-light, self-ministry, 2026-07-03)

Built to `penelope-gen4/docs/committees/2026-07-03-scher-stacks-design.md` — that ruling is not
relitigated here. This is the concrete layout the labor phase writes verbatim: file paths,
`Cargo.toml`, migration SQL, `src/` skeleton. Every choice below traces to a specific ruling or
holdout in that minute; citations inline as `[ruling: …]`.

## 1. Repo layout

Single binary, no workspace. A workspace only earns its keep once there are ≥2 crates that need
independent versioning or separate publish; today there's one binary path-depending on one
external crate (`scher-core`), which is exactly `gen4-policy`'s existing shape
`[ruling: "Dependency shape"]`. Don't build the workspace `Cargo.toml` ceremony for a repo this
size — add it later if a second crate (e.g. a provenance module, deferred) actually needs it.

```
scher-stacks/
  Cargo.toml
  Cargo.lock                    (committed — this is a binary, not a library)
  README.md
  ARCHITECTURE.md               (the discipline, translated per the ruling — see §6)
  migrations/
    0001_event.sql
  src/
    main.rs                     binary entrypoint: config, pool, router, MCP listener, serve
    config.rs                   env-var config (DATABASE_URL, BIND_ADDR, etc.) — no framework
    store.rs                    the persistence seam: EventRow <-> Postgres, per-tenant clock
    rest.rs                     Axum router + handlers (REST surface)
    mcp.rs                      MCP surface (hand-rolled JSON-RPC over stdio or HTTP — see §5)
    wire.rs                     wire structs mirroring EventRow field-for-field (no derive-magic
                                 beyond serde; this is the "hand-written wire structs" ruling)
    error.rs                    one error enum, mapped to REST status codes and MCP error objects
  tests/
    conformance_roundtrip.rs    persist(soc) -> load reproduces identical Society (H1)
    conformance_inert_lay.rs   relaying an existing (stack, slug) through HTTP is a no-op
    conformance_as_of.rs        Trajectory-style replay-as-of correctness through the store
    tenancy_isolation.rs        two stacks, colliding slugs, no cross-tenant clock/read leak
```

No `db/` directory, no ORM-generated schema folder — migrations are the only SQL artifact and
they live at the repo root under `migrations/`, run by a plain runner (see §4), not a framework
CLI baked into the build.

## 2. `Cargo.toml`

Every dependency individually justified, per `ARCHITECTURE.md`'s translated discipline
`[ruling: "a short, individually-justified dependency list, not a framework's whole ecosystem"]`.
Versions match what `api/Cargo.toml` already pins, where the same crate is reused, so the two
repos' proven-good versions don't drift apart for no reason.

```toml
[package]
name = "scher-stacks"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Standing REST+MCP service over a Postgres-backed scher-core Society, one Society per tenant (stack). Append-only wire: no PUT/PATCH/DELETE."

[[bin]]
name = "scher-stacks"
path = "src/main.rs"

[dependencies]
scher-core = { path = "../penelope-gen4/scher/scher-core" }

# REST surface.
axum = "0.7"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "signal"] }

# wire (de)serialization — hand-written structs, serde only does the mechanical part.
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# raw SQL, no ORM — query_as into hand-written structs, migrate! macro for plain numbered .sql
# files (compile-time-checked file presence only; it does not generate schema or queries).
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "postgres", "macros", "migrate"] }

[dev-dependencies]
proptest = "1"          # matches scher-core's own property-testing discipline
```

Notably absent, on purpose: no `tower-http` grab-bag, no `utoipa`/OpenAPI generator (codegen ban),
no `diesel`/`sea-orm` (ORM ban), no MCP SDK crate — §5 explains why MCP is hand-rolled rather than
a dependency.

The path-dep is literal and relative (`../penelope-gen4/scher/scher-core`), matching how
`gen4-policy` depends on it today. Both repos must be checked out as siblings for this to build —
document that in `README.md`, not solved by vendoring or a registry publish (out of scope; nothing
in the ruling asks for scher-core to be published).

## 3. The migration: `migrations/0001_event.sql`

This is the schema every holdout (H1, H2, H3, H4) gates on. `EventRow`'s own fields
(`slug`, `content`, `title`, `subject`, `object`, `witnessed`) plus exactly one tenancy column,
`stack`, scoping a whole Society per tenant — never a slug prefix
`[ruling: Refusal 2, "What generalizes": "Multi-tenancy"]`.

```sql
-- 0001_event.sql
-- The one table. EventRow's own shape (scher-core/src/lib.rs), witnessed included — never
-- CanonRow's amputated shape (api/src/main.rs:710, the TODO(socratic) every light cited).
-- `stack` is the tenant boundary: one Society per stack, never a slug prefix within one Society
-- (Refusal 2 — lay/prehensions_onto/interval_of/{slug}~q all assume one flat opaque-slug space;
-- tenancy has to be a real column scoping which space a slug lives in, not text inside the slug).

CREATE TABLE stack.event (
    seq         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    stack       TEXT   NOT NULL,
    slug        TEXT   NOT NULL,
    content     TEXT   NOT NULL DEFAULT '',
    title       TEXT,
    subject     TEXT,
    object      TEXT,
    -- Society-assigned, per-tenant monotone clock. Persisted AS GIVEN by the store's own
    -- in-memory Society::insert ratchet (scher-core lib.rs:107-118: witnessed.unwrap_or(clock+1),
    -- clock = clock.max(witnessed)) — never re-minted from a table-global MAX(witnessed) at
    -- persist time. See store.rs (§4) for how the per-tenant clock is computed on write.
    witnessed   BIGINT NOT NULL,

    -- Refusal 2, made structural: a slug is only unique WITHIN a stack. Two tenants may each
    -- have their own "day-2026-07-03" node; they are different rows, different addresses,
    -- never the same row, never cross-visible.
    CONSTRAINT event_stack_slug_unique UNIQUE (stack, slug)
);

-- Every real read path (load-by-stack, as-of scans) filters by stack first; this index makes
-- that filter — and the ORDER BY witnessed, seq load pattern api/src/main.rs already proved out —
-- cheap without leaning on the UNIQUE constraint's own index for range scans.
CREATE INDEX event_stack_witnessed_idx ON stack.event (stack, witnessed, seq);

-- Structural reads (subject/object graph walks, prehensions_onto-over-HTTP) always scope by
-- stack too — never soc.all()-style cross-tenant scans (the kernel light's named unsafety).
CREATE INDEX event_stack_subject_idx ON stack.event (stack, subject) WHERE subject IS NOT NULL;
CREATE INDEX event_stack_object_idx  ON stack.event (stack, object)  WHERE object  IS NOT NULL;
```

Notes on choices, each pinned to a ruling:

- **Schema-qualified table (`stack.event`), not `public.event`.** Mirrors `canon.event`'s own
  precedent in `api/src/main.rs` (a named schema, not the default) — a style match, not a new
  invention, and it reads as "the stack schema" which is the whole point of the tenant column.
  `CREATE SCHEMA IF NOT EXISTS stack;` is migration `0000_schema.sql` (below), run before it.
- **No `content_ct`/`nonce`/`beat_key_id`/`enc_alg`/`hash_version`/`keyring` anything.** Crypto
  stays upstream in each consumer — this table has no crypto organs at all
  `[already ruled, before this sitting; reconfirmed: "its crypto organs ... stay behind in
  penelope-gen4"]`. `content`/`title` are plain columns, plaintext, because scher-stacks itself
  never encrypts; a consumer that needs encryption does it before the row ever reaches this wire.
- **`content TEXT NOT NULL DEFAULT ''`, not nullable.** `EventRow.content` in scher-core is a bare
  `String`, never `Option<String>` — the wire shape should refuse the same amputation `CanonRow`
  committed (nullable content) in the other direction: `EventRow` already answers "empty vs
  absent" as "empty", so the column shouldn't reopen a question the kernel already closed.
- **`witnessed BIGINT NOT NULL`, not nullable, not `TIMESTAMPTZ`.** `EventRow.witnessed` is
  `Option<u64>` in memory only because a beat *not yet laid* has no clock — every row that reaches
  Postgres has already been laid (`Society::insert` always sets `Some(_)` before the row exists),
  so `NOT NULL` at rest is correct and closes off the H1 failure mode ("witnessed is None, is it
  404 or unclocked?" — TODO(socratic) at api/src/main.rs:132 — moot here: it can't happen). It's a
  logical clock (a ratcheted counter), not wall time, so `BIGINT` not `TIMESTAMPTZ`.
- **No `beat_hash` column.** That's `CanonRow`'s crypto-authority concept
  (`beat_hash_v2`/`hash_version`), explicitly named as staying behind. The deferred hash-chained
  provenance module (Blocked/deferred, last item) is a *separate future table or column*, built
  only once scher-stacks exists as real code — not smuggled into 0001 now.
- **`seq BIGINT GENERATED ALWAYS AS IDENTITY`, not `SERIAL`.** Same tie-break role `seq` plays in
  `api/src/main.rs`'s `ORDER BY witnessed, seq` (ties in `witnessed` — e.g. a batch lay — resolve
  by insertion order); `GENERATED ALWAYS AS IDENTITY` is the modern non-deprecated spelling of the
  same mechanism, no behavior change.

`migrations/0000_schema.sql` (runs first, numbered so plain lexical/numeric migration ordering —
which `sqlx migrate` uses natively — puts it before `0001`):

```sql
-- 0000_schema.sql
CREATE SCHEMA IF NOT EXISTS stack;
```

## 4. `src/store.rs` — the persistence seam, corrected not copied

This is the one file every holdout gates on (H1). It generalizes `persist_society_pg`/
`load_canon_pg`'s *pattern* — lock-inside/persist-outside, `ON CONFLICT DO NOTHING` dedup by
address, `ORDER BY witnessed, seq` on load — but fixes the two things every light named:

**Fix A — per-tenant clock, never a global `MAX`.** `api/src/main.rs:838` computes
`SELECT COALESCE(MAX(witnessed), 0) FROM canon.event` — table-global. scher-stacks scopes that
by `stack`:

```rust
// store.rs — mint the next per-tenant clock value, mirroring Society::insert's own ratchet
// (scher-core lib.rs: witnessed.unwrap_or(clock + 1), clock = clock.max(witnessed)) but scoped
// per stack so one tenant's writes can never advance another tenant's frontier.
async fn next_witnessed(pool: &PgPool, stack: &str, given: Option<i64>) -> sqlx::Result<i64> {
    let base: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(witnessed), 0) FROM stack.event WHERE stack = $1",
    )
    .bind(stack)
    .fetch_one(pool)
    .await?;
    Ok(match given {
        Some(w) => base.max(w),   // an explicit witness may tie or exceed base; never regress it
        None => base + 1,         // un-witnessed rows get daylight from this tenant's own frontier
    })
}
```

Every query in `store.rs` — this one, the dedup pre-check, the insert, the load — carries
`WHERE stack = $1` (or `stack.event(stack, ...)` positionally). There is no query in this file that
touches `stack.event` without a stack bound; that's the structural form of Refusal 2, not just the
schema constraint.

**Fix B — `witnessed` rides the wire, persisted as given, never re-minted at persist time for rows
that already carry it.** The wire struct (`wire.rs`) has `witnessed: Option<i64>` field-for-field
with `EventRow`; when a client-laid beat arrives with `Some(w)` already set (the reader-muslin
case: offline lay-order stamped by the client's own Society), that value is authoritative and
persisted as-is (still `.max()`'d against this tenant's base so the per-tenant clock stays
monotone — an explicit witness can tie the frontier but the store never *invents* a smaller one).
Only a row that arrives with `witnessed: None` gets the tenant's `base + 1`. This is the literal
fix for the `TODO(socratic)` at `api/src/main.rs:710`.

**Fix C — the SELECT-then-INSERT race, flagged not-blocking by H1, fixed here since this is fresh
code with no precedent to inherit.** Rather than a separate `SELECT 1 FROM stack.event WHERE
stack=$1 AND slug=$2` pre-check race against a later `INSERT ... ON CONFLICT DO NOTHING` (two
concurrent writers can both see "absent" and then both mint), use a single statement with
`RETURNING` to learn atomically whether the row was genuinely new:

```rust
// store.rs — one round trip, atomic dedup-and-insert. rows_affected() == 1 means genuine append
// (mirrors Society::lay's own true/false); == 0 means inert (existing (stack, slug), no-op).
pub async fn lay_row(pool: &PgPool, stack: &str, row: &wire::EventRow) -> sqlx::Result<bool> {
    let witnessed = next_witnessed(pool, stack, row.witnessed).await?;
    let r = sqlx::query(
        r#"INSERT INTO stack.event (stack, slug, content, title, subject, object, witnessed)
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
    Ok(r.rows_affected() == 1)
}
```

This still has a narrow race window between `next_witnessed`'s read and the `INSERT` (two
concurrent un-witnessed lays in the same stack could both compute `base + 1` and collide on
`witnessed` — not on `slug`, so both would insert, landing at the same clock tick with `seq`
breaking the tie). That's an accepted, named tradeoff — not the amputation H1 was refusing (no
data loss, no silent reorder past what `ORDER BY witnessed, seq` already resolves), and it matches
`Society::insert`'s own single-threaded ratchet closely enough that a stricter fix (e.g.
`SELECT ... FOR UPDATE` scoped to the stack, or an advisory lock keyed by `stack`) is a fast-follow
noted in `store.rs`'s own doc-comment, not a blocker for the scaffold.

**Load, scoped and ordered exactly like the proven pattern:**

```rust
pub async fn load_stack(pool: &PgPool, stack: &str) -> sqlx::Result<scher_core::Society> {
    let rows: Vec<wire::EventRow> = sqlx::query_as(
        r#"SELECT slug, content, title, subject, object, witnessed
           FROM stack.event WHERE stack = $1 ORDER BY witnessed, seq"#,
    )
    .bind(stack)
    .fetch_all(pool)
    .await?;

    let mut soc = scher_core::Society::new();
    for r in rows {
        soc.lay(r.into()); // wire::EventRow -> scher_core::EventRow, field-for-field, witnessed intact
    }
    Ok(soc)
}
```

## 5. REST + MCP over one pool, append-only verb set

`src/rest.rs` — Axum router, one pool shared with the MCP surface (the ruling: "one Rust binary
serving REST + MCP over one Postgres pool"):

```rust
// rest.rs — every route is a GET (read) or POST (lay). No PUT, no PATCH, no DELETE — not omitted
// by oversight, refused structurally: these verbs are never matched by the router at all, so
// there's no handler to accidentally wire up later. [ruling: H2, non-negotiable]
Router::new()
    .route("/stacks/:stack/events", get(list_events).post(lay_event))
    .route("/stacks/:stack/events/:slug", get(get_event))
    .route("/stacks/:stack/reaches", get(reaches))   // the gift-channel `reaches` extraction,
                                                       // served over HTTP [ruling: "gift-channel ask"]
    .with_state(ctx)
```

`lay_event` (`POST /stacks/:stack/events`) accepts a single `wire::EventRow` (or an array for
batch-lay, mirroring `lay_all`), calls `store::lay_row` per row, returns which slugs were genuinely
appended vs inert — the HTTP-surface expression of "lay of a colliding slug is inert, no error"
that the conformance test (`tests/conformance_inert_lay.rs`) checks isn't silently violated by, say,
an over-eager 409 on the wire layer.

`src/mcp.rs` — no MCP SDK crate exists in this ecosystem yet (not pulled in on spec, per the "short,
individually-justified dependency list" discipline — pulling an unaudited framework crate to save
writing ~150 lines of JSON-RPC dispatch is exactly the ORM trade the ruling refused in spirit). MCP
is JSON-RPC 2.0 over stdio (or an HTTP transport if the consuming client needs that — same
dispatch table either way), hand-dispatched to the same `store.rs` functions `rest.rs` calls:

```rust
// mcp.rs — the tool surface is a literal mirror of rest.rs's routes, same append-only law: tools
// are named lay_event / list_events / get_event / reaches. No delete_event, no update_event tool
// exists — the same structural refusal as the REST router, expressed as "no such JSON-RPC method"
// rather than "no such HTTP route".
```

If a future pass wants a real MCP SDK crate once one is vetted, this file is the whole surface
that needs to change — `store.rs` stays untouched, which is the point of the seam.

## 6. `ARCHITECTURE.md` — translated, not transplanted

A short doc (not copied from scher-core's) stating the four translated disciplines the ruling
named, each with its literal enforcement point:

1. **Raw SQL, no ORM.** Enforced by dependency choice (§2: `sqlx` only, no `diesel`/`sea-orm`) —
   every query in `store.rs` is a literal `.sql`-shaped string, `query_as` into hand-written
   structs.
2. **No codegen.** Migrations are plain numbered `.sql` files (§3); wire structs are hand-written
   in `wire.rs` (§5); no OpenAPI/schema generator dependency exists to produce either.
3. **Short, justified dependency list.** §2's table is the whole list; any addition to
   `Cargo.toml` should append a one-line justification comment at the point of addition, same
   convention `scher-core`'s own `Cargo.toml` doc-comment models.
4. **Append-only made structural in the verb set.** §5: no PUT/PATCH/DELETE route or MCP tool
   exists to remove — "undo is occlusion, which is just more rows."

## 7. What this scaffold explicitly does NOT do (deferred, per the ruling)

- No migration of penelope-gen4's own persistence onto this store (Blocked/deferred).
- No hash-chained provenance module/column in `0001` (Blocked/deferred — a real design tension
  on canonicalizing a prehended set was named and explicitly left unresolved; building it now
  would be discovering that bug later instead of designing it first).
- No resolution of the `{slug}~q` mode-beat convention's tension with opaque-slugs (H2, flagged
  not blocking) — `lay_p`'s `~q` convention is inherited as-is from scher-core's own grammar via
  `store.rs`'s use of `scher_core::Society`, not re-litigated at the wire layer in this pass.
- No workspace, no second crate, no vendoring/publishing of `scher-core` — the path-dep is literal
  and requires sibling checkouts (§2).
