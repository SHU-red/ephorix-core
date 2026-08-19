//! Generic multi-source ingestion (`POST /api/v1/ingest`) and the normalized
//! query (`GET /api/v1/measurements`). Any device in any format lands here
//! after its adapter maps native fields to canonical metrics — the same
//! vocabulary Pebble uses internally.

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    normalize::{insert_measurements, Measurement},
};

const MAX_INGEST_ROWS: usize = 5000;
const MAX_QUERY_DAYS: i64 = 366;

#[derive(Debug, Deserialize)]
pub struct IngestMeasurement {
    pub metric: String,
    pub value: f64,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestSample {
    pub timestamp: DateTime<Utc>,
    pub measurements: Vec<IngestMeasurement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestBatch {
    /// Canonical source name: pebble | fitbit | garmin | apple_health | manual | ...
    pub source: String,
    #[serde(default)]
    pub device_id: Option<String>,
    pub samples: Vec<IngestSample>,
}

pub async fn ingest(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(batch): Json<IngestBatch>,
) -> ApiResult<Json<serde_json::Value>> {
    if batch.source.trim().is_empty() {
        return Err(ApiError::BadRequest("source must not be empty".to_string()));
    }
    if batch.samples.is_empty() {
        return Err(ApiError::BadRequest("samples must not be empty".to_string()));
    }

    let mut rows: Vec<Measurement> = Vec::new();
    for s in &batch.samples {
        if s.measurements.is_empty() {
            continue;
        }
        for m in &s.measurements {
            if m.metric.trim().is_empty() || !m.value.is_finite() {
                return Err(ApiError::BadRequest("invalid metric name or value".to_string()));
            }
            rows.push(Measurement::new(
                s.timestamp,
                m.metric.trim(),
                m.value,
                m.unit.as_deref().unwrap_or(""),
            ));
        }
    }
    if rows.len() > MAX_INGEST_ROWS {
        return Err(ApiError::BadRequest(format!(
            "batch exceeds max of {MAX_INGEST_ROWS} measurements"
        )));
    }

    let inserted = insert_measurements(
        &pool,
        user.0,
        batch.source.trim(),
        batch.device_id.as_deref(),
        &rows,
    )
    .await?;

    Ok(Json(json!({ "inserted": inserted })))
}

#[derive(Debug, Deserialize)]
pub struct MeasurementsQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementPoint {
    pub ts: f64,
    pub source: String,
    pub metric: String,
    pub value: f64,
    pub unit: Option<String>,
}

pub async fn list_measurements(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<MeasurementsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(MAX_QUERY_DAYS) {
        return Err(ApiError::BadRequest(format!("range exceeds {MAX_QUERY_DAYS} days")));
    }

    // Single normalized view across all sources; optionally narrowed by
    // metric and/or source.
    let rows: Vec<MeasurementPoint> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM ts) * 1000)::float8 AS ts,
            source,
            metric,
            value,
            unit
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND ($4::text IS NULL OR metric = $4)
           AND ($5::text IS NULL OR source = $5)
         ORDER BY ts
         LIMIT 20000",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .bind(&q.metric)
    .bind(&q.source)
    .fetch_all(&pool)
    .await?;

    Ok(Json(json!({ "points": rows })))
}
