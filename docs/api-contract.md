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

High-throughput push of batched raw sensor metrics. One transaction per batch.
Max 1000 samples per batch. Idempotent-ish: plain inserts (dedupe by the watch
queue, which retries only until acknowledged).

```jsonc
{
  "deviceId": "pebble:6b3a7f",            // informational, optional
  "batchedAt": "2026-08-18T10:00:00Z",    // optional, queue flush time
  "samples": [
    {
      "timestamp": "2026-08-18T09:59:00Z", // REQUIRED
      "heartRate": 128,                    // optional, BPM, null while off-wrist
      "steps": 42,                         // optional, step delta in bucket
      "activeCalories": 3.2                // optional, kcal delta in bucket
    }
  ]
}
```

Response `201`:
```json
{ "inserted": 2 }
```

---

## 2. Marker events (Start_Marker / Stop_Marker)

### `POST /api/v1/events/marker`

Discrete event from the watch (or web). The backend materializes the session:
`start` → creates an `active` session; `stop` → closes it (by `sessionId` or the
latest open one). `pause` / `resume` are informational rest-period markers —
recorded against the open session, never closing it. Unknown/missing type →
session recorded as **Undefined Agoge** (`typeId: null`).

```jsonc
{
  "kind": "start",                        // "start" | "stop" | "pause" | "resume"
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
