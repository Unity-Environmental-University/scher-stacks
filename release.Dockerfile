# Runtime image for a tagged release. Built only by .github/workflows/release.yml, which puts
# the release binaries at image/<arch>/scher-stacks before running this — nothing is compiled
# here, so the container carries the same bytes as the release tarball.
#
# Migrations are compiled INTO the binary (`sqlx::migrate!` embeds migrations/ at build time)
# and applied on startup, so no migrations directory is needed at runtime.

FROM debian:bookworm-slim

# curl for container healthchecks; ca-certificates for any TLS the operator adds in front.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
COPY image/${TARGETARCH}/scher-stacks /usr/local/bin/scher-stacks

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

# Nothing here needs root.
RUN useradd --system --uid 10001 stacks
USER stacks

CMD ["scher-stacks"]
