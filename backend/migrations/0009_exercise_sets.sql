-- v9: manual exercise sets for Agoge sessions.
-- Per-set rows (reps / weight / rest) so session stats (sets, totalReps,
-- volumeKg) and the session detail UI work on structured data instead of
-- the measurement stream. An "exercise" is the group of rows sharing an
-- exercise_name within a session.
CREATE TABLE exercise_sets (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id    UUID        NOT NULL REFERENCES agoge_sessions(id) ON DELETE CASCADE,
    user_id       UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    exercise_name TEXT        NOT NULL,
    set_number    INT         NOT NULL,
    reps          INT         NOT NULL,
    weight_kg     REAL,
    rest_sec      INT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_exercise_sets_session
    ON exercise_sets (session_id, created_at, set_number);
