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
