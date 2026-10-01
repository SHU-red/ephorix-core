# EphoriX — API Contract v0.1 (POC)

Base URL: `http://<host>:3000`
Auth: every `/api/v1/*` request MUST carry the header `X-EphoriX-Token: <token>`.
POC tokens (seeded): `ephorix-dev-1`, `ephorix-dev-2`.
Errors: `{"error": {"code": "bad_request|unauthorized|not_found|internal_error", "message": "..."}}`

All timestamps are ISO-8601 UTC strings (`2026-08-18T10:00:00Z`) unless noted.
The `timeline` endpoint returns epoch **milliseconds** to keep charting fast.

---

## 1. Health ingestion (PebbleKit JS → backend)

### `POST /api/v1/health/batch`

High-throughput push of batched raw sensor metrics. Max 1000 samples per batch.
Idempotent: rows dedupe on `(userId, timestamp)` (`ON CONFLICT DO NOTHING`), so a
re-pushed or retried batch never duplicates. Writes are **first-write-wins** — a
retry carrying corrected values for an existing timestamp is ignored.

**Counter semantics (important).** `steps` and `activeCalories` are the watch's
*cumulative local-day totals* (`health_service_sum(metric, start_of_today, now)`),
NOT per-bucket deltas. The watch re-sends the growing value at every cadence, and
the read path re-derives per-bucket deltas (the timeline SQL). Consumers MUST NOT
`SUM()` these rows. `heartRate` is an instantaneous (filtered) sample.

```jsonc
{
  "deviceId": "pebble:6b3a7f",            // informational, optional
  "batchedAt": "2026-08-18T10:00:00Z",    // optional, queue flush time
  "samples": [
    {
      "timestamp": "2026-08-18T09:59:00Z", // REQUIRED
      "heartRate": 128,                    // optional, BPM, null while off-wrist
      "steps": 42,                         // optional, CUMULATIVE steps since local midnight
      "activeCalories": 3.2                // optional, CUMULATIVE kcal since local midnight
    }
  ]
}
```

Response `200`:
```json
{ "inserted": 2, "normalized": 2 }
```
`inserted` counts raw + normalized rows; `normalized` is the `measurements`
mirror subset.

### `POST /api/v1/health/days`

Daily aggregate backfill from the watch (Pebble Health retains up to 30 days).
Max 31 days per batch. Each day becomes one `measurements` row per present
metric, anchored at the day's **UTC 12:00** (`YYYY-MM-DD` is the watch's local
date). Idempotent on `(userId, metric, ts)` — re-posting a day is a no-op.

```jsonc
{
  "deviceId": "pebble:6b3a7f",
  "batchedAt": "2026-08-18T10:00:00Z",
  "days": [
    { "d": "2026-08-17", "steps": 12000, "activeKcal": 540,
      "sleepSeconds": 26400, "restfulSleepSeconds": 7200,
      "distanceM": 9100, "activeSeconds": 3100, "restingKcal": 1600 }
  ]
}
```
Response `200`: `{ "inserted": <n> }`. Absent fields are omitted (never zero).

---

## 2. Marker events (Start_Marker / Stop_Marker)

### `POST /api/v1/events/marker`

