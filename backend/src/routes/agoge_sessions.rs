//! CRUD for Agoge Sessions (derived state; also manually editable).

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    models::AgogeSession,
};

#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn list(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<SessionListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sessions: Vec<AgogeSession> = sqlx::query_as(
        "SELECT * FROM agoge_sessions
         WHERE user_id = $1
           AND ($2::text IS NULL OR status = $2)
           AND ($3::timestamptz IS NULL OR start_time >= $3)
           AND ($4::timestamptz IS NULL OR start_time <= $4)
         ORDER BY start_time DESC
         LIMIT $5",
    )
    .bind(user.0)
    .bind(q.status)
    .bind(q.from)
    .bind(q.to)
    .bind(q.limit.unwrap_or(200))
    .fetch_all(&pool)
    .await?;
    Ok(Json(json!({ "sessions": sessions })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    #[serde(default)]
    pub type_id: Option<Uuid>,
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Retroactive session creation from the web UI timeline.
pub async fn create(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateSession>,
) -> ApiResult<(axum::http::StatusCode, Json<AgogeSession>)> {
    validate_times(body.start_time, body.end_time)?;

    let status = match body.status.as_deref() {
        Some(s) if s == "active" || s == "closed" => s.to_string(),
        None => {
            if body.end_time.is_some() {
                "closed".to_string()
            } else {
                "active".to_string()
            }
        }
        Some(_) => {
            return Err(ApiError::BadRequest(
                "status must be 'active' or 'closed'".to_string(),
            ))
        }
    };

    let session = sqlx::query_as::<_, AgogeSession>(
        "INSERT INTO agoge_sessions (user_id, type_id, start_time, end_time, status)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(user.0)
    .bind(body.type_id)
    .bind(body.start_time)
    .bind(body.end_time)
    .bind(status)
    .fetch_one(&pool)
    .await?;
    Ok((axum::http::StatusCode::CREATED, Json(session)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSession {
    #[serde(default)]
    pub type_id: Option<Uuid>,
    #[serde(default)]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Manual close (add a stop) or edit. Only the owning user may mutate.
pub async fn update(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateSession>,
) -> ApiResult<Json<AgogeSession>> {
    let current = sqlx::query_as::<_, AgogeSession>(
        "SELECT * FROM agoge_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("session {id} not found")))?;

    let end_time = body.end_time.or(current.end_time);
    let status = body
        .status
        .clone()
        .unwrap_or_else(|| if end_time.is_some() { "closed".to_string() } else { current.status.clone() });
    if status != "active" && status != "closed" {
        return Err(ApiError::BadRequest("status must be 'active' or 'closed'".to_string()));
    }
    validate_times(current.start_time, end_time)?;

    let updated = sqlx::query_as::<_, AgogeSession>(
        "UPDATE agoge_sessions
         SET type_id = COALESCE($3, type_id),
             end_time = $4,
             status = $5,
             updated_at = now()
         WHERE id = $1 AND user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(user.0)
    .bind(body.type_id)
    .bind(end_time)
    .bind(status)
    .fetch_one(&pool)
    .await?;
    Ok(Json(updated))
}

pub async fn delete(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let res = sqlx::query("DELETE FROM agoge_sessions WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.0)
        .execute(&pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound(format!("session {id} not found")));
    }
    Ok(Json(json!({ "deleted": id })))
}

/// Aggregate stats for one session: duration, pause time, and measurement
/// rollups over [start_time, end_time ?? now()].
pub async fn stats(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let session = sqlx::query_as::<_, AgogeSession>(
        "SELECT * FROM agoge_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("session {id} not found")))?;

    let end = session.end_time.unwrap_or_else(Utc::now);
    let duration_sec = (end - session.start_time).num_seconds();

    // Each 'pause' pairs with the next 'resume'; a trailing unpaired pause
    // runs to the end of the window.
    let markers: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT kind, occurred_at FROM agoge_markers
         WHERE user_id = $1 AND kind IN ('pause', 'resume')
           AND occurred_at >= $2 AND occurred_at < $3
         ORDER BY occurred_at",
    )
    .bind(user.0)
    .bind(session.start_time)
    .bind(end)
    .fetch_all(&pool)
    .await?;

    let mut pause_sec: i64 = 0;
    let mut open_pause: Option<DateTime<Utc>> = None;
    for (kind, at) in &markers {
        match kind.as_str() {
            "pause" => {
                if open_pause.is_none() {
                    open_pause = Some(*at);
                }
            }
            "resume" => {
                if let Some(p) = open_pause.take() {
                    pause_sec += (*at - p).num_seconds();
                }
            }
            _ => {}
        }
    }
    if let Some(p) = open_pause {
        pause_sec += (end - p).num_seconds();
    }

    let agg: (f64, f64, f64, f64) = sqlx::query_as(
        "SELECT
            COALESCE(SUM(value) FILTER (WHERE metric = 'reps'), 0)::float8,
            COALESCE(SUM(value) FILTER (WHERE metric = 'active_calories'), 0)::float8,
            COALESCE(AVG(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8,
            COALESCE(MAX(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('reps', 'active_calories', 'heart_rate')",
    )
    .bind(user.0)
    .bind(session.start_time)
    .bind(end)
    .fetch_one(&pool)
    .await?;

    Ok(Json(json!({
        "durationSec": duration_sec,
        "activeSec": duration_sec - pause_sec,
        "pauseSec": pause_sec,
        "reps": agg.0.round() as i64,
        "calories": agg.1,
        "avgHr": agg.2,
        "peakHr": agg.3.round() as i64,
    })))
}

fn validate_times(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> ApiResult<()> {
    if let Some(e) = end {
        if e < start {
            return Err(ApiError::BadRequest(
                "end_time must be after start_time".to_string(),
            ));
        }
    }
    Ok(())
}
