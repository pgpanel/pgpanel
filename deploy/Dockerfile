# syntax=docker/dockerfile:1.7

# ── Frontend build ────────────────────────────────────────────────────────
FROM node:22-bookworm-slim AS frontend
WORKDIR /frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install
COPY frontend/ ./
RUN npm run build

# ── Rust build ────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS backend
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libclang-dev clang cmake build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations

# Build release binary
RUN cargo build --release -p pgpanel-api

# ── Runtime ───────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /var/lib/pgpanel --create-home --shell /usr/sbin/nologin pgpanel

WORKDIR /app

COPY --from=backend /src/target/release/pgpanel /app/pgpanel
COPY --from=backend /src/migrations /app/migrations
COPY --from=frontend /frontend/build /app/static

ENV PGPANEL_BIND=0.0.0.0:8080 \
    PGPANEL_DATA_DIR=/var/lib/pgpanel \
    PGPANEL_STATIC_DIR=/app/static \
    PGPANEL_COOKIE_SECURE=true \
    RUST_LOG=pgpanel=info,tower_http=info

# Docker socket is mounted at runtime (privileged risk — see SECURITY.md).
# Panel process runs as root only if socket permissions require it;
# prefer rootless docker or a dedicated docker group in production.

EXPOSE 8080
VOLUME ["/var/lib/pgpanel"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/health || exit 1

WORKDIR /app
CMD ["/app/pgpanel"]
