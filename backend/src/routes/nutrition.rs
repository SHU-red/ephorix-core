//! Food and water intake. Each entry is stored in `nutrition_log` and also
//! mirrored into the normalized `measurements` store (water_ml / food_kcal)
//! so body-battery and any cross-source aggregation can see nutrition without
//! special-casing this table.

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    normalize::{insert_measurements, Measurement, METRIC_FOOD_KCAL, METRIC_WATER_ML},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NutritionEntry {
    /// water | food
    pub kind: String,
    /// ml for water, kcal for food
    pub amount: f64,
    pub consumed_at: DateTime<Utc>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NutritionRow {
    pub id: Uuid,
    pub kind: String,
    pub amount: f64,
    pub consumed_at: DateTime<Utc>,
    pub note: Option<String>,
}

pub async fn add_nutrition(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(entry): Json<NutritionEntry>,
) -> ApiResult<Json<NutritionRow>> {
    let kind = entry.kind.trim().to_ascii_lowercase();
    if kind != "water" && kind != "food" {
        return Err(ApiError::BadRequest("kind must be 'water' or 'food'".to_string()));
    }
    if !entry.amount.is_finite() || entry.amount <= 0.0 {
        return Err(ApiError::BadRequest("amount must be positive".to_string()));
    }

    let row: NutritionRow = sqlx::query_as(
        "INSERT INTO nutrition_log (user_id, kind, amount, consumed_at, note)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, kind, amount, consumed_at, note",
    )
    .bind(user.0)
    .bind(&kind)
    .bind(entry.amount)
    .bind(entry.consumed_at)
    .bind(&entry.note)
    .fetch_one(&pool)
    .await?;

    // Mirror into the normalized store.
    let (metric, unit) = if kind == "water" {
        (METRIC_WATER_ML, "ml")
    } else {
        (METRIC_FOOD_KCAL, "kcal")
    };
    let _ = insert_measurements(
        &pool,
        user.0,
        "manual",
        None,
        &[Measurement::new(entry.consumed_at, metric, entry.amount, unit)],
    )
    .await?;

    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct NutritionQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

pub async fn list_nutrition(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<NutritionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(366) {
        return Err(ApiError::BadRequest("range exceeds 366 days".to_string()));
    }

    let rows: Vec<NutritionRow> = sqlx::query_as(
        "SELECT id, kind, amount, consumed_at, note
         FROM nutrition_log
         WHERE user_id = $1 AND consumed_at >= $2 AND consumed_at < $3
         ORDER BY consumed_at DESC",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&pool)
    .await?;

    let (mut water_ml, mut food_kcal) = (0.0, 0.0);
    for r in &rows {
        if r.kind == "water" {
            water_ml += r.amount;
        } else {
            food_kcal += r.amount;
        }
    }

    Ok(Json(json!({
        "entries": rows,
        "totals": { "waterMl": water_ml, "foodKcal": food_kcal }
    })))
}
