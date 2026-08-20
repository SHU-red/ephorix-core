//! CRUD for Agoge Types (reference data, shared across users).

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    models::AgogeType,
};

pub async fn list(State(pool): State<PgPool>) -> ApiResult<Json<serde_json::Value>> {
    let types: Vec<AgogeType> =
        sqlx::query_as("SELECT * FROM agoge_types ORDER BY name").fetch_all(&pool).await?;
    Ok(Json(json!({ "types": types })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertType {
    pub name: String,
    #[serde(default)]
    pub color_code: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

const CATEGORIES: [&str; 6] = [
    "distance", "repetitive", "dynamic", "circuit", "recovery", "mixed",
];

fn normalize_category(c: Option<String>) -> String {
    let c = c.unwrap_or_else(|| "mixed".to_string());
    if CATEGORIES.contains(&c.as_str()) {
        c
    } else {
        "mixed".to_string()
    }
}
fn normalize_config(c: Option<serde_json::Value>) -> serde_json::Value {
    match c {
        Some(serde_json::Value::Object(o)) => serde_json::Value::Object(o),
        _ => serde_json::Value::Object(Default::default()),
    }
}

pub async fn create(
    State(pool): State<PgPool>,
    Json(body): Json<UpsertType>,
) -> ApiResult<(axum::http::StatusCode, Json<AgogeType>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".to_string()));
    }
    let category = normalize_category(body.category);
    let config = normalize_config(body.config);
    let ty = sqlx::query_as::<_, AgogeType>(
        "INSERT INTO agoge_types (name, color_code, icon, category, config)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(name)
    .bind(body.color_code.unwrap_or_else(|| "#E53935".to_string()))
    .bind(body.icon.unwrap_or_else(|| "dumbbell".to_string()))
    .bind(&category)
    .bind(&config)
    .fetch_one(&pool)
    .await?;
    Ok((axum::http::StatusCode::CREATED, Json(ty)))
}

pub async fn update(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpsertType>,
) -> ApiResult<Json<AgogeType>> {
    let category = normalize_category(body.category);
    let config = normalize_config(body.config);
    let ty = sqlx::query_as::<_, AgogeType>(
        "UPDATE agoge_types
         SET name = $2,
             color_code = COALESCE($3, color_code),
             icon = COALESCE($4, icon),
             category = $5,
             config = $6
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(body.color_code)
    .bind(body.icon)
    .bind(&category)
    .bind(&config)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("agoge type {id} not found")))?;
    Ok(Json(ty))
}

pub async fn delete(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let res = sqlx::query("DELETE FROM agoge_types WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound(format!("agoge type {id} not found")));
    }
    Ok(Json(json!({ "deleted": id })))
}

