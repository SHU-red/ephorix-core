-- v12: server-side AI/action audit log with one-step revert.
--
-- Every reversible mutation that can be applied by hand or from a PYTHIA
-- proposal (settings PUT, nutrition POST, measurements POST) writes one row
-- here inside the same transaction: the mutation's payload (for the list
-- view) plus the exact `undo` recipe to reverse it. `reverted_at` is set by
-- POST /api/v1/actions/{id}/revert; a row with `reverted_at` set cannot be
-- reverted again (409).

CREATE TABLE ai_action_log (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT        NOT NULL,          -- settings | nutrition | measurement
    target      TEXT        NOT NULL,          -- e.g. "settings", "nutrition", "weight_kg"
    payload     JSONB       NOT NULL,          -- what the mutation did (list view)
    undo        JSONB       NOT NULL,          -- recipe to reverse the mutation
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    reverted_at TIMESTAMPTZ
);

CREATE INDEX idx_ai_action_log_user_time
    ON ai_action_log (user_id, created_at DESC);
