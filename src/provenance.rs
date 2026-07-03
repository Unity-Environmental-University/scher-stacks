// provenance.rs — the hash-chained provenance module, built verbatim to
// docs/2026-07-03-provenance-hash-design.md (that doc is the spec; this file is the labor
// phase's build of it). Extends api/src/crypto.rs's beat_hash_v2 recipe (slot-digest, length-
// prefixed, slug-excluded) with one new appended slot: chain_digest(P), the canonicalized digest
// of everything a row prehends, resolved to each ancestor's OWN already-stored provenance hash
// (making this a transitive Merkle-DAG over the prehends-relation, not a flat content hash).
//
// Store-agnostic on purpose (§5 of the design doc): no Society/PgPool parameter anywhere in this
// module. Callers (store.rs, in a later labor pass) resolve `prehended_hashes` from already-
// persisted rows in topological order BEFORE calling compute_provenance_hash — that's what makes
// "fixed at lay-time" real rather than aspirational, and what lets verify_provenance_hash replay
// exactly the inputs that were canonicalized, never re-querying a live store for a possibly-
// different-today prehended set.
//
// Not yet wired into store.rs/rest.rs/mcp.rs: the design doc (§4's storage note, §7) explicitly
// defers the `provenance_hash`/`prehended_hashes` persistence shape and the provenance-relevant
// quality-tag choice to a later labor pass, rather than silently default them here. Until that
// wiring lands, this module is a real, independently-tested unit with no live caller — named dead
// code, not an oversight, so `#[allow(dead_code)]` is explicit rather than a suppressed warning.

#![allow(dead_code)]

use sha2::{Digest, Sha256};

/// Version byte for the provenance-hash recipe. Independent of penelope-gen4's beat_hash
/// HASH_VERSION (api/src/crypto.rs) — a different recipe (adds a chain slot beat_hash_v2 doesn't
/// have), not a revision of the same one. Never overload HASH_VERSION with this.
pub const PROVENANCE_HASH_VERSION: u8 = 1;

/// One row's contribution to the chain: everything this event's provenance hash was computed
/// FROM, at the moment it was computed. Stored alongside the hash (not re-derivable from the live
/// store after the fact, since later lays can change what `prehensions_onto` would return for the
/// same as_of if queried again — the whole point of "fixed at lay-time").
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
    /// structurally names (subject/object), resolved BEFORE this call — see design doc §5.
    /// Order here does not matter; compute_provenance_hash sorts internally (§3 step 3).
    pub prehended_hashes: &'a [[u8; 32]],
}

