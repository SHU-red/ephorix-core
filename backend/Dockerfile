# EphoriX API — Rust (axum + sqlx), statically linked enough for a slim image.
FROM rust:1.97-slim AS builder

WORKDIR /app

# Build deps first for layer caching. Cargo.lock is copied too: without it the
# dependency graph is re-resolved on every build and the cached layer above
# does not reflect what actually ships.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# Migrations are embedded at compile time via sqlx::migrate!.
COPY migrations ./migrations
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ephorix-api /usr/local/bin/ephorix-api

EXPOSE 3000
CMD ["ephorix-api"]
