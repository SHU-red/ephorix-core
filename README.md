# EphoriX Core

Backend + web UI for the EphoriX health/fitness ecosystem. Strictly black and
red; raw sensor metrics and discrete Agoge events are kept separate.

## The three names

- **EphoriX** — the *ephors*, the Spartan overseers surveilling your hard
  agoge workout. Every rep, every beat, every step is watched and logged.
- **Agoge** — the Spartan training regimen itself; a workout. Start one on
  the watch, train, close it — the raw stream is kept for retro-analysis.
- **Leonidas** — the standard. Set your goals and train toward reaching
  Leonidas: the ceiling of the Spartan body.

```
ephorix-core/
├── docker-compose.yml          # one port (web :9000), one volume (db), api internal
├── docker-compose.dev.yml      # dev override: exposes db for local cargo run
├── backend/                    # Rust: axum + sqlx
│   ├── migrations/             # schema + seed (hypertable, users, types, settings)
│   ├── src/
│   │   ├── auth.rs             # X-EphoriX-Token middleware (multi-user ready)
│   │   └── routes/             # health, events, agoge-types, sessions, timeline, settings
│   └── Dockerfile
├── frontend/                   # Leptos (CSR) + uPlot, served by trunk
├── scripts/publish.sh          # build images locally, push to GHCR
└── docs/api-contract.md        # JSON payload contract (PebbleKit JS ↔ backend)
```

## Architecture

- **Stateless & event-driven.** `raw_health_data` is a TimescaleDB hypertable
  (`timestamp`, `user_id`, `heart_rate`, `steps`, `active_calories`). Agoge
  sessions are derived from Start/Stop marker events (`agoge_markers`) but
  remain manually editable. `pause`/`resume` markers record rest periods
  inside an open session without closing it. Raw data is never locked to
  sessions — the UI joins them by time range at query time, so ML can
  retro-analyze the raw stream (reps, rest periods, workout detection).
- **Offline-first.** PebbleKit JS queues payloads in `localStorage` and flushes
  with exponential backoff on reconnect. Permanent client errors (4xx) are
  dead-lettered so a stale item can never block the queue. The watch sends
  health snapshots only per its Auto Push mode (battery discipline) — see
  `ephorix-pebble`.
- **Multi-user.** POC uses fixed tokens (`ephorix-dev-1`, `ephorix-dev-2`)
  resolved against `users`; every query is scoped by the authenticated
  `user_id`. Swapping in real auth touches only `auth.rs`.
- **One volume, everything in the DB.** All state — raw metrics, sessions,
  markers, Agoge types, and per-user UI settings (`user_settings` JSONB) —
  lives in the database volume. No second persistent mount.
- **Multi-source, normalized.** Every source (Pebble, Fitbit, Garmin, Apple
  Health, manual) converges on the `measurements` hypertable via
  `POST /api/v1/ingest`; the Pebble health batch is one such adapter. Derived
  metrics — body battery (`/api/v1/metrics/body-battery`), automated workout
  detection (`/api/v1/metrics/workouts`), and food/water intake
  (`/api/v1/nutrition`) — read that one normalized store, so they work for
  any source.

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
API access without a proxy).

## Web UI

- **Timeline**: heart rate, steps and active-kcal series with a live legend;
  each metric toggles from a compact pill strip. Time range is a button row —
  **1D / 1W / 1M / 1Y** — downsampled server-side (`time_bucket`), so even a
  year stays fluid.
- **Region selection**: drag on the plot → the range is shown, then
  "CREATE SESSION FROM SELECTION" (with a chosen Agoge type) or
  "CLEAR SELECTION". "CLOSE OPEN AT CURSOR" closes the open session at the
  hovered time.
- **Agoge sessions**: list with type/status/times, delete.
- **Agoge types**: create and edit — name, color picker, monochrome SVG glyph
  set. Renames/deletes never break sessions (referenced by id; missing types
  render as *Undefined*).
- **Settings**: a collapsible panel holds the API base URL and token
  (`X-EphoriX-Token`). Series visibility and the timeline range are also
  stored per user (`/api/v1/settings`, `user_settings` table) — the web app
  needs no volume of its own.

## Images (GHCR, built locally)

Images are always built on your machine and pushed to GitHub Packages —
no CI. `scripts/publish.sh` applies the tagging scheme:

| Build | Tags on `ghcr.io/shu-red/ephorix-{api,web}` |
|---|---|
| `./scripts/publish.sh` (dev) | `dev` (rolling) + `dev-<short-sha>` (lock) |
| `./scripts/publish.sh vX.Y.Z` (release) | `latest` (rolling) + `vX.Y.Z` + `X.Y` + `<short-sha>` (lock) |

`latest` is only produced by release builds, `dev` only by dev builds. Every
build is additionally tagged with its commit, so any deployed version can be
locked by sha.

```bash
docker login ghcr.io -u <user> -p <PAT>   # PAT scope: write:packages
./scripts/publish.sh --no-push            # build + tag only, smoke-test first
./scripts/publish.sh                      # build + push
```

The frontend image build takes a few minutes (it installs trunk +
wasm-bindgen-cli inside the builder). Equivalent one-liner via compose:

```bash
EPHORIX_TAG=dev docker compose build && EPHORIX_TAG=dev docker compose push
```

Deploy from the registry (no toolchain needed on the server):

```bash
# .env: EPHORIX_TAG=latest   # one tag = the whole product (api + web)
docker compose pull
docker compose up -d
```

The timescaledb database image is upstream and never built.

## Deploying to a server

The watch does **not** need the frontend — it talks to the API on the same
port. Production stack (no toolchain on the box — images come from GHCR):

```bash
# .env: EPHORIX_TAG=latest, EPHORIX_PG_VOLUME=..., EPHORIX_WEB_PORT=9000
docker compose pull
docker compose up -d
```

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
4. **Backup** `EPHORIX_PG_VOLUME` (or the bind-mount path). The hypertable,
   sessions, types and settings all live there — it is the only state in the
   stack. On RHEL/Fedora SELinux hosts use `EPHORIX_VOLUME_MODE=z` (or `Z`)
   for bind mounts.

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
- The watch stores its backend URL/token on the phone (Pebble config page,
  `localStorage`); its Auto Push mode is persisted on the watch itself.
- Auth is the documented dummy-token scheme; swap `auth.rs` for real auth
  without touching the data layer.
- Day-to-day development: see `DEVELOPMENT.md`.
