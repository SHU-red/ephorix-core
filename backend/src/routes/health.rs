//! High-throughput ingestion of batched raw health metrics (PebbleKit JS
//! pushes on reconnect). One transaction per batch.
//!
//! Two writes:
//!   1. `raw_health_data` — the Pebble-optimized fast path the timeline reads
//!      (heart_rate / steps / active_calories).
//!   2. `measurements` — the normalized cross-source store; every signal the
//!      watch reports (sleep, resting HR, distance, movement, reps, ...) lands
//!      here so derived metrics work on the full picture.

use axum::{
    extract::{Extension, State},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    normalize::{insert_measurements, normalize_pebble, Measurement},
};

pub const MAX_BATCH_SAMPLES: usize = 1000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSample {
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub heart_rate: Option<i16>,
    #[serde(default)]
    pub steps: Option<i32>,
    #[serde(default)]
    pub active_calories: Option<f32>,
    // --- expanded signals (all optional) ---
    #[serde(default)]
    pub sleep_seconds: Option<i32>,
    #[serde(default)]
    pub restful_sleep_seconds: Option<i32>,
    #[serde(default)]
    pub distance_m: Option<f32>,
    #[serde(default)]
    pub active_seconds: Option<i32>,
    #[serde(default)]
    pub resting_kcal: Option<f32>,
    #[serde(default)]
    pub movement_intensity: Option<f32>,
    #[serde(default)]
    pub reps: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthBatch {
    /// Watch identifier (Pebble MAC / UUID) — informational, for fleet
    /// debugging. Auth still comes from the token header.
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub batched_at: Option<DateTime<Utc>>,
    pub samples: Vec<HealthSample>,
}

pub async fn ingest_batch(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(batch): Json<HealthBatch>,
) -> ApiResult<Json<serde_json::Value>> {
    if batch.samples.is_empty() {
        return Err(ApiError::BadRequest("samples must not be empty".to_string()));
    }
    if batch.samples.len() > MAX_BATCH_SAMPLES {
        return Err(ApiError::BadRequest(format!(
            "batch exceeds max of {MAX_BATCH_SAMPLES} samples"
        )));
    }

    tracing::debug!(
        "ingesting {} samples from device {:?}, batched_at {:?}",
        batch.samples.len(),
        batch.device_id,
        batch.batched_at
    );

    let mut tx = pool.begin().await?;
    let mut raw_inserted = 0usize;
    for s in &batch.samples {
        let res = sqlx::query(
            "INSERT INTO raw_health_data (timestamp, user_id, heart_rate, steps, active_calories)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, timestamp) DO NOTHING",
        )
        .bind(s.timestamp)
        .bind(user.0)
        .bind(s.heart_rate)
        .bind(s.steps)
        .bind(s.active_calories)
        .execute(&mut *tx)
        .await?;
        raw_inserted += res.rows_affected() as usize;
    }
    tx.commit().await?;

    // Normalized mirror for every reported signal.
    let normalized: Vec<_> = batch
        .samples
        .iter()
        .flat_map(|s| {
            normalize_pebble(
                s.timestamp,
                s.heart_rate,
                s.steps,
                s.active_calories,
                s.sleep_seconds,
                s.restful_sleep_seconds,
                s.distance_m,
                s.active_seconds,
                s.resting_kcal,
                s.movement_intensity,
                s.reps,
            )
        })
        .collect();
    let normalized_count = insert_measurements(
        &pool,
        user.0,
        "pebble",
        batch.device_id.as_deref(),
        &normalized,
    )
    .await?;

    Ok(Json(json!({
        "inserted": raw_inserted + normalized_count,
        "normalized": normalized_count,
    })))
}

/// Maximum points in a per-session pulse series; longer windows are
/// bucketed down to keep the payload bounded.
pub const PULSE_SERIES_MAX: usize = 120;

/// Load one session window's heart-rate series from `raw_health_data` (the
/// fast path the timeline reads — NOT the `measurements` mirror, which may
/// lag or be incomplete for watch-pushed data) and derive avg/min/max plus
/// a series bucketed to at most [`PULSE_SERIES_MAX`] points.
///
/// `end` is exclusive, matching the `/stats` measurements rollup window.
/// The wire shape is `{"avgHr","minHr","maxHr","series"}` where the three
/// stats are `null` and `series` is empty when the window has no HR rows.
/// Bucket timestamps are the bucket-window start; `hr` is the rounded
/// bucket mean. Also serves as the stats handler's fallback average when
/// the stop-marker `avg_hr` is missing.
pub async fn session_pulse(
    pool: &PgPool,
    user_id: Uuid,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<serde_json::Value, sqlx::Error> {
    let rows: Vec<(DateTime<Utc>, i16)> = sqlx::query_as(
        "SELECT timestamp, heart_rate FROM raw_health_data
         WHERE user_id = $1 AND heart_rate IS NOT NULL
           AND timestamp >= $2 AND timestamp < $3
         ORDER BY timestamp",
    )
    .bind(user_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let avg_hr = if rows.is_empty() {
        None
    } else {
        Some(rows.iter().map(|(_, hr)| f64::from(*hr)).sum::<f64>() / rows.len() as f64)
    };
    let min_hr = rows.iter().map(|(_, hr)| *hr).min();
    let max_hr = rows.iter().map(|(_, hr)| *hr).max();

    let series: Vec<serde_json::Value> = if rows.len() <= PULSE_SERIES_MAX {
        rows.iter().map(|(t, hr)| json!({ "t": t, "hr": hr })).collect()
    } else {
        // Equal-width time slices over [start, end); each bucket holds the
        // mean of the samples that fall into it.
        let span_ms = (end - start).num_milliseconds().max(1) as f64;
        let bucket_ms = span_ms / PULSE_SERIES_MAX as f64;
        let mut buckets: Vec<(f64, usize)> = vec![(0.0, 0); PULSE_SERIES_MAX];
        for (t, hr) in &rows {
            let idx = (((*t - start).num_milliseconds() as f64) / bucket_ms).floor() as usize;
            let b = &mut buckets[idx.min(PULSE_SERIES_MAX - 1)];
            b.0 += f64::from(*hr);
            b.1 += 1;
        }
        buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.1 > 0)
            .map(|(i, b)| {
                json!({
                    "t": start + chrono::Duration::milliseconds((i as f64 * bucket_ms).round() as i64),
                    "hr": (b.0 / b.1 as f64).round() as i64,
                })
            })
            .collect()
    };

    Ok(json!({
        "avgHr": avg_hr,
        "minHr": min_hr,
        "maxHr": max_hr,
        "series": series,
    }))
}

/// Maximum calendar days accepted in one day-history payload (watch backfills
/// up to 7; the cap guards the endpoint against abuse).
pub const MAX_DAYS: usize = 31;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayValue {
    /// Calendar day, `YYYY-MM-DD` (UTC). The watch sends `"d"` (alias);
    /// `"date"` is the canonical wire name for API clients.
    #[serde(alias = "d")]
    pub date: String,
    #[serde(default)]
    pub steps: Option<i32>,
    #[serde(default)]
    pub active_kcal: Option<f32>,
    #[serde(default)]
    pub sleep_seconds: Option<i32>,
    #[serde(default)]
    pub restful_sleep_seconds: Option<i32>,
    #[serde(default)]
    pub distance_m: Option<f32>,
    #[serde(default)]
    pub active_seconds: Option<i32>,
    #[serde(default)]
    pub resting_kcal: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayBatch {
    /// Watch identifier (Pebble MAC / UUID) — informational, for fleet
    /// debugging. Auth still comes from the token header.
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub batched_at: Option<DateTime<Utc>>,
    pub days: Vec<DayValue>,
}

/// Daily aggregates (Pebble Health historical backfill). One row per
/// (day, metric) at the day's UTC midday anchor, normalized through the same
/// Pebble adapter as `ingest_batch` — identical canonical metric names, units
/// and zero/negative filtering — so day aggregates and raw samples share the
/// `measurements` vocabulary. Idempotent: re-posting the same day re-inserts
/// nothing (unique `(user_id, metric, ts)`, see migration 0010).
pub async fn ingest_days(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(batch): Json<DayBatch>,
) -> ApiResult<Json<serde_json::Value>> {
    if batch.days.is_empty() {
        return Err(ApiError::BadRequest("days must not be empty".to_string()));
    }
    if batch.days.len() > MAX_DAYS {
        return Err(ApiError::BadRequest(format!(
            "days exceeds max of {MAX_DAYS}"
        )));
    }

    tracing::debug!(
        "ingesting {} day aggregates from device {:?}, batched_at {:?}",
        batch.days.len(),
        batch.device_id,
        batch.batched_at
    );

    let mut rows: Vec<Measurement> = Vec::new();
    for d in &batch.days {
        let day = parse_day(&d.date)?;
        // Non-finite values are rejected outright; negatives follow the
        // shared adapter's filter (omitted) — day aggregates must stay in
        // the same shape the raw-sample path would have produced.
        for (name, v) in [
            ("activeKcal", d.active_kcal),
            ("distanceM", d.distance_m),
            ("restingKcal", d.resting_kcal),
        ] {
            if v.is_some_and(|v| !v.is_finite()) {
                return Err(ApiError::BadRequest(format!(
                    "{name} must be finite (day {})",
                    d.date
                )));
            }
        }
        let ts = day
            .and_hms_opt(12, 0, 0)
            .expect("midday anchor is always a valid time")
            .and_utc();
        rows.extend(normalize_pebble(
            ts,
            None, // no heart rate in day aggregates
            d.steps,
            d.active_kcal,
            d.sleep_seconds,
            d.restful_sleep_seconds,
            d.distance_m,
            d.active_seconds,
            d.resting_kcal,
            None, // no movement intensity in day aggregates
            None, // no reps in day aggregates
        ));
    }

    let inserted = insert_measurements(&pool, user.0, "pebble", batch.device_id.as_deref(), &rows)
        .await?;

    Ok(Json(json!({ "inserted": inserted })))
}

/// Strict `YYYY-MM-DD` (zero-padded, valid calendar date); the round-trip
/// rejects sloppy forms chrono would otherwise accept (`2026-1-5`).
fn parse_day(raw: &str) -> ApiResult<NaiveDate> {
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        ApiError::BadRequest(format!("date must be YYYY-MM-DD, got {raw:?}"))
    })?;
    if day.format("%Y-%m-%d").to_string() != raw {
        return Err(ApiError::BadRequest(format!(
            "date must be YYYY-MM-DD, got {raw:?}"
        )));
    }
    Ok(day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_batch_accepts_watch_d_key_and_api_date_key() {
        // Watch serializes day entries with "d"; API clients use "date".
        let watch: DayBatch = serde_json::from_value(json!({
            "days": [{
                "d": "2026-08-20",
                "steps": 12000,
                "activeKcal": 410.5,
            }]
        }))
        .unwrap();
        assert_eq!(watch.days[0].date, "2026-08-20");
        assert_eq!(watch.days[0].steps, Some(12000));
        assert_eq!(watch.days[0].active_kcal, Some(410.5));

        let api: DayBatch = serde_json::from_value(json!({
            "days": [{
                "date": "2026-08-20",
                "steps": 9000,
            }]
        }))
        .unwrap();
        assert_eq!(api.days[0].date, "2026-08-20");
        assert_eq!(api.days[0].steps, Some(9000));
    }

    #[test]
    fn parse_day_keeps_strict_round_trip() {
        assert!(parse_day("2026-08-20").is_ok());
        assert!(parse_day("2026-8-20").is_err()); // not zero-padded
        assert!(parse_day("2026-08-32").is_err()); // invalid calendar date
    }
}
