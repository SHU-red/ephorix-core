-- v10: idempotent ingestion.
--
-- Late/duplicate re-pushes (watch/JS reconnect after days offline) previously
-- inserted duplicate rows on every retry. This migration makes the dedup keys
-- enforceable:
--   measurements   unique on (user_id, metric, ts)
--   raw_health_data unique on (user_id, timestamp)
-- TimescaleDB rule: unique indexes on hypertables MUST include the
-- partitioning column (ts / timestamp) — both keys do.
--
-- Before creating the unique indexes, dedupe existing rows: for each key,
-- keep the earliest row (smallest ctid == earliest physical insertion) and
-- delete the later duplicates. Both statements are safe to re-run (the DELETE
-- is a no-op once duplicates are gone; CREATE INDEX uses IF NOT EXISTS).

-- measurements: one row per (user_id, metric, ts), keep earliest.
DELETE FROM measurements a
USING measurements b
WHERE a.ctid > b.ctid
  AND a.user_id = b.user_id
  AND a.metric = b.metric
  AND a.ts = b.ts;

CREATE UNIQUE INDEX IF NOT EXISTS idx_measurements_dedup
    ON measurements (user_id, metric, ts);

-- raw_health_data: one row per (user_id, timestamp), keep earliest.
DELETE FROM raw_health_data a
USING raw_health_data b
WHERE a.ctid > b.ctid
  AND a.user_id = b.user_id
  AND a.timestamp = b.timestamp;

CREATE UNIQUE INDEX IF NOT EXISTS idx_raw_health_dedup
    ON raw_health_data (user_id, timestamp);
