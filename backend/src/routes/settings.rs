//! Per-user settings stored in the DB (user_settings table) — the web UI's
//! preferences live in the same volume as everything else; no second
//! persistent mount needed. Free-form JSONB so the schema can grow without
//! migrations.

use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    auth::AuthUser,
    error::ApiResult,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    pub settings: Value,
}

pub async fn get_settings(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<Value>> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT settings FROM user_settings WHERE user_id = $1")
        .bind(user.0)
        .fetch_optional(&pool)
        .await?;
    let settings = row.map(|r| r.0).unwrap_or_else(|| json!({}));
    Ok(Json(json!({ "settings": settings })))
}

pub async fn put_settings(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<UpdateSettings>,
) -> ApiResult<Json<Value>> {
    sqlx::query(
        "INSERT INTO user_settings (user_id, settings, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (user_id)
         DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
    )
    .bind(user.0)
    .bind(&body.settings)
    .execute(&pool)
    .await?;
    Ok(Json(json!({ "settings": body.settings })))
}
