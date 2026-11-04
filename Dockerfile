# syntax=docker/dockerfile:1.7
#
# Lodestone: the backend plus the web UI it serves. Build from the repo root:
#   docker build -t lodestone .
#
# Run it with compose.example.yaml: it needs the Docker socket (servers run
# as sibling containers), host networking (to reach their RCON on
# 127.0.0.1) and a data dir mounted at the same path inside and out.

# The web build is platform independent, so it runs natively on the builder.
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS web
# git fetches @sunstead/ui, a GitHub dependency pinned to a tag.
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,id=lodestone-npm,sharing=locked,target=/root/.npm npm ci --no-audit --no-fund
COPY frontend/ ./
COPY shared/ ../shared/
RUN npm run build:live

FROM rust:1-bookworm AS build
WORKDIR /src
COPY backend/ backend/
COPY shared/ shared/
RUN --mount=type=cache,id=lodestone-registry,sharing=locked,target=/usr/local/cargo/registry \
    --mount=type=cache,id=lodestone-target,sharing=locked,target=/src/backend/target \
    cd backend && cargo build --locked --release --bin lodestone \
 && cp /src/backend/target/release/lodestone /lodestone

# distroless/cc: glibc and libgcc, no shell. TLS roots are compiled in
# (rustls + webpki roots) and SQLite is bundled.
FROM gcr.io/distroless/cc-debian12
COPY --from=build /lodestone /usr/local/bin/lodestone
COPY --from=web /src/frontend/dist /usr/share/lodestone/web
ENV LODESTONE_STATIC_DIR=/usr/share/lodestone/web \
    LODESTONE_BIND=0.0.0.0:7878 \
    LODESTONE_DATA=/srv/lodestone
EXPOSE 7878
ENTRYPOINT ["/usr/local/bin/lodestone"]
