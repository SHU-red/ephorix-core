-- v4: normalized multi-source measurement store + derived metrics.
--
-- raw_health_data stays as the Pebble-optimized fast path for the timeline;
-- `measurements` is the cross-source normalized truth that every source
-- adapter (Pebble, Fitbit, Garmin, Apple Health, manual) converges on.
-- Derived metrics (body battery, workout detection, nutrition) read from
-- `measurements` so they work for any source, not just Pebble.

-- ---------------------------------------------------------------------------
-- Measurements: long-form (ts, source, device, metric, value, unit).
-- One row per (metric, timestamp); sources push whatever signals they have.
-- ---------------------------------------------------------------------------
CREATE TABLE measurements (
    ts        TIMESTAMPTZ NOT NULL,
    user_id   UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source    TEXT        NOT NULL,              -- pebble | fitbit | garmin | apple_health | manual
    device_id TEXT,                              -- source device identifier
    metric    TEXT        NOT NULL,              -- heart_rate | steps | active_calories | sleep_seconds | ...
    value     DOUBLE PRECISION NOT NULL,
    unit      TEXT,
    meta      JSONB       NOT NULL DEFAULT '{}'::jsonb
);

SELECT create_hypertable(
    'measurements',
    'ts',
    chunk_time_interval => INTERVAL '1 day'
);

CREATE INDEX idx_measurements_user_metric_time
    ON measurements (user_id, metric, ts DESC);

-- ---------------------------------------------------------------------------
-- Nutrition log: food (kcal) and water (ml) intake events. Also mirrored into
-- `measurements` (water_ml / food_kcal) for a single normalized view.
-- ---------------------------------------------------------------------------
CREATE TABLE nutrition_log (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT        NOT NULL CHECK (kind IN ('water', 'food')),
    amount      DOUBLE PRECISION NOT NULL,       -- ml for water, kcal for food
    consumed_at TIMESTAMPTZ NOT NULL,
    note        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nutrition_user_time
    ON nutrition_log (user_id, consumed_at DESC);

-- ---------------------------------------------------------------------------
-- Body energy ("body battery"): per-day score derived from sleep (recharge)
-- vs activity (drain). Computed on demand, cached here.
-- ---------------------------------------------------------------------------
CREATE TABLE body_energy (
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day        DATE        NOT NULL,
    score      DOUBLE PRECISION NOT NULL,        -- 0..100
    recharge   DOUBLE PRECISION NOT NULL DEFAULT 0,
    drain      DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, day)
);

-- ---------------------------------------------------------------------------
-- Workout detections: contiguous high-effort windows found in the normalized
-- stream (elevated HR / movement intensity), independent of the manual
-- start/stop Agoge markers.
-- ---------------------------------------------------------------------------
CREATE TABLE workout_detections (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id     UUID        REFERENCES agoge_sessions(id) ON DELETE SET NULL,
    detected_start TIMESTAMPTZ NOT NULL,
    detected_end   TIMESTAMPTZ NOT NULL,
    confidence     DOUBLE PRECISION NOT NULL,
    metrics        JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_workout_detections_user_time
    ON workout_detections (user_id, detected_start DESC);
