-- v11: workout summary columns on agoge_sessions + day-aggregate index.
--
-- The watch's Stop marker carries a workout summary (duration, kcal, avg HR,
-- reps, intensity, distance) computed from its session ring + accelerometer.
-- Those values are written onto the closed session row (all nullable; a Stop
-- without a summary leaves them NULL).

ALTER TABLE agoge_sessions
    ADD COLUMN duration_sec INT,
    ADD COLUMN workout_kcal REAL,
    ADD COLUMN avg_hr INT,
    ADD COLUMN reps INT,
    ADD COLUMN movement_intensity REAL,
    ADD COLUMN distance_m REAL;

-- Day aggregates (POST /api/v1/health/days) query by (user_id, metric, ts).
-- Migration 0010 already created the UNIQUE idx_measurements_dedup on these
-- columns; the dedicated non-unique index keeps the name stable for the
-- ingest paths regardless of the dedup index's future shape.
CREATE INDEX IF NOT EXISTS idx_measurements_days
    ON measurements (user_id, metric, ts);
