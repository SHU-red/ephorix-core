//! High-throughput ingestion of batched raw health metrics (PebbleKit JS
//! pushes on reconnect). One transaction per batch.

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

    Ok(Json(json!({ "inserted": batch.samples.len() })))
}
