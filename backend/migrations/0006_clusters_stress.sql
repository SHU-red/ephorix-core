-- v6: Agoge type clusters (signal-driven evaluation) + stress in body energy.

-- Each Agoge type belongs to a cluster that tells the watch + backend which
-- sensor tells the truth for that activity (see docs/clusters):
--   distance    steady-state: steps/distance primary, HR secondary
--   repetitive  accel bursts: reps/intensity primary, HR spikes secondary
--   dynamic     erratic: HR variability + accel intensity, "bursts" not reps
--   circuit     distance + repetitive alternating (Hyrox/CrossFit)
--   recovery    low intensity: duration + low HR only
--   mixed       fallback (default): HR + steps, shallow evaluation
ALTER TABLE agoge_types
    ADD COLUMN category TEXT NOT NULL DEFAULT 'mixed'
        CHECK (category IN ('distance', 'repetitive', 'dynamic', 'circuit', 'recovery', 'mixed'));

-- Stress: a 0..100 physiological-load score (HR-elevation based; no HRV on
-- the watch). Discharges the body battery alongside activity drain.
ALTER TABLE body_energy
    ADD COLUMN stress DOUBLE PRECISION NOT NULL DEFAULT 0;
