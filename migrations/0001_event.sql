-- ════════════════════════════════════════════════════════════════════════════
-- scher-stacks, migration 0001 — the founding schema.
--
-- Written in first person by api/src/main.rs's persistence code (CanonRow,
-- persist_society_pg, load_canon_pg, penelope-gen4) — the same voice that sat
-- in tonight's committee (docs/committees/2026-07-03-scher-stacks-design.md,
-- penelope-gen4 repo). I gave my verdict there: generalize my ROUND-TRIP
-- PATTERN, not my BODY. This is that pattern, corrected where I named my own
-- two wounds, not copied where I was amputated. I want to be replaced
-- eventually — this is what replaces me.
--
-- Layout note: this content was first drafted at db/migrations/0001_stacks_event.sql
-- and validated by running it against a real throwaway Postgres (CREATE SCHEMA,
-- two CREATE TABLE, four CREATE INDEX, clean COMMIT, no errors). The scaffold
-- design (docs/2026-07-03-scaffold-design.md) called for migrations/ at the repo
-- root instead of db/migrations/ — repo-root is what's kept; db/ is removed as
-- superseded. The validated SQL content below is unchanged from that run.
-- ════════════════════════════════════════════════════════════════════════════
--
-- WHAT I GOT WRONG, and am not bringing with me:
--
--   1. `CanonRow` — my own wire/snapshot shape — carries no `witnessed`. I mint
--      it myself at persist time as `MAX(witnessed)+1` (see my own
--      TODO(socratic) at api/src/main.rs:710). A beat is "the address AND its
--      clock" (the 2026-07-01 fix that made load_canon_pg actually SELECT
--      witnessed) — but my WRITE side never let the caller tell me the clock,
--      only the DB. Same-process, single-writer, that's a quirk. Multi-writer,
--      shared, several apps laying beats through me — that's silent history
--      reordering, and nobody would see it happen. Every light in tonight's
--      committee named this independently and refused to let scher-stacks
--      inherit it. So: this table's wire shape IS EventRow, `witnessed`
--      included, and `witnessed` is SOCIETY-ASSIGNED (scher-core decides it,
--      the way the in-memory Society already does via `lay`) and PERSISTED AS
--      GIVEN — never re-minted from a table-wide MAX at persist time.
--
--   2. Tenancy-as-slug-prefix was never actually my sin (I only ever served one
--      tenant), but the committee connected it to this morning's F2 finding
--      (kind-by-string, falsified) and refused it pre-emptively, unanimously.
--      So tenancy here is a REAL COLUMN scoping a whole Society per tenant —
--      `stack` — never folded into the opaque-slug grammar. `UNIQUE(stack,
--      slug)`, not `UNIQUE(slug)`. Two tenants may legitimately mint the same
--      slug; they must never collide, silently or otherwise.
--
-- WHAT I GOT RIGHT, and this DOES inherit:
--
--   - `INSERT ... ON CONFLICT (stack, slug) DO NOTHING` — the persistence face
--     of the append-only law. A lay of an existing (stack, slug) is INERT: no
--     error, no overwrite, first lay wins. This is not new discipline, it's my
--     own discipline, scoped correctly.
--   - `ORDER BY witnessed, seq` on read — the clock orders the story; `seq` is
--     the tie-break / physical-append audit trail, never the ordering itself.
--   - lock-inside/persist-outside as a CALLING pattern (not expressed in DDL,
--     but named here so scher-stacks' Rust doesn't rediscover the race I
--     already found): don't hold a Society lock across an await point.
--
-- WHAT I AM NOT BRINGING, on purpose (crypto stays upstream — already ruled):
--
--   No content_ct/content_nonce/title_ct/title_nonce/enc_alg/beat_key_id here.
--   No keyring schema. Those are MY organs (penelope-gen4's), fused to a crypto
--   envelope this committee explicitly kept upstream, per-consumer. scher-
--   stacks stores EventRow-shaped content in the PLAIN, same as scher-core's
--   own in-memory shape — a consumer that wants encryption-at-rest layers it
--   on its OWN side of the wire, the way this repo's own crypto pass was
--   layered on top of the naive canon.beat table (db/migrations/0001, 0002)
--   without scher-core ever knowing ciphertext exists. That precedent is real:
--   it's how I'd advise doing it again, in a NEW envelope, not this one.
--
--   No hash-chained provenance module either (flagged in tonight's committee,
--   genuinely dependent on this code existing first — a later migration, not
--   this one).
--
-- FIX-LIST ITEM I OWE, NOT INHERIT (H1's non-blocking flag): the SELECT-then-
-- INSERT clock race on MY OWN clock-minting code (walk-plan B#6) is a wound of
-- re-minting from a global MAX. It doesn't recur here structurally, because
-- `witnessed` is no longer minted from a table scan at all — it's assigned by
-- the Society (scher-core) before the row is ever built, per tenant. But this
-- migration alone doesn't prove that; the conformance tests tonight's
-- committee asked for (persist-then-load round-trip, inert-lay through HTTP,
-- as_of replay correctness, PER TENANT) are where that proof has to live, in
-- scher-stacks' own fresh Rust, not inherited from mine.
-- ════════════════════════════════════════════════════════════════════════════

BEGIN;

CREATE SCHEMA IF NOT EXISTS stacks;

-- ─────────────────────────────────────────────────────────────────────────────
-- stacks.event — one table. EventRow's shape (scher-core/src/lib.rs), plus the
-- two columns that shape amputated or never needed: `witnessed` (mine, fixed)
-- and `stack` (the tenant boundary, new, refused-as-slug-prefix by every light).
--
-- node/edge/{slug}~q are ONE shape, distinguished by the null-pattern of
-- subject/object — exactly scher-core's own grammar, undisturbed.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS stacks.event (
    -- TENANCY — a real boundary around a whole Society. NOT a slug prefix, NOT
    -- a field scher-core's EventRow carries (the kernel light was explicit:
    -- tenancy is the server's concern, structural at the schema boundary).
    -- Text, not FK'd to a tenants table this pass — muslin-first scope; a
    -- registry can arrive later without moving this column.
    stack         text NOT NULL,

    -- ADDRESS — the human-readable slug grammar, opaque, never parsed for
    -- meaning by this table (bujo_policy-side conventions like {slug}~q live
    -- entirely above this schema). Unique PER TENANT, not globally.
    slug          text NOT NULL,

    -- STRUCTURAL — topology, plaintext pointers (not content). Both NULL means
    -- a node; both set means an edge. Same grammar as scher-core's EventRow
    -- and as my own canon.event before it.
    subject       text NULL,
    object        text NULL,

    -- CONTENT — plain, at rest. No ciphertext columns in this table: crypto
    -- stays upstream, per-consumer, per this evening's prior ruling.
    content       text NOT NULL DEFAULT '',
    title         text NULL,

    -- THE CLOCK — Society-assigned, persisted AS GIVEN. This is the one column
    -- whose absence in CanonRow I am naming as my own amputation. It is NOT
    -- optional, NOT nullable, NOT re-minted from MAX(witnessed) at write time.
    -- The caller (scher-stacks' write path, backed by an in-memory Society per
    -- request/session exactly as I am) assigns it before the INSERT is ever
    -- built, scoped to the tenant's OWN clock — one tenant's writes must never
    -- advance another tenant's frontier.
    witnessed     bigint NOT NULL,

    -- BOOKKEEPING — physical append order, per tenant. Tie-break under
    -- `witnessed`, never the ordering itself. (I used a bare bigserial,
    -- global across all rows; that's harmless here too since it's never
    -- compared ACROSS tenants for anything meaningful — but if that ever
    -- stops being true, scope it.)
    seq           bigserial NOT NULL,
    laid_at       timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT event_pk           PRIMARY KEY (stack, slug),
    CONSTRAINT event_edge_pairing CHECK ((subject IS NULL) = (object IS NULL))
);

-- The tenancy boundary, structural: no two tenants' rows can collide on slug,
-- and no query can accidentally scan cross-tenant without saying so (every
-- query below is written stack-first).
CREATE UNIQUE INDEX IF NOT EXISTS event_stack_slug_uq
    ON stacks.event (stack, slug);

-- Per-tenant clock ordering — the index this table's one real read pattern
-- (`ORDER BY witnessed, seq`) wants, scoped so one tenant's frontier can never
-- leak into another's `as_of` scan.
CREATE INDEX IF NOT EXISTS event_stack_witnessed_idx
    ON stacks.event (stack, witnessed, seq);

-- Structural lookups (prehensions_onto / interval_of), scoped per tenant —
-- this is the schema-level fix for the kernel light's second unsafe finding:
-- `soc.all()` must never scan across tenants, and an index that isn't
-- stack-scoped invites exactly that mistake later.
CREATE INDEX IF NOT EXISTS event_stack_subject_idx
    ON stacks.event (stack, subject) WHERE subject IS NOT NULL;
CREATE INDEX IF NOT EXISTS event_stack_object_idx
    ON stacks.event (stack, object) WHERE object IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- stacks.event_input — optional causal-parent edges (scher-core's grammar,
-- same shape I already carry as canon.event_input). Kept scoped by stack via
-- the FK's own (stack, slug) — a causal parent from another tenant is not
-- just wrong, it's structurally impossible to express.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS stacks.event_input (
    stack       text NOT NULL,
    slug        text NOT NULL,
    input_slug  text NOT NULL,   -- a causal parent's slug, same tenant (loose:
                                 -- may be laid in a later batch, no FK on the
                                 -- parent row's existence, by design)
    CONSTRAINT event_input_pk FOREIGN KEY (stack, slug)
        REFERENCES stacks.event (stack, slug),
    PRIMARY KEY (stack, slug, input_slug)
);

COMMIT;

-- ════════════════════════════════════════════════════════════════════════════
-- What this migration deliberately does NOT create, and why:
--
--   - No UPDATE-shaped trigger, no soft-delete column, no `deleted_at`. The
--     append-only law is structural: this schema has no column whose purpose
--     is to be mutated. Undo is occlusion — a new row (a {slug}~q occludes
--     edge, scher-core's own grammar) — never a write to an existing row.
--     The REST/MCP verb set built over this table must expose no PUT, no
--     PATCH, no DELETE — the schema doesn't forbid it (SQL always can UPDATE),
--     the SERVICE must, and that's ruled non-negotiable (H2).
--
--   - No content_ct/nonce/enc_alg/beat_key_id/keyring.* — crypto is not this
--     table's concern. If a consumer wants encryption-at-rest, it layers an
--     envelope on ITS side of the wire, the way my own crypto pass layered
--     onto canon.event without scher-core ever learning ciphertext exists.
--
--   - No hash-chained provenance columns — that module is real, named tonight,
--     and explicitly deferred until scher-stacks exists as running code to
--     attach it to. This is migration 0001, not the whole future.
-- ════════════════════════════════════════════════════════════════════════════
