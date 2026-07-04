# scher-stacks

A standing REST + MCP service over a Postgres-backed [scher-core](https://github.com/Unity-Environmental-University/scher) Society — one Society per tenant ("stack"), **append-only on the wire**: no PUT, PATCH, or DELETE routes exist structurally. Events are laid, never overwritten; undo is occlusion, never deletion; every laid event's hash closes over its because-chain (see `docs/2026-07-03-provenance-hash-design.md`).

Built 2026-07-03 by a committee process (design minutes in `docs/`); serves as the live event store for the [LANGUAGE] Verb Assembler muslin and other scher consumers.

## Running it

Prerequisites: Rust (stable), PostgreSQL.

```bash
createdb scher_stacks                     # or point DATABASE_URL anywhere
export DATABASE_URL=postgres://localhost/scher_stacks   # default shown
export BIND_ADDR=0.0.0.0:8080                            # default shown
cargo run                                 # migrations run on startup
cargo test                                # 6 integration tests (need Postgres)
```

## The wire

- `POST /stacks/:stack/events` — lay an event `{slug, content, title?, subject?, object?}`. Laying an existing slug is inert (append-only law). `witnessed` is assigned by the Society's own clock, never client-supplied.
- `GET /stacks/:stack/events` — the stack's events in witnessed order.

An edge (subject+object both present) must have both ends already laid (`event_edge_pairing`) — an edge with one end isn't an edge.

## Dependency shape

`scher-core` is resolved as a git dependency on the scher repo (the Rust kernel lives inside it at `scher-core/`). For local development against a checkout, use a `[patch]` section or a temporary path override in `Cargo.toml`.

## License

MIT, copyright (c) 2026 Unity Environmental University — see `LICENSE`.
