# Releasing scher-stacks

## Cut a release

1. Bump `version` in `Cargo.toml` (and `Cargo.lock` via `cargo check`), merge to `main`.
2. Tag and push:
   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```
   The release workflow fails if the tag and `Cargo.toml` version disagree. A tag with a
   hyphen (`v0.2.0-rc1`) is published as a prerelease and does not move `latest`.
3. The workflow produces:
   - `scher-stacks-<tag>-linux-x86_64.tar.gz` and `...-linux-arm64.tar.gz`
   - `scher-stacks-<tag>-migrations.tar.gz`
   - `SHA256SUMS` over all three
   - `ghcr.io/unity-environmental-university/scher-stacks:<tag>` (amd64 + arm64)
   - a GitHub Release with generated notes

Binaries are built on Debian bookworm, so they run on the `debian:bookworm-slim` image
and on any glibc at least that new.

## Upgrading: migrations

The binary runs `sqlx::migrate!("./migrations")` at startup, so starting the new version
applies any new migrations. Migrations are forward-only. To apply them first, or to
review them, unpack the migrations tarball and run the files in order with `psql`:

```bash
for f in migrations/*.sql; do psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$f"; done
```

(That loop is for a fresh database. On an existing one, apply only files newer than what
`_sqlx_migrations` records, or just let the service do it.) Back up before upgrading;
there is no down migration.

## Proposal: pin scher-core by tag, not branch

`Cargo.toml` depends on scher-core by `branch = "frame-is-not-a-user"`. `Cargo.lock`
records the commit, so `--locked` builds are reproducible, but any `cargo update`
silently moves the kernel. For releases, prefer a tag on the scher repo:

```toml
scher-core = { git = "https://github.com/Unity-Environmental-University/scher.git", tag = "v0.3.0" }
```

Not changed in this PR: it needs a decision about which scher tag contains the commit
currently locked, and penelope-course's `deploy/prepare-context.sh` pins the branch
(`SCHER_CORE_REF=origin/frame-is-not-a-user`), so the two should move together.

## penelope-course: pull an image instead of vendoring

Today `deploy/prepare-context.sh` copies scher-stacks and scher-core into `deploy/vendor/`
and `stacks.Dockerfile` compiles them. With a released image, the stacks service needs
no build at all. In the compose file, replace the `build:` block with:

```yaml
stacks:
  image: ghcr.io/unity-environmental-university/scher-stacks:v0.2.0   # pin the tag
  environment:
    DATABASE_URL: ...
```

Then `stacks.Dockerfile` and the scher-stacks and scher-core halves of
`prepare-context.sh` can go (the npm `scher` and `scher-core-wasm` copies are separate).
Differences to check: the release image has no `curl`, so a `curl`-based healthcheck must
become a Postgres/TCP check or be run from another container; it runs as uid 10001.

## One-time repo settings

- After the first image push, set the `scher-stacks` package visibility (Packages) to
  public, or grant the deploy host a token with `read:packages`.
- Settings > Actions > General: workflow permissions can stay read-only; the workflows
  request `packages: write` and `contents: write` themselves.
