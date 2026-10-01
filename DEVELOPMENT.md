# Development

Everything you need to iterate locally. Two build targets, two tools:

- **Watch app** → build & install via **CloudPebble** (no local SDK needed)
- **Containers** (api + web images) → build locally, push via **`scripts/publish.sh`**

## Local stack (docker)

Spin up the whole product from the repo — no registry, no publish:

```bash
docker compose up --build -d
# web UI:      http://localhost:9000
# healthcheck: curl localhost:9000/healthz
```

`--build` builds `api` + `web` from the repo sources and tags them with the
current `EPHORIX_TAG` (default `latest`). Migrations run automatically on
boot. Data lands in the `ephorix_pgdata` volume — wipe it with
`docker compose down -v` to start fresh.

### Backend iteration (cargo run)

The db is internal-only in the default compose. For a fast
edit → compile → run loop, expose it with the dev override:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d db
cd backend
DATABASE_URL=postgres://ephorix:ephorix@localhost:5432/ephorix cargo run
# API on http://localhost:3000 (token header: X-EphoriX-Token: ephorix-dev-1)
```

### Frontend iteration (trunk)

```bash
cd frontend
cargo install trunk wasm-bindgen-cli   # once
rustup target add wasm32-unknown-unknown
trunk serve --proxy-backend http://localhost:3000
# http://localhost:8080 — /api/* is proxied to the running backend
```

The UI defaults to same-origin API calls (BASE field empty); with the proxy
flag it just works. Leave BASE empty in production (nginx does the proxying).

### Watch ↔ backend loop (emulator)

For iterating on the watch app against a local backend, `ephorix-pebble` ships
a dev harness that boots the emulator, points the phone-side JS at
`http://localhost:3000`, drives synthetic sensor scenarios, and asserts what
reached the backend — including a dependency-free recorder backend for when you
have no Docker/Postgres:

```bash
cd ../ephorix-pebble
./scripts/dev.sh help
```

See the "Dev loop" section of `ephorix-pebble/README.md`.

## Watch app via CloudPebble

No Pebble SDK on your machine required — CloudPebble (rebble.io) builds for
you and gives you an install QR code.

1. Go to **cloudpebble.net**, log in (Rebble account).
2. **Create a new Pebble project** (watchapp).
3. Replace the generated sources with the repo files:
   - `ephorix-pebble/appinfo.json` → project settings (import via the
     project's JSON editor or paste the contents)
   - `ephorix-pebble/src/c/*.c` and `*.h` → **C code** (all five files:
     `main.c`, `comm.c`, `comm.h`, `health_collect.c`, `health_collect.h`)
   - `ephorix-pebble/src/js/pebble-js-app.js` → **JS** (must be named
     `pebble-js-app.js`)
   - `ephorix-pebble/resources/resources.json` → resources (empty media list
     is fine)
4. **Build** → target **Pebble Time 2** (`emery`).
5. **Install** → scan the QR code with the Pebble/Rebble app on your phone.
6. Configure: long-press the app in the phone app → Settings → set
   **BACKEND BASE URL** (`http://<your-server>:9000`) and **TOKEN**.

The app is self-contained: types are pulled from the backend, health
snapshots are pushed per the watch's Auto Push mode, and everything queues
offline-first in the JS layer.

## Publishing containers (GHCR)

Two images, one tag, so `api` + `web` always match:
`ghcr.io/shu-red/ephorix-api` (context `./backend`) and
`ghcr.io/shu-red/ephorix-web` (context `./frontend`).

### CI

`.github/workflows/docker-publish.yml` builds and pushes both on every push to
`main` and every `v*` tag. Pull requests build both images **without pushing**,
so a broken Dockerfile fails before merge.

- `main` → `:main`, `:<sha>`, `:latest`
- `v1.2.3` → `:1.2.3`, `:1.2`, `:<sha>`, `:latest`
- `latest` is tagged only on the default branch — which is what
  `docker compose pull` resolves.

**If the push fails with `denied: permission_denied: write_package`**, the
build itself still succeeded — that error only appears at the export step. The
cause is package access, not the workflow: `GITHUB_TOKEN` is scoped to this
repository, and a package that already exists under the account but is **not
linked to this repo** cannot be written by it. The images were first published
by hand via `publish.sh` + a PAT, which is exactly how a package ends up
unlinked. Fix once per image:

> Profile → **Packages** → `ephorix-api` → *Package settings* → **Manage Actions
> access** → *Add repository* → `SHU-red/ephorix-core`, role **Write**
> (repeat for `ephorix-web`)

Deleting the package works too — CI then recreates it already linked.
Raising **Settings → Actions → General → Workflow permissions** is *not* the
fix; the workflow already requests `packages: write` explicitly, and a run with
the repo default set to read-write was still denied.

### Local build (no CI round-trip)

```bash
sudo docker login ghcr.io -u <github-user> -p <PAT>   # scope: write:packages
```

> Note: on this machine docker needs `sudo` (no docker group) and `sudo`
> reads **root's** docker config — hence the `sudo docker login` above. A
> login done as your normal user is NOT seen by the sudo run.

```bash
./scripts/publish.sh --no-push   # build + tag only — smoke-test first
./scripts/publish.sh             # dev build  → dev + dev-<sha>
./scripts/publish.sh v1.2.3      # release    → latest + v1.2.3 + X.Y + <sha>
```

Deploy anywhere (server needs no toolchain):

```bash
# server .env: EPHORIX_TAG=dev   (or latest / a locked dev-<sha>)
docker compose pull && docker compose up -d
```

The frontend image build takes a few minutes (it installs trunk +
wasm-bindgen-cli inside the builder). Smoke-test locally before pushing:
`./scripts/publish.sh --no-push`, then
`docker run --rm -p 9000:80 ghcr.io/shu-red/ephorix-web:dev`.

## Verification

- Backend unit tests: `cd backend && cargo test`
- Full API E2E against a live stack: the curl examples in `README.md`
  (healthz, types, timeline)
- Web UI: covered by the browser run in the frontend section
- Pebble build (if you keep the SDK): `cd ephorix-pebble && pebble build` —
  SDK 4.33 requires the Python 3.13+ waf patches listed in the pebble
  README; CloudPebble sidesteps all of that.
