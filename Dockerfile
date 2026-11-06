# SubForge server in a container — the deployment the automation is built for.
#
#   docker build -t subforge-server .
#   docker run -d --name subforge -p 8080:8080 \
#     -v /srv/subforge:/data -v /srv/media:/media:rw subforge-server
#
# FFmpeg and MKVToolNix come from the distribution packages: the container is the one place
# where "install these two" is not a user-facing step.

# syntax=docker/dockerfile:1

# ── frontend ────────────────────────────────────────────────────────────────────────────────
FROM node:20-bookworm-slim AS web
WORKDIR /src
# Only the manifests first, so a source-only change does not reinstall node_modules.
COPY package.json package-lock.json ./
RUN npm ci
COPY tsconfig.json tsconfig.node.json vite.config.ts vitest.config.ts index.html eslint.config.js ./
COPY public ./public
COPY src ./src
COPY scripts ./scripts
RUN npm run build

# ── server ──────────────────────────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS build
# cmake/clang build whisper.cpp; libssl-dev + pkg-config for reqwest's native TLS.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY server ./server
COPY crates ./crates
RUN cargo build --release --manifest-path server/Cargo.toml

# ── runtime ─────────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl ffmpeg mkvtoolnix \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /src/server/target/release/subforge-server /usr/local/bin/subforge-server
COPY --from=web /src/dist ./dist

# /data holds config.json and the job journal; /media is where libraries are mounted.
ENV SUBFORGE_HOST=0.0.0.0:8080 \
    SUBFORGE_CONFIG_DIR=/data \
    RUST_LOG=info

RUN useradd --uid 10001 --create-home app \
    && mkdir -p /data /media \
    && chown -R app:app /data /media /app
USER app

VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
    CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/subforge-server"]
