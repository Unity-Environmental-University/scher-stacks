# Releasing scher-stacks

A release is a pushed `v*` tag. [`.github/workflows/release.yml`](.github/workflows/release.yml) does the rest:

| Job | What it does |
|---|---|
| `verify` | Fails unless the tag equals `Cargo.toml`'s version (`v0.2.0` ↔ `version = "0.2.0"`). |
| `binaries` | `cargo build --release --locked` natively on x86_64 and arm64 runners (Ubuntu 22.04, so glibc ≥ 2.35 — runs on Debian bookworm and newer). |
| `image` | Copies those same binaries into [`release.Dockerfile`](release.Dockerfile) and pushes a multi-arch image to `ghcr.io/unity-environmental-university/scher-stacks:<tag>` (also `:<version>` and, for non-prerelease tags, `:latest`). |
| `release` | Creates the GitHub Release with generated notes and attaches the files listed below. |

Files attached to each release:

```
scher-stacks-v0.2.0-x86_64-unknown-linux-gnu.tar.gz   binary + README + LICENSE
scher-stacks-v0.2.0-aarch64-unknown-linux-gnu.tar.gz
scher-stacks-v0.2.0-migrations.tar.gz                 the migrations/ folder at that tag
SHA256SUMS
```

Caching: `Swatinem/rust-cache` caches compiled dependencies per target, and the image build uses the GitHub Actions layer cache. Nothing gets compiled inside Docker.

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs on every push and PR: `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`. The test job runs against a Postgres 16 service container. It first boots the binary once, which applies every migration to an empty database, then checks that the count in `_sqlx_migrations` matches the number of files.

## Cutting a release

1. Pin scher-core to a fixed rev, not a branch. See [the next section](#pinning-scher-core).
2. Bump `version` in `Cargo.toml`, then run `cargo build` so that `Cargo.lock` picks up the new version. Commit both files and merge to `main` with CI green.
3. Tag the merge commit and push the tag:

   ```bash
   git switch main && git pull
   git tag -a v0.2.0 -m "scher-stacks v0.2.0"
   git push origin v0.2.0
   ```

4. Follow the run with `gh run watch`. The release appears at `gh release view v0.2.0`.

A tag containing a hyphen, such as `v0.2.0-rc.1`, is published as a GitHub prerelease and does not move `:latest`.

If a release run fails partway, fix the problem, delete the tag (`git push --delete origin v0.2.0 && git tag -d v0.2.0`, plus `gh release delete v0.2.0` if a release was created), and tag again. Don't reuse a tag that has already been deployed. Bump to the next patch version instead.

## Pinning scher-core

`Cargo.toml` currently pins the kernel to a **branch**:

```toml
scher-core = { git = "https://github.com/Unity-Environmental-University/scher.git", branch = "frame-is-not-a-user" }
```

Release builds use `--locked`, so in practice they build the rev recorded in `Cargo.lock` (`f6564125…` at the time of writing), not the branch tip. That only holds while the lockfile is left alone. Any `cargo update` silently moves to whatever the branch points at, and the branch has already moved: on 2026-10-16 its tip is `66c0af0`.

For releases, pin by **rev** (or by a scher tag, once scher tags kernel releases):

```toml
scher-core = { git = "https://github.com/Unity-Environmental-University/scher.git", rev = "f6564125e9af1ed10728cbe0dc36c42cbbfc7216" }
```

Then `Cargo.toml`, `Cargo.lock`, and the release all name the same kernel. Moving to a newer kernel becomes a visible one-line diff in a PR.

## Deploying a release (penelope-course)

Today `penelope-course/deploy/prepare-context.sh` copies the scher-stacks source and a `git archive` of scher-core into `deploy/vendor/`, then `stacks.Dockerfile` compiles them. Two things go wrong with that:

- The build takes minutes and needs a Rust toolchain.
- It vendors `origin/frame-is-not-a-user` (the branch **tip**) and deletes `Cargo.lock`. So the deployed kernel is not the one `Cargo.lock` pins, and dependency versions float.

With releases there are two simpler options.

**Option A, pull the image (recommended).** Drop the scher-stacks part of `prepare-context.sh` and `stacks.Dockerfile`. In compose:

```yaml
services:
  stacks:
    image: ghcr.io/unity-environmental-university/scher-stacks:v0.2.0
    environment:
      DATABASE_URL: postgres://…
      BIND_ADDR: 0.0.0.0:8080
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080/stacks/health/events"]
```

For a stricter pin, use the digest (`…/scher-stacks@sha256:…`) shown in the release run's `image` job.

**Option B, download the binary.** If penelope-course needs to keep its own Dockerfile:

```dockerfile
FROM debian:bookworm-slim
ARG STACKS_VERSION=v0.2.0
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && case "$TARGETARCH" in amd64) t=x86_64 ;; arm64) t=aarch64 ;; esac \
 && base=https://github.com/Unity-Environmental-University/scher-stacks/releases/download/$STACKS_VERSION \
 && f=scher-stacks-$STACKS_VERSION-$t-unknown-linux-gnu.tar.gz \
 && curl -fsSLO "$base/$f" && curl -fsSLO "$base/SHA256SUMS" \
 && grep " $f\$" SHA256SUMS | sha256sum -c - \
 && tar -xzf "$f" --strip-components=1 -C /usr/local/bin "${f%.tar.gz}/scher-stacks" \
 && rm -f "$f" SHA256SUMS
```

With either option, upgrading is a one-line version change in penelope-course, and that repo no longer needs `~/repos/scher-stacks` or `~/repos/scher` checked out for the stacks service.

## Migrations on upgrade

The migrations ship inside the binary. `sqlx::migrate!("./migrations")` embeds `migrations/*.sql` at compile time. At startup, `main.rs` runs any migrations that `_sqlx_migrations` doesn't already list, in filename order, before it binds the port. So:

- **Upgrading means starting the new version.** No separate migration step is needed. The release image deliberately has no `migrations/` directory.
- **Migrations are forward-only.** The store is append-only and there are no down migrations. To roll back the binary, the older binary must still work against the newer schema. Write migrations to be additive (new nullable columns, new tables, new indexes), as `0002_laid_by` (PR #1) is.
- **Never edit a migration that has shipped.** sqlx stores a checksum for each applied migration and refuses to start if a file it already applied has changed. Fix mistakes with a new numbered migration.
- **Run one instance during an upgrade.** sqlx takes a Postgres advisory lock while it migrates, so concurrent starts are safe. Still, start the new version once, let it migrate, then scale up.
- **Back up first** for anything that isn't purely additive: `pg_dump -Fc "$DATABASE_URL" > pre-v0.2.0.dump`.

`scher-stacks-<tag>-migrations.tar.gz` holds the same SQL for review, or for a DBA who wants to apply it by hand. If you apply it by hand, the service still checks `_sqlx_migrations`. Bookkeeping and DDL then need to agree, so prefer letting the binary migrate.

`build.rs` makes cargo rebuild whenever `migrations/` changes. Without it, adding a migration leaves the crate "Fresh" and an older binary, without the new migration, gets built or shipped. This matters for local dev builds as much as for releases.
