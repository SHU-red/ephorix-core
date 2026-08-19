# Stage 1: build the Leptos CSR bundle with trunk.
# wasm-bindgen-cli MUST match the wasm-bindgen version in frontend/Cargo.lock.
FROM rust:1.97-slim AS builder

WORKDIR /app

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --version 0.21.14 --locked \
    && cargo install wasm-bindgen-cli --version 0.2.127 --locked

COPY Cargo.toml Cargo.lock ./
COPY index.html ./
COPY public ./public
COPY src ./src

RUN trunk build --release

# Stage 2: nginx serves the bundle and proxies /api to the API service
# (same-origin, so the browser never needs CORS).
FROM nginx:1.27-alpine

COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80