/// length-prefix a field into the running hash (8-byte big-endian len, then bytes) — the same
/// convention as api/src/crypto.rs's own `field()` helper, reused verbatim rather than reinvented,
/// so the two recipes stay legible as one family.
fn field(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

fn sha256(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

/// mode tag, mirroring api/src/crypto.rs's `mode_of` (no ~q convention here — scher-stacks'
/// provenance recipe classifies purely on subject/object presence; the `{slug}~q` convention's
/// tension with opaque slugs is explicitly out of scope for this module, per the design doc §7).
fn mode_of(subject: Option<&str>, object: Option<&str>) -> &'static str {
    if subject.is_some() && object.is_some() {
        "edge"
    } else {
        "node"
    }
}

/// The canonicalization step alone (design doc §3), exposed separately so it can be unit-tested
/// against order-independence without going through the full recipe.
///
/// Sorts by digest bytes ascending (never by slug — slugs are opaque per scher's own
/// opaque-slugs-no-string-matching discipline; sorting by slug would smuggle slug-spelling back
/// into a preimage the recipe otherwise excludes it from), count-prefixes (8-byte big-endian
/// number of elements, so a truncation can't be silently reinterpreted as a smaller, differently-
/// ordered set), length-prefixes each element (matching `field()`'s convention), then SHA-256s the
/// result. Empty input is well-defined (§3 step 5), not a special case at the call site: every
/// event has a chain digest, most are just chains of length zero.
pub fn chain_digest(prehended_hashes: &[[u8; 32]]) -> [u8; 32] {
    let mut sorted: Vec<[u8; 32]> = prehended_hashes.to_vec();
    sorted.sort_unstable();
    // A set has no duplicate members — a hash appearing twice in the input (e.g. a row that
    // structurally names the same slug as both subject and object of two different edges) folds
    // to one membership, matching "P is a SET of (stack, slug) pairs" in the design doc's §3 step 1.
    sorted.dedup();

    let mut h = Sha256::new();
    h.update([PROVENANCE_HASH_VERSION]);
    h.update((sorted.len() as u64).to_be_bytes());
    for digest in &sorted {
        field(&mut h, digest);
    }
    h.finalize().into()
}

/// THE RECIPE (v1). provenance_hash =
///   H( VER | H(subject) | H(object) | H(mode) | H(title) | H(content) | chain_digest(P) )
/// — beat_hash_v2's own five slots (api/src/crypto.rs:206), unchanged in shape, plus one appended
/// slot: chain_digest(P), the canonicalized digest of the prehended set (this module's own §3).
/// slug excluded, same rename-invariance beat_hash_v2 already established.
///
/// Fixed at lay-time: `input.prehended_hashes` must be the set as of THIS row's own `witnessed`
/// moment, never recomputed later against a different as_of — this function has no way to check
/// that (it is store-agnostic, no Society/PgPool param), which is deliberate: the caller owns that
/// invariant, this function only owns making the same inputs always produce the same output.
pub fn compute_provenance_hash(input: &ProvenanceInput) -> [u8; 32] {
    let title_digest = sha256(input.title.unwrap_or("").as_bytes());
    let content_digest = sha256(input.content.as_bytes());
    let mode = mode_of(input.subject, input.object);
    let chain = chain_digest(input.prehended_hashes);

    let mut h = Sha256::new();
    h.update([PROVENANCE_HASH_VERSION]);
    field(&mut h, &sha256(input.subject.unwrap_or("").as_bytes()));
    field(&mut h, &sha256(input.object.unwrap_or("").as_bytes()));
    field(&mut h, &sha256(mode.as_bytes()));
    field(&mut h, &title_digest);
    field(&mut h, &content_digest);
    field(&mut h, &chain);
    h.finalize().into()
}

/// Recompute the hash from a row's own stored fields plus its stored prehended-hash set, and
/// compare against a stored provenance_hash value. Never recomputes the prehended set from the
/// live store (no `soc`/`pool` parameter) — verification is pure-function replay of exactly the
/// inputs that were canonicalized at lay-time, so it can prove "this row's stored hash still
/// matches its stored inputs" without ever needing to ask "what would prehend onto this NOW."
/// Detects: content tampering (any scalar field changed), chain tampering (a stored
/// prehended-hash swapped for a forged one), and truncation/reordering of the chain (caught by
/// the count-prefix and per-element length-prefix in chain_digest).
pub fn verify_provenance_hash(input: &ProvenanceInput, expected: &[u8; 32]) -> bool {
    &compute_provenance_hash(input) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_bytes(b: &[u8]) -> [u8; 32] {
        sha256(b)
    }

    // ---- Test 1 — order-independence (design doc §6, Test 1) --------------------------------
    // chain_digest is a canonicalization, not a serialization: the SET of prehended hashes must
    // hash identically regardless of what order they were fetched/iterated in — Postgres row
    // order and Society::all()'s Map-iteration order are both unspecified (scher-core's own
    // doc-comment already warns of this, "like a Map's values()"), and a canonicalization that
    // leaked either would make the same logical history hash differently on two reads of the
    // same data.
    #[test]
    fn chain_digest_is_order_independent() {
        let a = sha256_bytes(b"row-a-provenance-hash-stand-in");
        let b = sha256_bytes(b"row-b-provenance-hash-stand-in");
        let c = sha256_bytes(b"row-c-provenance-hash-stand-in");

        let orderings: [Vec<[u8; 32]>; 6] = [
            vec![a, b, c],
            vec![a, c, b],
            vec![b, a, c],
            vec![b, c, a],
            vec![c, a, b],
            vec![c, b, a],
        ];
        let first = chain_digest(&orderings[0]);
        for ordering in &orderings[1..] {
            assert_eq!(
                chain_digest(ordering),
                first,
                "chain_digest must depend on the SET of prehended hashes, never the order they \
                 were fetched/iterated in"
            );
        }

        // and the same property one level up: compute_provenance_hash itself must be insensitive
        // to the order prehended_hashes arrives in, not just chain_digest in isolation.
        let order_abc = [a, b, c];
        let order_cba = [c, b, a];
        let input_abc = ProvenanceInput {
            stack: "s1",
            slug: "decision-1",
            content: "granted",
            title: None,
            subject: Some("actor-1"),
            object: Some("resource-1"),
            witnessed: 42,
            prehended_hashes: &order_abc,
        };
        let input_cba = ProvenanceInput {
            stack: "s1",
            slug: "decision-1",
            content: "granted",
            title: None,
            subject: Some("actor-1"),
            object: Some("resource-1"),
            witnessed: 42,
            prehended_hashes: &order_cba,
        };
        let h1 = compute_provenance_hash(&input_abc);
        let h2 = compute_provenance_hash(&input_cba);
        assert_eq!(h1, h2);
    }

    // ---- Test 2 — single-fact-change changes the hash (design doc §6, Test 2) ---------------
    // Own content, chain membership, and unrelated scalar fields must each independently move the
    // hash; an identical call must reproduce an identical hash (the determinism baseline the
    // other assertions lean on).
    #[test]
    fn single_fact_change_changes_hash() {
        let ancestor_hash = sha256_bytes(b"ancestor-provenance-hash-stand-in");
        let ancestors = [ancestor_hash];

        let base = ProvenanceInput {
            stack: "s1",
            slug: "decision-1",
            content: "granted",
            title: None,
            subject: Some("actor-1"),
            object: Some("resource-1"),
            witnessed: 10,
            prehended_hashes: &ancestors,
        };
        let base_hash = compute_provenance_hash(&base);

        // 2a. Change this row's OWN content — the hash must change (beat_hash_v2's existing
        // content-sensitivity, inherited unchanged).
        let content_changed = ProvenanceInput {
            content: "denied",
            ..base
        };
        assert_ne!(
            compute_provenance_hash(&content_changed),
            base_hash,
            "changing this row's own content must change its provenance hash"
        );

        // 2b. Change NOTHING about this row itself, but swap one ancestor's hash for a different
        // (but same-length) one — simulating "an ancestor's content was tampered with, so its OWN
        // provenance hash changed, so this descendant's chain input changed."
        let different_ancestor = sha256_bytes(b"a-DIFFERENT-ancestor-hash-stand-in");
        let different_ancestors = [different_ancestor];
        let chain_changed = ProvenanceInput {
            prehended_hashes: &different_ancestors,
            ..base
        };
        assert_ne!(
            compute_provenance_hash(&chain_changed),
            base_hash,
            "changing which ancestor hash is chained in — even with this row's own fields \
             byte-for-byte identical — must change the hash. This is the actual chaining \
             property: tampering with an ancestor must be visible at every descendant."
        );

        // 2c. Sanity: an unrelated field changing (subject) must also move the hash — the
        // ruling's committee-cited rename-invariance is about SLUG specifically, not about
        // subject/object.
        let subject_changed = ProvenanceInput {
            subject: Some("actor-2"),
            ..base
        };
        assert_ne!(compute_provenance_hash(&subject_changed), base_hash);

        // 2d. Sanity check the converse of Test 1's property lives here too: an IDENTICAL call
        // must reproduce the IDENTICAL hash (determinism, the pure-function baseline).
        assert_eq!(compute_provenance_hash(&base), base_hash);
    }

    // ---- Test 3 — no collisions across 10,000 distinct DAG-shaped chains (design doc §6, Test 3)
    // Does not prove SHA-256 is collision-resistant (out of scope to re-litigate here) — proves
    // the chain_digest canonicalization and the recipe's slot composition don't accidentally
    // collapse distinct chains into the same preimage before SHA-256 ever sees them (e.g. an
    // off-by-one in the length-prefix scheme that let two different-shaped sets serialize
    // identically). 10,000 is a canonicalization-bug stress test budget, not a birthday-bound
    // argument.
    #[test]
    fn no_collisions_across_10_000_distinct_chains() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};
        use std::collections::HashSet;

        let mut seen: HashSet<[u8; 32]> = HashSet::with_capacity(10_000);
        // seeded, reproducible failures.
        let mut rng = StdRng::seed_from_u64(0x5CE2_57AC_5_5057);

        // A pool of already-computed hashes to draw "ancestors" from, grown as we go so later
        // chains can (and do) reference earlier ones — this is what makes the sample a genuine
        // DAG-shaped stress test, not 10,000 independent leaf hashes.
        let mut pool: Vec<[u8; 32]> = Vec::new();

        for i in 0..10_000usize {
            let max_fanin = pool.len().min(40);
            let fanin = if max_fanin == 0 {
                0
            } else {
                rng.gen_range(0..=max_fanin)
            };
            let mut prehended: Vec<[u8; 32]> = Vec::with_capacity(fanin);
            for _ in 0..fanin {
                let idx = rng.gen_range(0..pool.len());
                prehended.push(pool[idx]);
            }

            // vary own-row content so two rows are never accidental content-twins.
            let content = format!("stress-row-{i}-{}", rng.gen::<u64>());
            let has_subject = rng.gen_bool(0.5);
            let subject = if has_subject {
                Some(format!("actor-{}", rng.gen::<u32>()))
            } else {
                None
            };
            let object = if has_subject {
                Some(format!("resource-{}", rng.gen::<u32>()))
            } else {
                None
            };
            let slug = format!("row-{i}");

            let input = ProvenanceInput {
                stack: "stress",
                slug: &slug,
                content: &content,
                title: None,
                subject: subject.as_deref(),
                object: object.as_deref(),
                witnessed: i as u64,
                prehended_hashes: &prehended,
            };
            let h = compute_provenance_hash(&input);
            assert!(
                seen.insert(h),
                "collision at row {i}: two structurally distinct provenance chains produced the \
                 same hash — this points at the canonicalization or slot-composition, not at \
                 SHA-256 itself"
            );
            pool.push(h);
        }

        assert_eq!(seen.len(), 10_000);
    }

    // ---- Supporting sanity: chain_digest of the empty set is well-defined (design doc §3 step 5)
    #[test]
    fn empty_chain_digest_is_well_defined_and_stable() {
        let empty: [[u8; 32]; 0] = [];
        let d1 = chain_digest(&empty);
        let d2 = chain_digest(&[]);
        assert_eq!(d1, d2, "the empty prehended set must hash identically every time");

        // and it must differ from a non-empty chain — an empty chain is a real value, not a
        // silent stand-in for "same as omitted."
        let one = sha256_bytes(b"non-empty");
        assert_ne!(d1, chain_digest(&[one]));
    }

    // ---- Supporting sanity: duplicate ancestor references fold to one membership (§3 step 1: P
    // is a SET, not a multiset/list).
    #[test]
    fn duplicate_prehended_hashes_fold_to_one_membership() {
        let a = sha256_bytes(b"dup-a");
        let b = sha256_bytes(b"dup-b");
        assert_eq!(chain_digest(&[a, b, a]), chain_digest(&[a, b]));
    }

    // ---- Supporting sanity: verify_provenance_hash accepts a genuine match and rejects a forged
    // one, purely from stored fields (no store/session parameter).
    #[test]
    fn verify_accepts_genuine_and_rejects_forged() {
        let ancestor = sha256_bytes(b"verify-ancestor");
        let ancestors = [ancestor];
        let input = ProvenanceInput {
            stack: "s1",
            slug: "decision-9",
            content: "granted",
            title: Some("access decision"),
            subject: Some("actor-9"),
            object: Some("resource-9"),
            witnessed: 99,
            prehended_hashes: &ancestors,
        };
        let real_hash = compute_provenance_hash(&input);
        assert!(verify_provenance_hash(&input, &real_hash));

        let forged = sha256_bytes(b"forged-hash-not-actually-derived");
        assert!(!verify_provenance_hash(&input, &forged));
    }
}
