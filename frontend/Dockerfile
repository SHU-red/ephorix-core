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

# Cache-bust stable public/ assets: give style.css and uplot-bridge.js a
# per-build content hash in the URL and rewrite index.html to match. A stale
# cached stylesheet/bridge can then NEVER be served after a redeploy — the
# no-cache index.html always points at the fresh hashed file.
RUN css=$(sha256sum dist/style.css | cut -c1-16) \
 && mv dist/style.css "dist/style.$css.css" \
 && sed -i "s|/style.css|/style.$css.css|" dist/index.html \
 && js=$(sha256sum dist/uplot-bridge.js | cut -c1-16) \
 && mv dist/uplot-bridge.js "dist/uplot-bridge.$js.js" \
 && sed -i "s|/uplot-bridge.js|/uplot-bridge.$js.js|" dist/index.html

# Stage 2: nginx serves the bundle and proxies /api to the API service
# (same-origin, so the browser never needs CORS).
FROM nginx:1.27-alpine

COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html

EXPOSE 80