Discrete event from the watch (or web). The backend materializes the session:
`start` → creates an `active` session; `stop` → closes it (by `sessionId` or the
latest open one). `pause` / `resume` are informational rest-period markers —
recorded against the open session, never closing it. `dismiss` → the session is
**discarded without logging**: the session row and its whole marker stream are
deleted (by `sessionId`, else the user's latest open session); deleting nothing
is still a 200 so a retried queued job never stalls, and the response is
`{ "deleted": "<uuid>" | null }`. Unknown/missing type → session recorded as
**Undefined Agoge** (`typeId: null`).

**Idempotency & lifecycle.** Markers dedupe on `(userId, kind, occurredAt)` — a
replayed marker is a no-op, and a replayed `stop` returns the already-closed
session (200) instead of a 404, so a retried queued job never strands. Only one
Agoge is open at a time: a new `start` closes any other `active` session, so a
lost `stop` self-heals instead of leaving an orphaned open session.

```jsonc
{
  "kind": "start",                        // "start" | "stop" | "pause" | "resume" | "dismiss"
  "typeId": "11111111-1111-1111-1111-111111111111", // optional UUID (start only)
  "typeName": "Strength",                 // optional fallback lookup
  "occurredAt": "2026-08-18T09:30:00Z",   // optional, defaults to now()
  "sessionId": "22222222-2222-2222-2222-222222222222", // optional, for stop/pause/resume
  "source": "watch",                      // "watch" | "web"
  "meta": { "batteryPercent": 81 }        // optional, free-form
}
```

Response `200`/`201` — the materialized session (camelCase):
```json
{
  "id": "22222222-2222-2222-2222-222222222222",
  "userId": "00000000-0000-0000-0000-000000000001",
  "typeId": "11111111-1111-1111-1111-111111111111",
  "startTime": "2026-08-18T09:30:00Z",
  "endTime": null,
  "status": "active",
  "createdAt": "2026-08-18T09:30:02Z",
  "updatedAt": "2026-08-18T09:30:02Z"
}
```

### `GET /api/v1/events/markers?from=&to=&limit=`

Marker event stream for the user (for retro-analysis / UI).
```json
{ "markers": [ { "id": "...", "userId": "...", "sessionId": "...", "kind": "start", "occurredAt": "...", "source": "watch", "meta": null, "createdAt": "..." } ] }
```

---

## 3. Agoge Types CRUD

### `GET /api/v1/agoge-types`
```json
{ "types": [ { "id": "...", "name": "Strength", "colorCode": "#E53935", "icon": "dumbbell", "createdAt": "..." } ] }
```

### `POST /api/v1/agoge-types` — body `{ "name": "Yoga", "colorCode": "#8B0000", "icon": "lotus" }`
### `PUT /api/v1/agoge-types/{id}` — partial update, same fields
### `DELETE /api/v1/agoge-types/{id}` — sessions referencing it become Undefined (`typeId` nulled)

---

## 4. Agoge Sessions CRUD

### `GET /api/v1/agoge-sessions?status=active&from=&to=&limit=`
```json
{ "sessions": [ /* AgogeSession as above */ ] }
```

### `POST /api/v1/agoge-sessions` — retroactive creation from the web UI
```jsonc
{ "typeId": "11111111-...", "startTime": "2026-08-18T08:00:00Z", "endTime": "2026-08-18T09:00:00Z" }
// endTime omitted => status "active"
```

### `PATCH /api/v1/agoge-sessions/{id}` — close or edit
```jsonc
{ "endTime": "2026-08-18T09:15:00Z" }   // closes an open session
// also: { "typeId": ..., "status": "active|closed" }
```

### `DELETE /api/v1/agoge-sessions/{id}`

---

## 5. Timeline (web UI)

### `GET /api/v1/timeline?from=<ISO>&to=<ISO>&bucket=1 minute`

Server-side downsampling with TimescaleDB `time_bucket`. `bucket` is any
Postgres interval string (`10 seconds`, `1 hour`, `1 day`); defaults to
`1 minute`. Rejects buckets that would return > 2000 points (browser-lag
guard) with a hint in the error message. Max range 366 days.

Response — `points[i].ts` is epoch **ms**:
```jsonc
{
  "bucket": "1 minute",
  "points": [
    { "ts": 1784716800000, "heartRate": 122.5, "steps": 40, "activeCalories": 3.1 }
  ],
  "sessions": [ /* AgogeSession, only those overlapping [from, to) */ ]
}
```

Raw data is NEVER locked to sessions: association happens purely by
time-range overlap at query time, so retro-analysis (rep detection, rest
periods) can re-process the raw stream freely.

---

## 6. Settings (stored in the DB — no second volume)

### `GET /api/v1/settings`
```json
{ "settings": { "series": { "heartRate": true, "steps": true, "calories": true }, "rangeDays": 7 } }
```

### `PUT /api/v1/settings` — body `{ "settings": { ... } }`

Free-form JSONB per user; unknown keys are preserved. The web UI persists
series visibility and the timeline range here.

## 7. Health

### `GET /healthz` → `200 "ok"` (no auth, for orchestration probes)
