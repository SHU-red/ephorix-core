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
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    normalize::{insert_measurements, normalize_pebble},
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
    for s in &batch.samples {
        sqlx::query(
            "INSERT INTO raw_health_data (timestamp, user_id, heart_rate, steps, active_calories)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(s.timestamp)
        .bind(user.0)
        .bind(s.heart_rate)
        .bind(s.steps)
        .bind(s.active_calories)
        .execute(&mut *tx)
        .await?;
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
        "inserted": batch.samples.len(),
        "normalized": normalized_count,
    })))
}
