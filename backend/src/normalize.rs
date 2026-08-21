//! Normalization layer: every source converges on the canonical
//! `measurements` long-form schema `(ts, source, device_id, metric, value,
//! unit)`. Source adapters map their native field names/units to these
//! canonical metric names; derived metrics then read one table regardless of
//! where the data came from.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Canonical metric names. Keep this the single vocabulary for the normalized
/// store so cross-source aggregation never relies on adapter-specific keys.
pub const METRIC_HEART_RATE: &str = "heart_rate";
pub const METRIC_STEPS: &str = "steps";
pub const METRIC_ACTIVE_CALORIES: &str = "active_calories";
pub const METRIC_SLEEP_SECONDS: &str = "sleep_seconds";
pub const METRIC_RESTFUL_SLEEP_SECONDS: &str = "restful_sleep_seconds";
pub const METRIC_DISTANCE_M: &str = "distance_m";
pub const METRIC_ACTIVE_SECONDS: &str = "active_seconds";
pub const METRIC_RESTING_KCAL: &str = "resting_kcal";
pub const METRIC_MOVEMENT_INTENSITY: &str = "movement_intensity";
pub const METRIC_REPS: &str = "reps";
pub const METRIC_WATER_ML: &str = "water_ml";
pub const METRIC_FOOD_KCAL: &str = "food_kcal";
pub const METRIC_HRV: &str = "hrv";
pub const METRIC_RESTING_HR: &str = "resting_hr";
pub const METRIC_PROTEIN_G: &str = "protein_g";
pub const METRIC_CARBS_G: &str = "carbs_g";
pub const METRIC_FAT_G: &str = "fat_g";

/// One normalized value.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub ts: DateTime<Utc>,
    pub metric: String,
    pub value: f64,
    pub unit: String,
}

impl Measurement {
    pub fn new(ts: DateTime<Utc>, metric: &str, value: f64, unit: &str) -> Self {
        Measurement {
            ts,
            metric: metric.to_string(),
            value,
            unit: unit.to_string(),
        }
    }
}

/// Inserts a batch of normalized measurements in a single transaction.
///
/// Deduplication is enforced by the unique index `(user_id, metric, ts)`
/// (see migration 0010): a row that already exists for the same user, metric
/// and timestamp is skipped via `ON CONFLICT DO NOTHING`, so re-pushing the
/// same batch never duplicates data. Returns the number of rows actually
/// inserted (from the Postgres rows-affected counts), not the input length.
pub async fn insert_measurements(
    pool: &PgPool,
    user_id: Uuid,
    source: &str,
    device_id: Option<&str>,
    rows: &[Measurement],
) -> Result<usize, sqlx::Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut inserted = 0usize;
    for r in rows {
        let res = sqlx::query(
            "INSERT INTO measurements (ts, user_id, source, device_id, metric, value, unit)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, metric, ts) DO NOTHING",
        )
        .bind(r.ts)
        .bind(user_id)
        .bind(source)
        .bind(device_id)
        .bind(&r.metric)
        .bind(r.value)
        .bind(&r.unit)
        .execute(&mut *tx)
        .await?;
        inserted += res.rows_affected() as usize;
    }
    tx.commit().await?;
    Ok(inserted)
}

/// True when `metric` is one of the canonical metric names. This is the
/// single gate the import endpoint uses to decide what the normalized store
/// will accept.
pub fn is_canonical_metric(metric: &str) -> bool {
    metric == METRIC_HEART_RATE
        || metric == METRIC_STEPS
        || metric == METRIC_ACTIVE_CALORIES
        || metric == METRIC_SLEEP_SECONDS
        || metric == METRIC_RESTFUL_SLEEP_SECONDS
        || metric == METRIC_DISTANCE_M
        || metric == METRIC_ACTIVE_SECONDS
        || metric == METRIC_RESTING_KCAL
        || metric == METRIC_MOVEMENT_INTENSITY
        || metric == METRIC_REPS
        || metric == METRIC_WATER_ML
        || metric == METRIC_FOOD_KCAL
        || metric == METRIC_HRV
        || metric == METRIC_RESTING_HR
        || metric == METRIC_PROTEIN_G
        || metric == METRIC_CARBS_G
        || metric == METRIC_FAT_G
}

