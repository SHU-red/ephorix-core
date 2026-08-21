//! Generic data import (`POST /api/v1/import`): one JSON payload of flat,
//! timestamped samples from any source adapter (Google Health Connect export,
//! Apple Health XML, Garmin CSV, …). Every sample is normalized through the
//! canonical metric vocabulary; invalid samples are skipped and reported,
//! never aborting the whole batch. See `docs/import-adapter.md`.

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
    normalize::{insert_measurements, is_canonical_metric, normalize_import, Measurement},
};

/// Canonical source names accepted by the import endpoint.
pub const CANONICAL_SOURCES: [&str; 8] = [
    "pebble",
    "fitbit",
    "garmin",
    "apple_health",
    "manual",
    "health_connect",
    "csv",
    "gpx",
];

/// Cap on per-sample error strings returned in one response (skipped keeps
/// counting beyond this).
const MAX_IMPORT_ERRORS: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSample {
    /// RFC-3339 / ISO-8601 timestamp, e.g. `2026-08-21T12:00:00Z`.
    pub timestamp: String,
    /// Canonical metric name (see `normalize` for the vocabulary).
    pub metric: String,
    pub value: f64,
    #[serde(default)]
    pub unit: Option<String>,
    /// Free-form source metadata; carried for future coercion, not persisted.
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBody {
    /// One of `CANONICAL_SOURCES`.
    pub source: String,
    #[serde(default)]
    pub device_id: Option<String>,
    pub samples: Vec<ImportSample>,
}

pub async fn import(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<ImportBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let source = body.source.trim();
    if !CANONICAL_SOURCES.contains(&source) {
        return Err(ApiError::BadRequest(format!(
            "unknown source '{source}'; expected one of: pebble, fitbit, garmin, apple_health, manual, health_connect, csv, gpx"
        )));
    }
    if body.samples.is_empty() {
        return Err(ApiError::BadRequest("samples must not be empty".to_string()));
    }

    let mut rows: Vec<Measurement> = Vec::with_capacity(body.samples.len());
    let mut skipped = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (i, s) in body.samples.iter().enumerate() {
        let ts = DateTime::parse_from_rfc3339(&s.timestamp)
            .ok()
            .map(|dt| dt.with_timezone(&Utc));
        let sample = ts.and_then(|ts| {
            normalize_import(
                ts,
                source,
                body.device_id.as_deref(),
                &s.metric,
                s.value,
                s.unit.as_deref(),
                s.meta.as_ref(),
            )
        });
        match sample {
            Some(m) => rows.push(m),
            None => {
                skipped += 1;
                if errors.len() < MAX_IMPORT_ERRORS {
                    let msg = if ts.is_none() {
                        format!("sample {i}: invalid timestamp '{}'", s.timestamp)
                    } else if is_canonical_metric(s.metric.trim()) {
                        format!("sample {i}: non-finite value for '{}'", s.metric.trim())
                    } else {
                        format!("sample {i}: unknown metric '{}'", s.metric.trim())
                    };
                    errors.push(msg);
                }
            }
        }
    }

    let inserted = insert_measurements(&pool, user.0, source, body.device_id.as_deref(), &rows)
        .await?;

    Ok(Json(json!({
        "inserted": inserted,
        "skipped": skipped,
        "errors": errors,
    })))
}
