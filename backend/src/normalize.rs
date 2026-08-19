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
pub const METRIC_RESTING_HEART_RATE: &str = "resting_heart_rate";
pub const METRIC_DISTANCE_M: &str = "distance_m";
pub const METRIC_MOVEMENT_INTENSITY: &str = "movement_intensity";
pub const METRIC_REPS: &str = "reps";
pub const METRIC_WATER_ML: &str = "water_ml";
pub const METRIC_FOOD_KCAL: &str = "food_kcal";

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
    for r in rows {
        sqlx::query(
            "INSERT INTO measurements (ts, user_id, source, device_id, metric, value, unit)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
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
    }
    tx.commit().await?;
    Ok(rows.len())
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
    resting_heart_rate: Option<i16>,
    distance_m: Option<f32>,
    movement_intensity: Option<f32>,
    reps: Option<i32>,
) -> Vec<Measurement> {
    let mut out = Vec::with_capacity(9);
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
    if let Some(v) = resting_heart_rate.filter(|v| *v > 0) {
        out.push(Measurement::new(ts, METRIC_RESTING_HEART_RATE, v as f64, "bpm"));
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
