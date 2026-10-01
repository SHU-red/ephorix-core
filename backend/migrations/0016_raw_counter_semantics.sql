-- v16: correct the raw_health_data counter semantics in the catalog.
--
-- 0001 described `steps` / `active_calories` as "delta within the bucket".
-- They are not: the watch pushes CUMULATIVE local-day totals
-- (`health_service_sum(metric, start_of_today, now)`) on every snapshot, and the
-- read path (routes/timeline.rs) re-derives per-bucket deltas. Summing these
-- rows over-counts — the mistake that inflated the 2026-08-29 timeline ~6x
-- (49,849 vs 8,402 steps) once an HR-only row with NULL counters sat between
-- two snapshots.
--
-- 0001 is already applied everywhere and sqlx validates its checksum, so the
-- correction lands here as a catalog comment rather than as an edit to 0001.
-- COMMENT ON is idempotent.

COMMENT ON COLUMN raw_health_data.steps IS
    'CUMULATIVE steps since the watch''s local midnight (NOT a per-bucket delta); re-derive deltas on read';

COMMENT ON COLUMN raw_health_data.active_calories IS
    'CUMULATIVE kcal since the watch''s local midnight (NOT a per-bucket delta); re-derive deltas on read';

COMMENT ON COLUMN raw_health_data.heart_rate IS
    'Instantaneous (filtered) HR sample, BPM; NULL on HR-only history pushes and while off-wrist';
