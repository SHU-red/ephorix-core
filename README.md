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
docker compose up --build -d          # one exposed port: 9000
curl localhost:9000/healthz           # → ok (through nginx → api → db)
curl -H "X-EphoriX-Token: ephorix-dev-1" localhost:9000/api/v1/agoge-types
```

Migrations run automatically on boot (`sqlx::migrate!`).

**Topology: one port, one volume.** The database and API have **no host
ports** — they are internal to the compose network. The web service is the
only entry point:

| Port | What lives there |
|---|---|
| `9000` (`EPHORIX_WEB_PORT`) | web UI at `/`, API at `/api/*` (same-origin, no CORS) — the watch uses this same port as its base URL (`POST /api/v1/health/batch`, `POST /api/v1/events/marker`, …) |

The one volume is the database data dir (`EPHORIX_PG_VOLUME`).

Copy `.env.example` to `.env` and adjust. Healthchecks gate startup:
api becomes healthy only when `/healthz` answers, and the web container's
healthcheck runs through nginx → api → db, proving the whole chain.

| Variable | Default | Meaning |
|---|---|---|
| `EPHORIX_DB_USER` / `EPHORIX_DB_PASSWORD` / `EPHORIX_DB_NAME` | `ephorix` | database credentials (internal only) |
| `EPHORIX_PG_VOLUME` | `ephorix_pgdata` | the one volume: named volume, or an absolute host path for a bind mount |
| `EPHORIX_VOLUME_MODE` | *(empty)* | SELinux relabel mode `z`/`Z` for bind mounts (RHEL/Fedora) |
| `EPHORIX_TIMESCALE_TAG` | `latest-pg16` | TimescaleDB image tag |
| `EPHORIX_DATABASE_URL` | built from the above | full override (needed if the password contains URL-reserved chars) |
| `EPHORIX_WEB_PORT` | `9000` | the single exposed port (UI + `/api`) |
| `EPHORIX_LOG_LEVEL` | `info,ephorix_api=debug` | `RUST_LOG` filter |

Frontend (dev, port 8080):

```bash
cd frontend
cargo install trunk wasm-bindgen-cli   # once
rustup target add wasm32-unknown-unknown
trunk serve --proxy-backend http://localhost:3000
```

The UI defaults to same-origin API calls (BASE field empty): in dev, trunk
forwards `/api/*` to the backend via `--proxy-backend`; in production, nginx
does the same. Type a full URL in the BASE field to override (e.g. direct
API access without a proxy). Drag on the timeline to select a range →
"CREATE SESSION FROM SELECTION"; hover to a time → "CLOSE OPEN AT CURSOR".

## Deploying to a server

The watch does **not** need the frontend — it talks to the API on the same
port. Production stack: `docker compose up --build -d` behind TLS.

1. **Rotate the seeded tokens.** `0002_seed.sql` ships dev tokens
   (`ephorix-dev-1`, `ephorix-dev-2`). Add real users on the box:
   ```sql
   INSERT INTO users (token, display_name) VALUES ('<long-random-token>', 'Leonidas');
   ```
   (or `UPDATE users SET token = ... WHERE id = ...`). Put the token into the
   watch's config page (long-press the app in the Pebble phone app →
   Settings → BACKEND BASE URL + TOKEN).
2. **One port.** Expose only `9000` — the web service serves the UI and
   proxies `/api` to the backend, so the watch's base URL is simply
   `http://your-host:9000` and the browser needs no CORS. Nothing else is
   reachable from outside the stack (db and api have no host ports).
3. **TLS in front.** The web service is plain HTTP; put Caddy/nginx in front
   (Caddy: `reverse_proxy 127.0.0.1:9000` + your domain).
4. **Backup** `EPHORIX_PG_VOLUME` (or the bind-mount path). The hypertable
   and all data live there — it is the only state in the stack.

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
