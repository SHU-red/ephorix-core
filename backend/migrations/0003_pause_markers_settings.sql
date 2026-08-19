-- v3: pause/resume markers (rest-period events for retro-analysis) and
-- per-user settings stored in the DB (no second volume needed).

ALTER TABLE agoge_markers DROP CONSTRAINT agoge_markers_kind_check;
ALTER TABLE agoge_markers
    ADD CONSTRAINT agoge_markers_kind_check
    CHECK (kind IN ('start', 'stop', 'pause', 'resume'));

CREATE TABLE user_settings (
    user_id    UUID        PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    settings   JSONB       NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
