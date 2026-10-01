-- v15: idempotent marker ingestion.
--
-- The phone JS queue retries a marker on timeout/5xx; the marker path had no
-- dedupe key, so a retried Start/Stop inserted a second marker (and a second
-- session) alongside the first. The natural key of a marker event is
-- (user_id, kind, occurred_at): the same watch, kind and timestamp is always
-- the same physical event, never a distinct one.
--
-- Before creating the unique index, dedupe existing rows: for each key, keep
-- the earliest row (smallest ctid == earliest physical insertion) and delete
-- the later duplicates. Both statements are safe to re-run (the DELETE is a
-- no-op once duplicates are gone; CREATE INDEX uses IF NOT EXISTS).

-- agoge_markers: one row per (user_id, kind, occurred_at), keep earliest.
DELETE FROM agoge_markers a
USING agoge_markers b
WHERE a.ctid > b.ctid
  AND a.user_id = b.user_id
  AND a.kind = b.kind
  AND a.occurred_at = b.occurred_at;

CREATE UNIQUE INDEX IF NOT EXISTS idx_agoge_markers_natural
    ON agoge_markers (user_id, kind, occurred_at);

-- Supports the "one open Agoge at a time" policy: finding a user's stale open
-- session (auto-close on a new Start, Stop landing on the latest open one).
CREATE INDEX IF NOT EXISTS idx_agoge_sessions_user_status
    ON agoge_sessions (user_id, status);
