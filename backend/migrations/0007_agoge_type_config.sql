-- v7: per-category "senseful" sub-settings for Agoge types.
-- Free-form, category-dependent JSON the backend stores opaquely and the
-- future workout evaluation consumes (e.g. targetDistanceM for distance,
-- targetReps/targetSets for repetitive, targetDurationSec for recovery).
ALTER TABLE agoge_types
    ADD COLUMN config JSONB NOT NULL DEFAULT '{}'::jsonb;
