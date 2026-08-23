-- Per-Agoge HR sampling interval (seconds): while an Agoge of this type is
-- active the watch arms its heart-rate sampling request at this cadence,
-- overriding the global Auto Push setting. 0 = OS automatic; the OS clamps
-- requests above 600 s (10 min), so 15/30/60-min choices run at the cap.
ALTER TABLE agoge_types
    ADD COLUMN hr_sampling_interval INT NOT NULL DEFAULT 60;
