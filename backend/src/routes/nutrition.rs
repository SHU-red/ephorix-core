//! Food and water intake. Each entry is stored in `nutrition_log` and also
//! mirrored into the normalized `measurements` store (water_ml / food_kcal)
//! so body-battery and any cross-source aggregation can see nutrition without
//! special-casing this table.

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    normalize::{METRIC_FOOD_KCAL, METRIC_WATER_ML},
    routes::actions::{log_action, KIND_NUTRITION},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NutritionEntry {
    /// water | food
    pub kind: String,
    /// ml for water, kcal for food
    pub amount: f64,
    #[serde(default)]
    pub protein: f64,
    #[serde(default)]
    pub carbs: f64,
    #[serde(default)]
    pub fat: f64,
    /// e.g. "breakfast" — marks a food entry as a meal
    #[serde(default)]
    pub meal_type: Option<String>,
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
    pub protein: Option<f64>,
    pub carbs: Option<f64>,
    pub fat: Option<f64>,
    pub meal_type: Option<String>,
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
    for macro_g in [entry.protein, entry.carbs, entry.fat] {
        if !macro_g.is_finite() || macro_g < 0.0 {
            return Err(ApiError::BadRequest(
                "protein/carbs/fat must be non-negative".to_string(),
            ));
        }
    }

    let mut tx = pool.begin().await?;
    let row: NutritionRow = sqlx::query_as(
        "INSERT INTO nutrition_log
             (user_id, kind, amount, protein, carbs, fat, meal_type, consumed_at, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, kind, amount, protein, carbs, fat, meal_type, consumed_at, note",
    )
    .bind(user.0)
    .bind(&kind)
    .bind(entry.amount)
    .bind(entry.protein)
    .bind(entry.carbs)
    .bind(entry.fat)
    .bind(entry.meal_type.as_deref())
    .bind(entry.consumed_at)
    .bind(&entry.note)
    .fetch_one(&mut *tx)
    .await?;

    // Mirror into the normalized store (same dedup semantics as
    // `insert_measurements`: unique (user_id, metric, ts)).
    let (metric, unit) = if kind == "water" {
        (METRIC_WATER_ML, "ml")
    } else {
        (METRIC_FOOD_KCAL, "kcal")
    };
    let _ = sqlx::query(
        "INSERT INTO measurements (ts, user_id, source, device_id, metric, value, unit)
         VALUES ($1, $2, 'manual', NULL, $3, $4, $5)
         ON CONFLICT (user_id, metric, ts) DO NOTHING",
    )
    .bind(entry.consumed_at)
    .bind(user.0)
    .bind(metric)
    .bind(entry.amount)
    .bind(unit)
    .execute(&mut *tx)
    .await?;

    // Audit in the same transaction so the entry is always revertible.
    log_action(
        &mut tx,
        user.0,
        KIND_NUTRITION,
        "nutrition",
        json!({
            "id": row.id,
            "kind": &kind,
            "amount": entry.amount,
            "protein": entry.protein,
            "carbs": entry.carbs,
            "fat": entry.fat,
            "mealType": &entry.meal_type,
            "note": &entry.note,
            "ts": entry.consumed_at,
        }),
        json!({
            "id": row.id,
            "kind": &kind,
            "amount": entry.amount,
            "mealType": &entry.meal_type,
            "note": &entry.note,
            "ts": entry.consumed_at,
        }),
    )
    .await?;
    tx.commit().await?;

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
        "SELECT id, kind, amount, protein, carbs, fat, meal_type, consumed_at, note
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

#[derive(Debug, Deserialize)]
pub struct DailyQuery {
    /// Calendar date (UTC), YYYY-MM-DD.
    pub date: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MealOut {
    pub id: Uuid,
    /// water | food | meal
    pub r#type: String,
    pub meal_type: Option<String>,
    pub amount: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
    pub note: Option<String>,
    pub consumed_at: DateTime<Utc>,
}

pub async fn daily(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<DailyQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let day = NaiveDate::parse_from_str(&q.date, "%Y-%m-%d")
        .map_err(|_| ApiError::BadRequest("'date' must be YYYY-MM-DD".to_string()))?;
    let from = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let to = (day + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap().and_utc();

    let rows: Vec<NutritionRow> = sqlx::query_as(
        "SELECT id, kind, amount, protein, carbs, fat, meal_type, consumed_at, note
         FROM nutrition_log
         WHERE user_id = $1 AND consumed_at >= $2 AND consumed_at < $3
         ORDER BY consumed_at ASC",
    )
    .bind(user.0)
    .bind(from)
    .bind(to)
    .fetch_all(&pool)
    .await?;

    let (mut kcal, mut protein, mut carbs, mut fat, mut water_ml) =
        (0.0, 0.0, 0.0, 0.0, 0.0);
    for r in &rows {
        if r.kind == "water" {
            water_ml += r.amount;
        } else {
            kcal += r.amount;
        }
        protein += r.protein.unwrap_or(0.0);
        carbs += r.carbs.unwrap_or(0.0);
        fat += r.fat.unwrap_or(0.0);
    }

    // Per-user nutrition goals live in the free-form JSONB settings blob.
    let settings: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT settings FROM user_settings WHERE user_id = $1")
            .bind(user.0)
            .fetch_optional(&pool)
            .await?;
    let water_goal_ml = settings
        .map(|(s,)| s)
        .and_then(|s| s.get("nutrition").cloned())
        .and_then(|n| n.get("waterGoalMl").and_then(|v| v.as_f64()))
        .unwrap_or(2500.0);

    let meals: Vec<MealOut> = rows
        .iter()
        .map(|r| {
            let meal_kind = if r.kind == "water" {
                "water"
            } else if r.meal_type.is_some() {
                "meal"
            } else {
                "food"
            };
            MealOut {
                id: r.id,
                r#type: meal_kind.to_string(),
                meal_type: r.meal_type.clone(),
                amount: r.amount,
                protein: r.protein.unwrap_or(0.0),
                carbs: r.carbs.unwrap_or(0.0),
                fat: r.fat.unwrap_or(0.0),
                note: r.note.clone(),
                consumed_at: r.consumed_at,
            }
        })
        .collect();

    Ok(Json(json!({
        "date": day,
        "kcal": kcal,
        "protein": protein,
        "carbs": carbs,
        "fat": fat,
        "waterMl": water_ml,
        "waterGoalMl": water_goal_ml,
        "meals": meals,
    })))
}
