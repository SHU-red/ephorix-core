# EphoriX Core

Backend + web UI for the EphoriX health/fitness ecosystem. Strictly black and
red; raw sensor metrics and discrete Agoge events are kept separate.

```
ephorix-core/
├── docker-compose.yml          # TimescaleDB + API
├── backend/                    # Rust: axum + sqlx
│   ├── migrations/             # schema + seed (hypertable, users, types)
│   ├── src/
│   │   ├── auth.rs             # X-EphoriX-Token middleware (multi-user ready)
│   │   └── routes/             # health, events, agoge-types, agoge-sessions, timeline
│   └── Dockerfile
├── frontend/                   # Leptos (CSR) + uPlot, served by trunk
└── docs/api-contract.md        # JSON payload contract (PebbleKit JS ↔ backend)
```

## Architecture

- **Stateless & event-driven.** `Raw_Health_Data` is a TimescaleDB hypertable
  (`timestamp`, `user_id`, `heart_rate`, `steps`, `active_calories`). Agoge
  sessions are derived from Start/Stop marker events (`agoge_markers`) but
  remain manually editable. Raw data is never locked to sessions — the UI
  joins them by time range at query time, so ML can retro-analyze the raw
  stream (reps, rest periods, workout detection).
- **Offline-first.** PebbleKit JS queues payloads in `localStorage` and flushes
  with exponential backoff on reconnect (see `ephorix-pebble`).
- **Multi-user.** POC uses fixed tokens (`ephorix-dev-1`, `ephorix-dev-2`)
  resolved against `users`; every query is scoped by the authenticated
  `user_id`. Swapping in real auth touches only `auth.rs`.

## Run

```bash
docker compose up --build -d          # db + api on :5432 / :3000
curl localhost:3000/healthz           # → ok
curl -H "X-EphoriX-Token: ephorix-dev-1" localhost:3000/api/v1/agoge-types
```

Migrations run automatically on boot (`sqlx::migrate!`).

Everything in `docker-compose.yml` is env-driven; copy `.env.example` to `.env`
and adjust (ports, credentials, volume, image tag, CORS origins). The API
healthcheck keeps `depends_on: service_healthy` honest — the api container
only becomes "healthy" when `/healthz` answers.

| Variable | Default | Meaning |
|---|---|---|
| `EPHORIX_DB_USER` / `EPHORIX_DB_PASSWORD` / `EPHORIX_DB_NAME` | `ephorix` | database credentials |
| `EPHORIX_DB_PORT` | `5432` | host port for the database (omit to keep DB stack-internal) |
| `EPHORIX_PG_VOLUME` | `ephorix_pgdata` | named volume, or an absolute host path for a bind mount |
| `EPHORIX_VOLUME_MODE` | *(empty)* | SELinux relabel mode `z`/`Z` for bind mounts (RHEL/Fedora); ignored for named volumes |
| `EPHORIX_TIMESCALE_TAG` | `latest-pg16` | TimescaleDB image tag |
| `EPHORIX_DATABASE_URL` | built from the above | full override (needed if the password contains URL-reserved chars) |
| `EPHORIX_API_PORT` | `3000` | host port for the API |
| `EPHORIX_CORS_ORIGINS` | localhost:8080 | comma-separated browser origins allowed to call the API |
| `EPHORIX_LOG_LEVEL` | `info,ephorix_api=debug` | `RUST_LOG` filter |

Frontend (dev, port 8080):

```bash
cd frontend
cargo install trunk wasm-bindgen-cli   # once
rustup target add wasm32-unknown-unknown
trunk serve --open
```

Set BASE / TOKEN in the header and hit SYNC. Drag on the timeline to select a
range → "CREATE SESSION FROM SELECTION"; hover to a time → "CLOSE OPEN AT
CURSOR".

## Deploying to a server

The watch does **not** need the frontend — it talks to the API directly.
Minimal production stack: `docker compose up --build -d` behind TLS.

1. **Rotate the seeded tokens.** `0002_seed.sql` ships dev tokens
   (`ephorix-dev-1`, `ephorix-dev-2`). Add real users on the box:
   ```sql
   INSERT INTO users (token, display_name) VALUES ('<long-random-token>', 'Leonidas');
   ```
   (or `UPDATE users SET token = ... WHERE id = ...`). Put the token into the
   watch's config page (long-press the app in the Pebble phone app →
   Settings → BACKEND BASE URL + TOKEN).
2. **TLS in front.** The API is plain HTTP; put Caddy/nginx in front
   (Caddy: `reverse_proxy 127.0.0.1:3000` + your domain). Set
   `EPHORIX_CORS_ORIGINS` to the served frontend origin if you host the UI
   on a domain.
3. **Backup** `EPHORIX_PG_VOLUME` (or the bind-mount path). The hypertable
   and all data live there.
4. **Watch defaults.** `src/js/pebble-js-app.js` defaults to
   `http://192.168.1.10:3000` — the config page overrides it; nothing to
   recompile.

## Verify

```bash
cd backend && cargo test    # bucket validation unit tests
```

## Notes / limits (POC)

- Migrations and all endpoints were verified end-to-end against PostgreSQL
  16.2 with the TimescaleDB 2.13.0 extension loaded (hypertable chunk
  confirmed via `timescaledb_information`). `docker compose up` should apply
  them unchanged.
- Timeline bucket guard caps responses at 2000 points; the client picks a
  bucket yielding ≤ 800.
- PebbleKit JS config page lets you set the backend base URL (persisted in
  `localStorage`).
- Auth is the documented dummy-token scheme; swap `auth.rs` for real auth
  without touching the data layer.
