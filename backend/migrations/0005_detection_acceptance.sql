-- v5: workout-detection acceptance + proposed activity type.
--
-- Detections start as 'proposed'. The user accepts (→ materialized as an
-- agoge_session and linked) or rejects them. `proposed_type_id` is the
-- activity type inferred from the user's own historical sessions, so the
-- system learns which signal pattern maps to which Agoge type.

ALTER TABLE workout_detections
    ADD COLUMN status TEXT NOT NULL DEFAULT 'proposed'
        CHECK (status IN ('proposed', 'accepted', 'rejected')),
    ADD COLUMN proposed_type_id UUID REFERENCES agoge_types(id) ON DELETE SET NULL;

-- One proposal per (user, window start) so re-running detection refreshes
-- instead of duplicating.
CREATE UNIQUE INDEX idx_workout_detections_user_start
    ON workout_detections (user_id, detected_start);
