//! Server-side audit log of every reversible settings/nutrition/measurements
//! write (manual or applied from a PYTHIA proposal), with one-step revert.
//!
//! Each mutation writes one `ai_action_log` row in the SAME transaction as
//! the mutation (see `log_action`): `payload` describes what happened for the
//! list view, `undo` carries the exact recipe to reverse it.
//!
//! Revert recipes per kind:
//! - `settings`     → undo `{ "before": <previous settings jsonb> }`; restores
//!   `user_settings.settings` to `before`.
//! - `nutrition`    → undo `{ id, kind, amount, mealType, note, ts }`; deletes
//!   the `nutrition_log` entry and its mirrored `measurements` row
//!   (water_ml / food_kcal at `ts`).
//! - `measurement`  → undo `{ metric, ts }`; deletes the `measurements` row.
//!
//! Reverting marks `reverted_at`; a second revert of the same row is rejected
//! with HTTP 409.

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
};

/// Action kinds recorded in `ai_action_log`.
pub const KIND_SETTINGS: &str = "settings";
pub const KIND_NUTRITION: &str = "nutrition";
pub const KIND_MEASUREMENT: &str = "measurement";

#[derive(Debug, Deserialize)]
pub struct ActionsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ActionRow {
    pub id: Uuid,
    pub kind: String,
    pub target: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
    pub reverted_at: Option<DateTime<Utc>>,
}

/// Records one reversible mutation. MUST be called inside the same transaction
/// as the mutation it describes, so a log row can never exist for an undo that
/// does not.
pub async fn log_action(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    kind: &str,
    target: &str,
    payload: Value,
    undo: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ai_action_log (user_id, kind, target, payload, undo)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(target)
    .bind(payload)
    .bind(undo)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Lists the user's actions, newest first (limit default 100).
pub async fn list_actions(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<ActionsQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows: Vec<ActionRow> = sqlx::query_as(
        "SELECT id, kind, target, payload, created_at, reverted_at
         FROM ai_action_log
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT $2",
    )
    .bind(user.0)
    .bind(limit)
    .fetch_all(&pool)
    .await?;

    Ok(Json(json!({ "actions": rows })))
}

/// Applies the stored undo for one action and marks it reverted. A second
/// revert of the same row fails with 409; an unknown id fails with 404.
pub async fn revert_action(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let mut tx = pool.begin().await?;

    // FOR UPDATE: two concurrent reverts of the same row must not both pass
    // the reverted_at check — the row lock serializes them.
    let row: Option<(String, Value, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT kind, undo, reverted_at
         FROM ai_action_log
         WHERE id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((kind, undo, reverted_at)) = row else {
        return Err(ApiError::NotFound("action not found".to_string()));
    };
    if reverted_at.is_some() {
        return Err(ApiError::Conflict(
            "action already reverted".to_string(),
        ));
    }

    match kind.as_str() {
        KIND_SETTINGS => {
            let before = undo.get("before").cloned().unwrap_or_else(|| json!({}));
            sqlx::query(
                "INSERT INTO user_settings (user_id, settings, updated_at)
                 VALUES ($1, $2, now())
                 ON CONFLICT (user_id)
                 DO UPDATE SET settings = EXCLUDED.settings, updated_at = now()",
            )
            .bind(user.0)
            .bind(before)
            .execute(&mut *tx)
            .await?;
        }
        KIND_NUTRITION => {
            let entry_id: Uuid = undo
                .get("id")
                .and_then(|v| serde_json::from_value::<Uuid>(v.clone()).ok())
                .ok_or_else(|| {
                    ApiError::BadRequest("action undo missing nutrition id".to_string())
                })?;
            let kind = undo.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let ts: DateTime<Utc> = undo
                .get("ts")
                .and_then(|v| serde_json::from_value::<DateTime<Utc>>(v.clone()).ok())
                .ok_or_else(|| {
                    ApiError::BadRequest("action undo missing nutrition ts".to_string())
                })?;
            // Delete the entry and its mirrored normalized row together.
            let metric = if kind == "water" {
                "water_ml"
            } else {
                "food_kcal"
            };
            sqlx::query("DELETE FROM nutrition_log WHERE id = $1 AND user_id = $2")
                .bind(entry_id)
                .bind(user.0)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "DELETE FROM measurements WHERE user_id = $1 AND metric = $2 AND ts = $3",
            )
            .bind(user.0)
            .bind(metric)
            .bind(ts)
            .execute(&mut *tx)
            .await?;
        }
        KIND_MEASUREMENT => {
            let metric = undo.get("metric").and_then(|v| v.as_str()).ok_or_else(|| {
                ApiError::BadRequest("action undo missing measurement metric".to_string())
            })?;
            let ts: DateTime<Utc> = undo
                .get("ts")
                .and_then(|v| serde_json::from_value::<DateTime<Utc>>(v.clone()).ok())
                .ok_or_else(|| {
                    ApiError::BadRequest("action undo missing measurement ts".to_string())
                })?;
            sqlx::query(
                "DELETE FROM measurements WHERE user_id = $1 AND metric = $2 AND ts = $3",
            )
            .bind(user.0)
            .bind(metric)
            .bind(ts)
            .execute(&mut *tx)
            .await?;
        }
        other => {
            return Err(ApiError::BadRequest(format!("unknown action kind: {other}")));
        }
    }

    sqlx::query("UPDATE ai_action_log SET reverted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(Json(json!({ "reverted": true })))
}