/// Generic import adapter: validates one flat, timestamped sample from any
/// source adapter (see `docs/import-adapter.md` for the field mappings).
/// Returns `None` when the metric is not canonical or the value is not
/// finite — the importer counts those as skipped. `source`, `device_id`,
/// and `meta` are carried through for source-specific unit coercion as the
/// vocabulary grows; today they are not stored on the measurement.
pub fn normalize_import(
    ts: DateTime<Utc>,
    source: &str,
    device_id: Option<&str>,
    metric: &str,
    value: f64,
    unit: Option<&str>,
    meta: Option<&serde_json::Value>,
) -> Option<Measurement> {
    let _ = (source, device_id, meta);
    let metric = metric.trim();
    if !is_canonical_metric(metric) || !value.is_finite() {
        return None;
    }
    let unit = unit.map(str::trim).unwrap_or("");
    Some(Measurement::new(ts, metric, value, unit))
}

/// Pebble adapter: maps a PebbleKit health snapshot to canonical metrics.
/// Returns only the signals the watch actually reported (all optional).
#[allow(clippy::too_many_arguments)]
pub fn normalize_pebble(
    ts: DateTime<Utc>,
    heart_rate: Option<i16>,
    steps: Option<i32>,
    active_calories: Option<f32>,
    sleep_seconds: Option<i32>,
    restful_sleep_seconds: Option<i32>,
    distance_m: Option<f32>,
    active_seconds: Option<i32>,
    resting_kcal: Option<f32>,
    movement_intensity: Option<f32>,
    reps: Option<i32>,
) -> Vec<Measurement> {
    let mut out = Vec::with_capacity(10);
    if let Some(v) = heart_rate.filter(|v| *v > 0) {
        out.push(Measurement::new(ts, METRIC_HEART_RATE, v as f64, "bpm"));
    }
    if let Some(v) = steps.filter(|v| *v >= 0) {
        out.push(Measurement::new(ts, METRIC_STEPS, v as f64, "count"));
    }
    if let Some(v) = active_calories.filter(|v| *v > 0.0) {
        out.push(Measurement::new(ts, METRIC_ACTIVE_CALORIES, v as f64, "kcal"));
    }
    if let Some(v) = sleep_seconds.filter(|v| *v >= 0) {
        out.push(Measurement::new(ts, METRIC_SLEEP_SECONDS, v as f64, "s"));
    }
    if let Some(v) = restful_sleep_seconds.filter(|v| *v >= 0) {
        out.push(Measurement::new(ts, METRIC_RESTFUL_SLEEP_SECONDS, v as f64, "s"));
    }
    if let Some(v) = active_seconds.filter(|v| *v >= 0) {
        out.push(Measurement::new(ts, METRIC_ACTIVE_SECONDS, v as f64, "s"));
    }
    if let Some(v) = resting_kcal.filter(|v| *v >= 0.0) {
        out.push(Measurement::new(ts, METRIC_RESTING_KCAL, v as f64, "kcal"));
    }
    if let Some(v) = distance_m.filter(|v| *v > 0.0) {
        out.push(Measurement::new(ts, METRIC_DISTANCE_M, v as f64, "m"));
    }
    if let Some(v) = movement_intensity.filter(|v| *v > 0.0) {
        out.push(Measurement::new(ts, METRIC_MOVEMENT_INTENSITY, v as f64, "au"));
    }
    if let Some(v) = reps.filter(|v| *v >= 0) {
        out.push(Measurement::new(ts, METRIC_REPS, v as f64, "count"));
    }
    out
}
