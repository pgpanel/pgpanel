# syntax=docker/dockerfile:1.7
# BuildKit required (docker buildx). Cargo/npm caches make incremental builds fast.

# ── Frontend build ────────────────────────────────────────────────────────
FROM node:22-bookworm-slim AS frontend
WORKDIR /frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN --mount=type=cache,target=/root/.npm,sharing=locked \
    npm ci --prefer-offline --no-audit --no-fund || npm install --no-audit --no-fund
COPY frontend/ ./
RUN --mount=type=cache,target=/root/.npm,sharing=locked \
    npm run build

# ── Rust build (cached deps + incremental target) ─────────────────────────
FROM rust:1-bookworm AS backend
WORKDIR /src

# pg_query needs protoc + rustfmt (prost codegen); clang for bindgen
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libclang-dev \
    clang \
    cmake \
    build-essential \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt \
    && protoc --version \
    && rustfmt --version

ENV PROTOC=/usr/bin/protoc \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=1

# 1) Manifest-only layer — dependency compile is cached across source edits
COPY Cargo.toml Cargo.lock* ./
COPY crates/pgpanel-api/Cargo.toml crates/pgpanel-api/
COPY crates/pgpanel-core/Cargo.toml crates/pgpanel-core/
COPY crates/pgpanel-docker/Cargo.toml crates/pgpanel-docker/
COPY crates/pgpanel-postgres/Cargo.toml crates/pgpanel-postgres/
COPY crates/pgpanel-backup/Cargo.toml crates/pgpanel-backup/
COPY crates/pgpanel-databasus/Cargo.toml crates/pgpanel-databasus/
COPY crates/pgpanel-jobs/Cargo.toml crates/pgpanel-jobs/

RUN mkdir -p \
      crates/pgpanel-api/src \
      crates/pgpanel-core/src \
      crates/pgpanel-docker/src \
      crates/pgpanel-postgres/src \
      crates/pgpanel-backup/src \
      crates/pgpanel-databasus/src \
      crates/pgpanel-jobs/src \
      migrations \
    && printf 'fn main() {}\n' > crates/pgpanel-api/src/main.rs \
    && for c in pgpanel-core pgpanel-docker pgpanel-postgres pgpanel-backup pgpanel-databasus pgpanel-jobs; do \
         printf '// dep-cache stub\n' > "crates/${c}/src/lib.rs"; \
       done \
    && printf '0.0.0\n' > VERSION \
    && touch migrations/.keep

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release -p pgpanel-api \
    && rm -rf crates/*/src

# 2) Real sources — only this layer invalidates on code change; target cache keeps deps warm
COPY crates ./crates
COPY migrations ./migrations
COPY VERSION ./VERSION

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    find crates -name '*.rs' -exec touch {} + \
    && cargo build --release -p pgpanel-api \
    && cp /src/target/release/pgpanel /src/pgpanel \
    && strip /src/pgpanel || true

# ── Runtime ───────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
ARG PGPANEL_VERSION=0.1.0
LABEL org.opencontainers.image.title="PgPanel" \
      org.opencontainers.image.description="Self-hosted PostgreSQL management panel" \
      org.opencontainers.image.vendor="Dezső Benedek Péter" \
      org.opencontainers.image.authors="Dezső Benedek Péter" \
      org.opencontainers.image.source="https://github.com/pgpanel/pgpanel" \
      org.opencontainers.image.version="${PGPANEL_VERSION}"

# docker CLI: panel runs `docker exec` for pg_dump/pg_restore against cluster containers
# (socket mounted at runtime — not the daemon).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" \
       > /etc/apt/sources.list.d/docker.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends docker-ce-cli \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /var/lib/pgpanel --create-home --shell /usr/sbin/nologin pgpanel

WORKDIR /app

COPY --from=backend /src/pgpanel /app/pgpanel
COPY --from=backend /src/migrations /app/migrations
COPY --from=frontend /frontend/build /app/static
COPY VERSION /app/VERSION

ENV PGPANEL_BIND=0.0.0.0:8080 \
    PGPANEL_DATA_DIR=/var/lib/pgpanel \
    PGPANEL_STATIC_DIR=/app/static \
    PGPANEL_COOKIE_SECURE=true \
    PGPANEL_VERSION=${PGPANEL_VERSION} \
    RUST_LOG=pgpanel=info,tower_http=info

EXPOSE 8080
VOLUME ["/var/lib/pgpanel"]

HEALTHCHECK --interval=5s --timeout=3s --start-period=15s --retries=12 \
  CMD curl -fsS http://127.0.0.1:8080/health || exit 1

WORKDIR /app
CMD ["/app/pgpanel"]
