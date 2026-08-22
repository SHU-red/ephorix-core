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
    routes::actions::{log_action, KIND_SETTINGS},
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
    // Snapshot the previous settings so the write can be reverted later.
    let prev: Value = {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT settings FROM user_settings WHERE user_id = $1")
                .bind(user.0)
                .fetch_optional(&pool)
                .await?;
        row.map(|r| r.0).unwrap_or_else(|| json!({}))
    };

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_settings (user_id, settings, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (user_id)
         DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
    )
    .bind(user.0)
    .bind(&body.settings)
    .execute(&mut *tx)
    .await?;
    log_action(
        &mut tx,
        user.0,
        KIND_SETTINGS,
        "settings",
        json!({ "before": &prev, "after": &body.settings }),
        json!({ "before": &prev }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "settings": body.settings })))
}
