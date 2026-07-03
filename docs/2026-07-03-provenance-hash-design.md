# Provenance-hash module design (file-light, self-ministry, 2026-07-03)

Built to `penelope-gen4/docs/committees/2026-07-03-scher-stacks-design.md` — that ruling is not
relitigated here. That ruling's own Blocked/deferred section is what assigns this ministry: a
hash-chained provenance module, genuinely dependent on scher-stacks existing as real code first,
that should extend penelope-gen4's `beat_hash_v2`/`beat_hash_v2_for` pattern
(`api/src/crypto.rs`, `api/src/main.rs`) rather than invent a new scheme, and that needs an
explicit canonicalization rule for hashing a whole *prehended set* — designed before implemented,
not discovered as a bug later. This doc is that design. It is the spec the labor phase builds
verbatim; it does not implement anything itself.

## 0. What this module is for, in one sentence

Every laid event gets a hash covering not just its own content but its full because-chain of
evidence and prior state, so that a recorded decision (a permission grant, a bujo capture,
anything laid through scher-stacks) cannot be quietly altered or backdated without the hash
chain revealing it.

## 1. The design tension, and the working default that resolves it

A cryptographic hash needs one canonical, fixed input. But what an event "prehends" can be read
differently by different frames — `as-of-then` (what was visible at lay-time) vs `as-of-now`
(what the current frame considers connected, including edges laid later that reach back onto an
older event as object). `prehensions_onto(soc, row, quality, as_of)` takes an `as_of` parameter
for exactly this reason — scher's own grammar already models that a "prehended set" is
frame-relative.

**The working default (per the ruling, not yet built, not yet re-litigated here):** the hash
closes over the provenance chain **as witnessed at lay-time**, fixed and immutable once
recorded. Frame-relativity then lives entirely in how that fixed history is later interpreted or
displayed — never in the hash computation itself. Concretely: `compute_provenance_hash` is called
once, at the moment a row is laid, with `as_of = this row's own witnessed value` (i.e., "what
existed in this stack up to and including the moment this row lands"). It is never recomputed
later against a different `as_of`. A later frame that wants to read the chain differently reads
the *stored, fixed* hash and the *stored, fixed* prehended-set snapshot that produced it — it
does not ask for a new hash.

This closes the tension by moving it out of the hash and into the read layer, exactly as the
ruling's Blocked/deferred item specifies.

## 2. Relationship to `beat_hash_v2`

`beat_hash_v2` (api/src/crypto.rs:206) hashes ONE row's own fields — a fixed set of scalar
slots (subject, object, mode, title digest, content digest), each entered as a fixed-size
32-byte digest, length-prefixed, slug excluded for rename-invariance. It has no notion of a
*chain* — it authenticates a beat's own content, not its ancestry.

The provenance hash extends this, it does not replace it:

- **Slot recipe reused verbatim.** The provenance hash's own-row contribution uses
  `beat_hash_v2`'s existing digest-slot approach unchanged (subject/object/mode/title/content
  digests, slug excluded, length-prefixed fields, `HASH_VERSION`-prefixed). scher-stacks has no
  crypto organs of its own (ciphertext, keyring) — plaintext columns per the scaffold's
  `0001_event.sql` — so where `beat_hash_v2_for` digests `content_ct`, scher-stacks digests plain
  `content` directly. This is the "same recipe, ciphertext↔plaintext inner-digest swap" move the
  ruling already names as unchanged-across-cutover precedent.
- **New: a chain slot.** One additional length-prefixed field is appended to the recipe: the
  **canonicalized digest of the prehended set** (§3below) — the hash of everything this event's
  own hash closes over, one level deep. Because that digest is itself built from each prehended
  row's *own* provenance hash (not its raw content), the chain is transitively closed: changing
  any ancestor changes that ancestor's provenance hash, which changes the canonicalized-set
  digest of everything that (transitively) prehends it, which changes every descendant's
  provenance hash. This is the actual "chain" in hash-chained provenance — it is a Merkle-DAG
  over the prehends-relation, not a linear chain.
