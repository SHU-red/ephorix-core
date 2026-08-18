//! Discrete marker events (Start_Marker / Stop_Marker) — the event stream
//! from the watch. Sessions are materialized from these but remain editable.

use axum::{
    extract::{Extension, Query, State},
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
    models::{AgogeMarker, AgogeSession},
};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    Start,
    Stop,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerEvent {
    pub kind: MarkerKind,
    /// Agoge type id. Unknown/missing -> session recorded as "Undefined".
    #[serde(default)]
    pub type_id: Option<Uuid>,
    /// Fallback lookup by name when type_id is absent (watch sends name).
    #[serde(default)]
    pub type_name: Option<String>,
    /// Defaults to now() when absent (watch clock drift mitigation).
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    /// For `stop`: which session to close. Absent -> latest open session.
    #[serde(default)]
    pub session_id: Option<Uuid>,
    /// watch | web
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

pub async fn ingest_marker(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(ev): Json<MarkerEvent>,
) -> ApiResult<Json<AgogeSession>> {
    let MarkerEvent {
        kind,
        type_id,
        type_name,
        occurred_at,
        session_id,
        source,
        meta,
    } = ev;
    let occurred_at = occurred_at.unwrap_or_else(Utc::now);
    let source = source.unwrap_or_else(|| "watch".to_string());

    let session = match kind {
        MarkerKind::Start => start_session(&pool, user.0, type_id, type_name, occurred_at).await?,
        MarkerKind::Stop => stop_session(&pool, user.0, session_id, occurred_at).await?,
    };

    insert_marker(&pool, user.0, session.id, &session, occurred_at, &source, meta).await?;
    Ok(Json(session))
}

async fn start_session(
    pool: &PgPool,
    user_id: Uuid,
    type_id: Option<Uuid>,
    type_name: Option<String>,
    occurred_at: DateTime<Utc>,
) -> ApiResult<AgogeSession> {
    let type_id = resolve_type_id(pool, type_id, type_name.as_deref()).await?;

    let session = sqlx::query_as::<_, AgogeSession>(
        "INSERT INTO agoge_sessions (user_id, type_id, start_time, status)
         VALUES ($1, $2, $3, 'active')
         RETURNING *",
    )
    .bind(user_id)
    .bind(type_id)
    .bind(occurred_at)
    .fetch_one(pool)
    .await?;

    Ok(session)
}

/// Closes the given session (if owned), else the user's latest open session.
async fn stop_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Option<Uuid>,
    occurred_at: DateTime<Utc>,
) -> ApiResult<AgogeSession> {
    let session = if let Some(sid) = session_id {
        sqlx::query_as::<_, AgogeSession>(
            "UPDATE agoge_sessions
             SET end_time = $3, status = 'closed', updated_at = now()
             WHERE id = $1 AND user_id = $2 AND status = 'active'
             RETURNING *",
        )
        .bind(sid)
        .bind(user_id)
        .bind(occurred_at)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound("session not found, not owned, or already closed".to_string())
        })?
    } else {
        sqlx::query_as::<_, AgogeSession>(
            "UPDATE agoge_sessions
             SET end_time = $2, status = 'closed', updated_at = now()
             WHERE id = (SELECT id FROM agoge_sessions
                         WHERE user_id = $1 AND status = 'active'
                         ORDER BY start_time DESC LIMIT 1)
             RETURNING *",
        )
        .bind(user_id)
        .bind(occurred_at)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("no open agoge session".to_string()))?
    };
    Ok(session)
}

async fn insert_marker(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    session: &AgogeSession,
    occurred_at: DateTime<Utc>,
    source: &str,
    meta: Option<serde_json::Value>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO agoge_markers (user_id, session_id, kind, occurred_at, source, meta)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(if session.end_time.is_some() { "stop" } else { "start" })
    .bind(occurred_at)
    .bind(source)
    .bind(meta)
    .execute(pool)
    .await?;
    Ok(())
}

/// Validates a client-supplied type id; falls back to name match; else
/// "Undefined" (NULL) — never fails the request on unknown types.
async fn resolve_type_id(
    pool: &PgPool,
    type_id: Option<Uuid>,
    type_name: Option<&str>,
) -> ApiResult<Option<Uuid>> {
    if let Some(tid) = type_id {
        let found: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM agoge_types WHERE id = $1")
            .bind(tid)
            .fetch_optional(pool)
            .await?;
        if found.is_some() {
            return Ok(Some(tid));
        }
        tracing::warn!("unknown agoge type id {tid}; falling back to name");
    }
    if let Some(name) = type_name {
        let found: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM agoge_types WHERE lower(name) = lower($1) LIMIT 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        if let Some((tid,)) = found {
            return Ok(Some(tid));
        }
    }
    Ok(None) // Undefined Agoge
}

#[derive(Debug, Deserialize)]
pub struct MarkerListQuery {
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn list_markers(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<MarkerListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let markers: Vec<AgogeMarker> = sqlx::query_as(
        "SELECT * FROM agoge_markers
         WHERE user_id = $1
           AND ($2::timestamptz IS NULL OR occurred_at >= $2)
           AND ($3::timestamptz IS NULL OR occurred_at <= $3)
         ORDER BY occurred_at DESC
         LIMIT $4",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .bind(q.limit.unwrap_or(500))
    .fetch_all(&pool)
    .await?;

    Ok(Json(json!({ "markers": markers })))
}
