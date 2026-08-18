# EphoriX API — Rust (axum + sqlx), statically linked enough for a slim image.
FROM rust:1.97-slim AS builder

WORKDIR /app

# Build deps first for layer caching.
COPY Cargo.toml ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Migrations are embedded at compile time via sqlx::migrate!.
COPY migrations ./migrations
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ephorix-api /usr/local/bin/ephorix-api

EXPOSE 3000
CMD ["ephorix-api"]
