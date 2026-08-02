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

ENV PROTOC=/usr/bin/protoc

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations
COPY VERSION ./VERSION

# Build release binary
RUN cargo build --release -p pgpanel-api

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

COPY --from=backend /src/target/release/pgpanel /app/pgpanel
COPY --from=backend /src/migrations /app/migrations
COPY --from=frontend /frontend/build /app/static
COPY VERSION /app/VERSION

ENV PGPANEL_BIND=0.0.0.0:8080 \
    PGPANEL_DATA_DIR=/var/lib/pgpanel \
    PGPANEL_STATIC_DIR=/app/static \
    PGPANEL_COOKIE_SECURE=true \
    PGPANEL_VERSION=${PGPANEL_VERSION} \
    RUST_LOG=pgpanel=info,tower_http=info

# Docker socket is mounted at runtime (privileged risk — see SECURITY.md).

EXPOSE 8080
VOLUME ["/var/lib/pgpanel"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/health || exit 1

WORKDIR /app
CMD ["/app/pgpanel"]
