//! User-logged body measurements (weight, body-fat %).
//!
//! `POST /api/v1/measurements` inserts one row into the normalized
//! `measurements` store with source `user`, deduplicated by the unique
//! `(user_id, metric, ts)` key (migration 0010) — re-posting the same value
//! for the same timestamp is a no-op (`inserted: 0`). Every insert also
//! writes an `ai_action_log` row in the same transaction so the entry can be
//! reverted from the actions list.
//!
//! Request body: `{ metric: "weight_kg" | "body_fat_pct", value: f64,
//! ts?: ISO-8601 }`. `ts` defaults to now; `value` is clamped to the metric's
//! sane range (weight 20..400 kg, body fat 3..60 %).

use axum::{
    extract::{Extension, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    routes::actions::{log_action, KIND_MEASUREMENT},
};

/// Canonical metrics accepted from the user directly.
pub const METRIC_WEIGHT_KG: &str = "weight_kg";
pub const METRIC_BODY_FAT_PCT: &str = "body_fat_pct";

/// Units recorded alongside each metric.
pub const UNIT_WEIGHT_KG: &str = "kg";
pub const UNIT_BODY_FAT_PCT: &str = "percent";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMeasurement {
    pub metric: String,
    pub value: f64,
    /// ISO-8601 timestamp; defaults to now when omitted.
    #[serde(default)]
    pub ts: Option<DateTime<Utc>>,
}

pub async fn add_measurement(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<AddMeasurement>,
) -> ApiResult<Json<Value>> {
    let metric = body.metric.trim().to_ascii_lowercase();
    let (unit, lo, hi) = match metric.as_str() {
        METRIC_WEIGHT_KG => (UNIT_WEIGHT_KG, 20.0, 400.0),
        METRIC_BODY_FAT_PCT => (UNIT_BODY_FAT_PCT, 3.0, 60.0),
        _ => {
            return Err(ApiError::BadRequest(
                "metric must be 'weight_kg' or 'body_fat_pct'".to_string(),
            ))
        }
    };
    if !body.value.is_finite() {
        return Err(ApiError::BadRequest("value must be finite".to_string()));
    }
    let value = body.value.clamp(lo, hi);
    let ts = body.ts.unwrap_or_else(Utc::now);

    let mut tx = pool.begin().await?;
    let res = sqlx::query(
        "INSERT INTO measurements (ts, user_id, source, device_id, metric, value, unit)
         VALUES ($1, $2, 'user', NULL, $3, $4, $5)
         ON CONFLICT (user_id, metric, ts) DO NOTHING",
    )
    .bind(ts)
    .bind(user.0)
    .bind(&metric)
    .bind(value)
    .bind(unit)
    .execute(&mut *tx)
    .await?;
    let inserted = res.rows_affected() as usize;

    // Audit only real inserts: a duplicate (inserted 0) made no change and has
    // nothing to revert.
    if inserted == 1 {
        log_action(
            &mut tx,
            user.0,
            KIND_MEASUREMENT,
            &metric,
            json!({
                "metric": &metric,
                "value": value,
                "unit": unit,
                "ts": &ts,
            }),
            json!({ "metric": &metric, "ts": &ts }),
        )
        .await?;
    }
    tx.commit().await?;

    Ok(Json(json!({ "inserted": inserted })))
}

/// Latest logged value of `metric` for the user (newest first), used by
/// PYTHIA to fill the `current` field of action proposals. `None` when the
/// user has no entry for that metric yet.
pub async fn latest(
    pool: &PgPool,
    user_id: Uuid,
    metric: &str,
) -> Result<Option<f64>, sqlx::Error> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT value FROM measurements
         WHERE user_id = $1 AND metric = $2
         ORDER BY ts DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(metric)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v))
}