- **Versioning.** A new `PROVENANCE_HASH_VERSION: u8` constant, independent of `HASH_VERSION`
  (crypto.rs's existing beat-hash version byte) — this is a different recipe with a different
  input shape (it includes a chain slot `beat_hash_v2` does not have), not a revision of the same
  one. Do not overload `HASH_VERSION`.

## 3. Canonicalization rule for the prehended set

This is the piece that does not exist yet and is the actual point of this design doc.

**What counts as "prehended," fixed at lay-time:** for a row `r` being laid at witnessed moment
`w` into stack `s`, the prehended set is:

```
P(r, w) = prehensions_onto(soc_s, r.slug, quality, as_of = w)
          for every quality this store tracks as provenance-relevant,
      ∪  { the row(s) r itself names structurally: r.subject, r.object, if present }
```

(The exact quality tag(s) — e.g. a `Q_GROUNDING`-style constant scoped to provenance, distinct
from scher-core's existing `Q_GROUNDING`/`Q_EXCLUSION`/`Q_DEPENDS_ON` — is a store.rs-level
config, not fixed by this doc; what this doc fixes is the *canonicalization*, not *which*
qualities are chained. Named here so the labor phase doesn't have to invent it silently.)

**The canonicalization rule itself** (the part that must be exact, since a hash's preimage must
be exact):

1. Collect the prehended set `P` as a set of `(stack, slug)` pairs — never raw content, never
   `EventRow` structs. A pair, not a bare slug, because provenance never crosses tenancy (an edge
   in one stack can never prehend a row in another).
2. For each `(stack, slug)` in `P`, resolve to that row's own **already-computed, already-stored**
   `provenance_hash` (not its content digest, not its `beat_hash_v2` — its provenance hash, so the
   chain is transitive as described in §2). A row with no stored provenance hash yet (shouldn't
   happen if hashes are computed at lay-time in dependency order — see §5 ordering note) is a hard
   error, never silently treated as absent.
3. **Sort the set before hashing — the fix for "hash needs one canonical input, prehension is a
   set."** Sort by the raw bytes of each row's own 32-byte provenance-hash digest, ascending,
   unsigned byte-wise comparison. (Not by `slug` — slug is opaque per scher's own
   opaque-slugs-no-string-matching discipline, and sorting by slug string would smuggle the slug
   back into a preimage that `beat_hash_v2`'s own recipe already went out of its way to exclude.
   Sorting by the digest itself is content-derived, not identity-derived, and is exactly as stable
   as the digests it sorts.)
4. Concatenate the sorted digests with an 8-byte big-endian count prefix (how many elements — so
   a stray truncation can't be silently reinterpreted as a smaller, differently-ordered set) and
   an 8-byte big-endian length prefix per element (matching the existing `field()` helper's
   length-prefix convention in crypto.rs, reused verbatim rather than reinvented), then SHA-256 the
   result. This is the "canonicalized digest of the prehended set" referenced in §2 — call it
   `chain_digest(P)`.
5. An empty prehended set (a bare node, no subject/object, no incoming provenance-tagged edges) is
   not a special case: `chain_digest(∅)` is simply the hash of the count-prefix `0u64` and nothing
   else — a fixed, well-defined value, not `None` and not a different code path. Every event has a
   chain digest; most are just chains of length zero.

This gives one canonical, deterministic, order-independent input for any prehended set,
regardless of what order rows were fetched from Postgres or iterated from `soc.all()` (which the
kernel's own doc-comment already warns is unspecified, "like a Map's values()").

## 4. Function signatures

Location: new module `src/provenance.rs` in scher-stacks (not scher-core — this is a
scher-stacks module per the ruling: "genuinely dependent on scher-stacks existing as real code
first ... a module *of* scher-stacks"). Depends on `scher_core::EventRow` and reuses
`api/src/crypto.rs`'s `sha256`/`field` pattern (copied in, not path-dep'd — scher-stacks has no
dependency on penelope-gen4's `api` crate; the *pattern* is what's reused, not the module).

```rust
/// Version byte for the provenance-hash recipe. Independent of penelope-gen4's beat_hash
/// HASH_VERSION — a different recipe (adds a chain slot beat_hash_v2 doesn't have), not a
/// revision of the same one.
pub const PROVENANCE_HASH_VERSION: u8 = 1;

/// One row's contribution to the chain: everything this event's provenance hash was computed
/// FROM, at the moment it was computed. Stored alongside the hash (not re-derivable from the
/// live store after the fact, since later lays can change what `prehensions_onto` would return
/// for the same as_of if queried again — the whole point of "fixed at lay-time").
pub struct ProvenanceInput<'a> {
    pub stack: &'a str,
    pub slug: &'a str,
    pub content: &'a str,
    pub title: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub object: Option<&'a str>,
    /// The moment this row was witnessed — the as_of the prehended set was fixed against.
    pub witnessed: u64,
    /// Already-computed provenance hashes of every (stack, slug) this row prehends onto or
    /// structurally names (subject/object), resolved BEFORE this call — see §5 ordering note.
    /// Order here does not matter; compute_provenance_hash sorts internally (§3 step 3).
    pub prehended_hashes: &'a [[u8; 32]],
}

/// THE RECIPE (v1). provenance_hash =
///   H( VER | H(subject) | H(object) | H(mode) | H(title_digest) | H(content_digest) | chain_digest(P) )
/// — beat_hash_v2's own five slots, unchanged, plus one appended slot: chain_digest(P), the
/// canonicalized digest of the prehended set (§3). slug excluded, same rename-invariance
/// beat_hash_v2 already established. Fixed at lay-time: `prehended_hashes` must be the set as
/// of THIS row's own `witnessed` moment, never recomputed later against a different as_of.
pub fn compute_provenance_hash(input: &ProvenanceInput) -> [u8; 32];

/// Recompute the hash from a row's own stored fields plus its stored prehended-hash set, and
/// compare against a stored provenance_hash value. Never recomputes the prehended set from the
/// live store (no `soc`/`pool` parameter) — verification is pure-function replay of exactly the
/// inputs that were canonicalized at lay-time, so it can prove "this row's stored hash still
/// matches its stored inputs" without ever needing to ask "what would prehend onto this NOW."
/// Detects: content tampering (any scalar field changed), chain tampering (a stored
/// prehended-hash swapped for a forged one), and truncation/reordering of the chain (caught by
/// the count-prefix and per-element length-prefix in chain_digest).
pub fn verify_provenance_hash(input: &ProvenanceInput, expected: &[u8; 32]) -> bool;

/// The canonicalization step alone (§3), exposed separately so it can be unit-tested against
/// order-independence without going through the full recipe. Sorts by digest bytes ascending,
/// count-prefixes, length-prefixes each element, SHA-256s the result. Empty input is well-defined
/// (§3 step 5), not a special case at the call site.
pub fn chain_digest(prehended_hashes: &[[u8; 32]]) -> [u8; 32];
```

Storage note (not a function signature, but load-bearing for §5): `stacks.event` gains one
column in a *later* migration (not `0001` — the scaffold doc already names this as deferred, not
smuggled into the base schema): `provenance_hash BYTEA NOT NULL` and, per the "store the inputs,
don't re-derive them" note above, either a `prehended_slugs TEXT[] NOT NULL DEFAULT '{}'`
(stack-scoped, resolved to hashes at read time) or a denormalized `prehended_hashes BYTEA[]`
directly. Which of those two is out of scope for this doc (a storage-shape decision, not a
hash-design decision) — named so the labor phase doesn't have to invent it silently, same as the
quality-tag note in §3.

## 5. Ordering note (why `prehended_hashes` must be pre-resolved, not computed inside the function)

A row's provenance hash depends on its ancestors' provenance hashes (§2, §3 step 2). This means
hashes must be computed in an order where every ancestor is hashed before its descendant — a
topological order over the prehends-DAG, not insertion order or `witnessed` order in general
(though in the common case, an edge can only prehend onto a row that already exists, so
`witnessed`-order lay is already topological in practice; the pure functions above take
pre-resolved hashes precisely so they don't have to assume that and don't have to walk the store
themselves). `compute_provenance_hash` is deliberately store-agnostic (no `Society`/`PgPool`
parameter) so it can't accidentally re-derive a *different* prehended set than the one that was
actually fixed at lay-time — the caller (store.rs, in the labor phase) is responsible for
resolving `prehended_hashes` from already-persisted rows before calling it, which is also what
naturally enforces the topological constraint (you cannot resolve a hash for a row that hasn't
been laid, and hasn't been laid yet).

## 6. The three test cases that MUST exist and pass

These are the spec's binding content — the labor phase builds these verbatim, in
`tests/provenance_*.rs` or `src/provenance.rs`'s own `#[cfg(test)] mod tests`, colocated with
scher-stacks' existing conformance-test family.

### Test 1 — order-independence (`chain_digest` is a canonicalization, not a serialization)

```rust
#[test]
fn chain_digest_is_order_independent() {
    // three arbitrary but fixed 32-byte digests, standing in for three prehended rows'
    // already-computed provenance hashes.
    let a: [u8; 32] = sha256_bytes(b"row-a-provenance-hash-stand-in");
    let b: [u8; 32] = sha256_bytes(b"row-b-provenance-hash-stand-in");
    let c: [u8; 32] = sha256_bytes(b"row-c-provenance-hash-stand-in");

    let orderings = [
        vec![a, b, c], vec![a, c, b], vec![b, a, c],
        vec![b, c, a], vec![c, a, b], vec![c, b, a],
    ];
    let first = chain_digest(&orderings[0]);
    for ordering in &orderings[1..] {
        assert_eq!(chain_digest(ordering), first,
            "chain_digest must depend on the SET of prehended hashes, never the order they were \
             fetched/iterated in — Postgres row order and Society::all()'s Map-iteration order \
             are both unspecified, and a canonicalization that leaked either would make the same \
             logical history hash differently on two reads of the same data");
    }

    // and the same property one level up: compute_provenance_hash itself must be insensitive to
    // the order prehended_hashes arrives in, not just chain_digest in isolation.
    let base_input = |order: &[[u8; 32]]| ProvenanceInput {
        stack: "s1", slug: "decision-1", content: "granted", title: None,
        subject: Some("actor-1"), object: Some("resource-1"), witnessed: 42,
        prehended_hashes: order,
    };
    let h1 = compute_provenance_hash(&base_input(&[a, b, c]));
    let h2 = compute_provenance_hash(&base_input(&[c, b, a]));
    assert_eq!(h1, h2);
}
```

### Test 2 — single-fact-change changes the hash (own content AND chain, both directions)

```rust
#[test]
fn single_fact_change_changes_hash() {
    let ancestor_hash: [u8; 32] = sha256_bytes(b"ancestor-provenance-hash-stand-in");

    let base = ProvenanceInput {
        stack: "s1", slug: "decision-1", content: "granted", title: None,
        subject: Some("actor-1"), object: Some("resource-1"), witnessed: 10,
        prehended_hashes: &[ancestor_hash],
    };
    let base_hash = compute_provenance_hash(&base);

    // 2a. Change this row's OWN content — the hash must change (beat_hash_v2's existing
    // content-sensitivity, inherited unchanged).
    let content_changed = ProvenanceInput { content: "denied", ..base };
    // NB: struct-update syntax shown for brevity; the real test constructs each variant fully
    // since ProvenanceInput borrows &str fields.
    assert_ne!(compute_provenance_hash(&content_changed), base_hash,
        "changing this row's own content must change its provenance hash — this is beat_hash_v2's \
         existing guarantee and must survive unchanged inside the larger recipe");

    // 2b. Change NOTHING about this row itself, but swap one ancestor's hash for a different
    // (but same-length) one — simulating "an ancestor's content was tampered with, so its OWN
    // provenance hash changed, so this descendant's chain input changed."
    let different_ancestor: [u8; 32] = sha256_bytes(b"a-DIFFERENT-ancestor-hash-stand-in");
    let chain_changed = ProvenanceInput { prehended_hashes: &[different_ancestor], ..base };
    assert_ne!(compute_provenance_hash(&chain_changed), base_hash,
        "changing which ancestor hash is chained in — even with this row's own fields byte-for- \
         byte identical — must change the hash. This is the actual chaining property: tampering \
         with an ancestor (which changes THAT ancestor's stored provenance hash) must be visible \
         at every descendant, not just swallowed at the point of tampering.");

    // 2c. Sanity: an unrelated field changing (subject) must also move the hash — the ruling's
    // committee-cited rename-invariance is about SLUG specifically, not about subject/object.
    let subject_changed = ProvenanceInput { subject: Some("actor-2"), ..base };
    assert_ne!(compute_provenance_hash(&subject_changed), base_hash);

    // 2d. Sanity check the converse of Test 1's property lives here too: an IDENTICAL call
    // must reproduce the IDENTICAL hash (determinism, the pure-function baseline the other
    // assertions all lean on).
    assert_eq!(compute_provenance_hash(&base), base_hash);
}
```

### Test 3 — no collisions across a stress-tested sample (10,000 distinct chains)

Sample size: **10,000** distinct, structurally-varied provenance chains (varying own-row content,
chain depth from 0 to a few hundred ancestors, and chain width from 0 to a few dozen siblings per
level), asserting zero collisions in the resulting hash set. This does not prove SHA-256 is
collision-resistant (it is, and that is out of scope to re-litigate here) — it proves the
*chain_digest canonicalization and the recipe's slot composition* don't accidentally collapse
distinct chains into the same preimage before SHA-256 ever sees them (e.g., an off-by-one in the
length-prefix scheme that let two different-shaped sets serialize identically).

```rust
#[test]
fn no_collisions_across_10_000_distinct_chains() {
    use std::collections::HashSet;
    let mut seen: HashSet<[u8; 32]> = HashSet::with_capacity(10_000);
    let mut rng = deterministic_rng(0xSCHER_STACKS_PROVENANCE); // seeded, reproducible failures

    // A pool of already-computed hashes to draw "ancestors" from, grown as we go so later
    // chains can (and do) reference earlier ones — this is what makes the sample a genuine
    // DAG-shaped stress test, not 10,000 independent leaf hashes.
    let mut pool: Vec<[u8; 32]> = Vec::new();

    for i in 0..10_000usize {
        let depth_fanin = rng.gen_range(0..=(pool.len().min(40))); // chain width this row draws
        let mut prehended: Vec<[u8; 32]> = Vec::with_capacity(depth_fanin);
        for _ in 0..depth_fanin {
            let idx = rng.gen_range(0..pool.len());
            prehended.push(pool[idx]);
        }

        // vary own-row content so two rows are never accidental content-twins.
        let content = format!("stress-row-{i}-{}", rng.gen::<u64>());
        let subject = if rng.gen_bool(0.5) { Some(format!("actor-{}", rng.gen::<u32>())) } else { None };
        let object  = if subject.is_some() { Some(format!("resource-{}", rng.gen::<u32>())) } else { None };

        let input = ProvenanceInput {
            stack: "stress", slug: &format!("row-{i}"), content: &content, title: None,
            subject: subject.as_deref(), object: object.as_deref(),
            witnessed: i as u64, prehended_hashes: &prehended,
        };
        let h = compute_provenance_hash(&input);
        assert!(seen.insert(h),
            "collision at row {i}: two structurally distinct provenance chains produced the same \
             hash — this points at the canonicalization or slot-composition, not at SHA-256 itself");
        pool.push(h);
    }

    assert_eq!(seen.len(), 10_000);
}
```

Sample-size note: 10,000 is chosen as large enough to be a meaningful stress test of the
canonicalization/composition logic (the thing actually at risk of a bug) within a fast unit-test
budget, not as a cryptographic collision-resistance argument (which would require a sample size
no test suite could run). If a future pass wants a genuine birthday-bound-adjacent stress test,
that is a separate, explicitly-labeled slow/ignored test, not a change to this one's budget.

## 7. What this doc explicitly does not do

- Does not implement `src/provenance.rs` — signatures and doc-comment-as-contract only, per this
  ministry's own charge.
- Does not decide the storage shape for `prehended_hashes`/`prehended_slugs` at rest (§4's storage
  note) — flagged, not resolved, same discipline as the scaffold doc's own H2 flag on `{slug}~q`.
- Does not pick which quality tag(s) gate "provenance-relevant" prehension (§3) — flagged as a
  store.rs-level config the labor phase must name explicitly, not silently default.
- Does not resolve the `{slug}~q` convention's tension with opaque slugs — inherited as-is,
  unrelated to this module's own design tension.
- Does not migrate penelope-gen4's own `beat_hash_v2` callers onto this recipe — the ruling scopes
  this to scher-stacks; whether/when penelope-gen4 itself adopts a chained provenance hash is a
  separate, later, undecided question, same shape as its own deferred migration-onto-scher-stacks
  question.

## Links and citations

- `penelope-gen4/docs/committees/2026-07-03-scher-stacks-design.md` — Blocked/deferred, last item:
  this module's charge, the working default on frame-relativity, the extend-not-invent directive.
- `scher-stacks/docs/2026-07-03-scaffold-design.md` — the schema/store this module attaches to;
  §3's note on `beat_hash` column deferral; §7's note that no hash-chained provenance module/column
  ships in `0001`.
- `api/src/crypto.rs:206` (`beat_hash_v2`), `:224` (`field`), `:232` (`sha256`) — the recipe and
  helpers this module extends verbatim rather than reinventing.
- `api/src/main.rs:814` (`beat_hash_v2_for`) — the ciphertext-inner-digest precedent this module's
  plaintext-inner-digest mirrors (same recipe, same swap-point, no crypto organs added).
- `scher/scher-core/src/lib.rs:213` (`prehensions_onto`), `:192` (`visible_at`), `:107`
  (`Society::insert`'s clock ratchet) — the `as_of`-relative read this module fixes at lay-time
  rather than inheriting frame-relativity into the hash itself.
- `scher/CLAUDE.md` — opaque-slugs-no-string-matching, the reason `chain_digest` sorts by digest
  bytes, never by slug.
- `lingit-verb-assembler/packages/answer-state-evaluator` — the originating thread (a permissions/
  access model for a Lingít verb assembler prototype) this module was recognized as belonging to
  scher-stacks instead of, per the ruling's own account of where the idea surfaced.
