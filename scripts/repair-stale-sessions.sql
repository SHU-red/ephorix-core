-- EphoriX: close Agoge sessions left `active` by a Stop that never arrived
-- (watch offline, app killed mid-workout, or a retried marker lost before the
-- idempotency fix in migration 0015). New Starts now self-heal going forward;
-- this closes the pre-existing orphans.
--
-- Review step 1 first. Step 2 is the repair. Both are safe to re-run.
-- Run against the app database, e.g.:
--   sudo docker exec -i ephorix-db psql -U ephorix -d ephorix -f - < repair-stale-sessions.sql

-- 1. Inspect what would be closed.
SELECT id, user_id, start_time, now() - start_time AS age
FROM agoge_sessions
WHERE status = 'active' AND start_time < now() - interval '12 hours'
ORDER BY start_time;

-- 2. Close them. end_time is the last raw sample at/after the session start
--    (the best evidence of when the workout actually stopped), else the start
--    itself. The 12h guard keeps a genuinely-running workout untouched.
UPDATE agoge_sessions s
SET status     = 'closed',
    end_time   = COALESCE(
        (SELECT MAX(r.timestamp)
           FROM raw_health_data r
          WHERE r.user_id = s.user_id
            AND r.timestamp >= s.start_time),
        s.start_time),
    updated_at = now()
WHERE s.status = 'active'
  AND s.start_time < now() - interval '12 hours';
