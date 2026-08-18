-- EphoriX schema v1
-- Multi-user from day one: every user-scoped table carries user_id.

-- ---------------------------------------------------------------------------
-- Users: the POC authenticates with fixed tokens (X-EphoriX-Token header),
-- but the data model is ready for real auth (swap token lookup for OIDC/JWT
-- subject mapping without touching any other table).
-- ---------------------------------------------------------------------------
CREATE TABLE users (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    token        TEXT        NOT NULL UNIQUE,
    display_name TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------------------
-- Agoge Types: reference data (globally shared; not user-scoped).
-- ---------------------------------------------------------------------------
CREATE TABLE agoge_types (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL,
    color_code TEXT        NOT NULL DEFAULT '#E53935',
    icon       TEXT        NOT NULL DEFAULT 'dumbbell',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------------------
-- Agoge Sessions: discrete training events. Derived state, materialized from
-- Start/Stop marker events but also editable manually via CRUD (retroactive
-- creation / closing from the web UI).
-- type_id NULL == "Undefined Agoge" (no type selected on the watch).
-- status: 'active' (open, end_time NULL) | 'closed'
-- ---------------------------------------------------------------------------
CREATE TABLE agoge_sessions (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type_id    UUID        REFERENCES agoge_types(id) ON DELETE SET NULL,
    start_time TIMESTAMPTZ NOT NULL,
    end_time   TIMESTAMPTZ,
    status     TEXT        NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_agoge_sessions_user_time
    ON agoge_sessions (user_id, start_time DESC);

-- ---------------------------------------------------------------------------
-- Marker events: the raw event stream (Start_Marker / Stop_Marker) pushed by
-- the watch or the web UI. Sessions are derived; markers are the source of
-- truth for retro-analysis.
-- ---------------------------------------------------------------------------
CREATE TABLE agoge_markers (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id  UUID        REFERENCES agoge_sessions(id) ON DELETE SET NULL,
    kind        TEXT        NOT NULL CHECK (kind IN ('start', 'stop')),
    occurred_at TIMESTAMPTZ NOT NULL,
    source      TEXT        NOT NULL DEFAULT 'watch', -- watch | web
    meta        JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_agoge_markers_user_time
    ON agoge_markers (user_id, occurred_at DESC);

-- ---------------------------------------------------------------------------
-- Raw_Health_Data: high-frequency sensor metrics. True TimescaleDB
-- hypertable, partitioned on timestamp. NOT linked to sessions — raw data is
-- associated with Agoge sessions via time-range joins at query time.
-- ---------------------------------------------------------------------------
CREATE TABLE raw_health_data (
    timestamp       TIMESTAMPTZ NOT NULL,
    user_id         UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    heart_rate      SMALLINT,              -- BPM, nullable (sleep / off-wrist)
    steps           INTEGER,               -- delta within the bucket
    active_calories REAL                   -- kcal delta within the bucket
);

SELECT create_hypertable(
    'raw_health_data',
    'timestamp',
    chunk_time_interval => INTERVAL '1 day'
);

CREATE INDEX idx_raw_health_user_time
    ON raw_health_data (user_id, timestamp DESC);
