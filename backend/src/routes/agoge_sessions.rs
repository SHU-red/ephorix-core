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
    routes::health::session_pulse,
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

/// `list` row: an `AgogeSession` plus the rendering bounds computed at read
/// time. No schema migration — `effective_end` is derived.
#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)] // some AgogeSession columns are selected but not sent on the wire
struct SessionListRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub type_id: Option<Uuid>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub status: String,
    pub duration_sec: Option<i32>,
    pub workout_kcal: Option<f32>,
    pub avg_hr: Option<i32>,
    pub reps: Option<i32>,
    pub movement_intensity: Option<f32>,
    pub distance_m: Option<f32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub effective_end: DateTime<Utc>,
    pub last_data_at: Option<DateTime<Utc>>,
    /// TODO signal: this session got a Start marker but no Stop. The UI
    /// resolves it by setting `end_time` via the existing session `update`
    /// endpoint (PATCH/PUT /api/v1/agoge/sessions/:id).
    pub needs_end: bool,
}

pub async fn list(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<SessionListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sessions: Vec<SessionListRow> = sqlx::query_as(
        "SELECT s.*,
                COALESCE(
                    s.end_time,
                    LEAST(
                        COALESCE(ns.next_start, 'infinity'::timestamptz),
                        COALESCE(ld.last_data, 'infinity'::timestamptz),
                        now()
                    )
                ) AS effective_end,
                ld.last_data AS last_data_at,
                (s.end_time IS NULL
                 AND (ld.last_data IS NULL
                      OR ld.last_data < now() - interval '10 minutes')) AS needs_end
         FROM agoge_sessions s
         LEFT JOIN LATERAL (
             SELECT MIN(start_time) AS next_start
             FROM agoge_sessions
             WHERE user_id = s.user_id AND start_time > s.start_time
         ) ns ON true
         LEFT JOIN LATERAL (
             -- Bound to THIS session's window. Without the upper bound an
             -- abandoned older session inherits the raw samples of every
             -- later workout, so `last_data` looks recent, `needs_end` stays
             -- false and the session is never flagged — exactly the case this
             -- exists for (several start-without-stop sessions in a row).
             -- `ns` is joined to the left, so it is in scope here.
             SELECT MAX(timestamp) AS last_data
             FROM raw_health_data
             WHERE user_id = s.user_id
               AND timestamp >= s.start_time
               AND (ns.next_start IS NULL OR timestamp < ns.next_start)
         ) ld ON true
         WHERE s.user_id = $1
           AND ($2::text IS NULL OR s.status = $2)
           AND ($3::timestamptz IS NULL OR s.start_time >= $3)
           AND ($4::timestamptz IS NULL OR s.start_time <= $4)
         ORDER BY s.start_time DESC
         LIMIT $5",
    )
    .bind(user.0)
    .bind(q.status)
    .bind(q.from)
    .bind(q.to)
    .bind(q.limit.unwrap_or(200))
    .fetch_all(&pool)
    .await?;
    // Wire shape is exactly what the cards need (type, status, start/end,
    // watch summary: duration / avg HR / kcal / reps / intensity / distance)
    // — no internal row fields (user_id, created_at, updated_at).
    let out: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "typeId": s.type_id,
                "startTime": s.start_time,
                "endTime": s.end_time,
                "status": s.status,
                "durationSec": s.duration_sec,
                "workoutKcal": s.workout_kcal,
                "avgHr": s.avg_hr,
                "reps": s.reps,
                "movementIntensity": s.movement_intensity,
                "distanceM": s.distance_m,
                "effectiveEnd": s.effective_end,
                "needsEnd": s.needs_end,
                "lastDataAt": s.last_data_at,
            })
        })
        .collect();
    Ok(Json(json!({ "sessions": out })))
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
    pub start_time: Option<DateTime<Utc>>,
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
    let start_time = body.start_time.or(Some(current.start_time)).unwrap();
    let status = body
        .status
        .clone()
        .unwrap_or_else(|| if end_time.is_some() { "closed".to_string() } else { current.status.clone() });
    if status != "active" && status != "closed" {
        return Err(ApiError::BadRequest("status must be 'active' or 'closed'".to_string()));
    }
    validate_times(start_time, end_time)?;

    let updated = sqlx::query_as::<_, AgogeSession>(
        "UPDATE agoge_sessions
         SET start_time = $3,
             type_id = COALESCE($4, type_id),
             end_time = $5,
             status = $6,
             updated_at = now()
         WHERE id = $1 AND user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(user.0)
    .bind(start_time)
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
    // Delete the session AND its own marker stream atomically. The markers
    // are linked via session_id (FK ON DELETE SET NULL) — leaving them would
    // orphan unattributable pause/resume rows that could leak into another
    // session's stats by time-range overlap. exercise_sets cascade via their
    // FK; nothing else references a session.
    let mut tx = pool.begin().await?;
    let markers = sqlx::query("DELETE FROM agoge_markers WHERE session_id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.0)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("DELETE FROM agoge_sessions WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.0)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound(format!("session {id} not found")));
    }
    Ok(Json(json!({ "deleted": id, "markers": markers.rows_affected() })))
}

/// Aggregate stats for one session: duration, pause time, reps/HR measurement
/// rollups over [start_time, end_time ?? now()], and active calories as the
/// cumulative-counter delta across that window.
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

    let agg: (f64, f64, f64) = sqlx::query_as(
        "SELECT
            COALESCE(SUM(value) FILTER (WHERE metric = 'reps'), 0)::float8,
            COALESCE(AVG(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8,
            COALESCE(MAX(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('reps', 'heart_rate')",
    )
    .bind(user.0)
    .bind(session.start_time)
    .bind(end)
    .fetch_one(&pool)
    .await?;

    // active_calories on the watch is a CUMULATIVE day total
    // (`health_service_sum(metric, today, now)` per push), so summing the
    // rows in the window inflates the session — measured: 1417 kcal reported
    // for a 39-minute session whose true delta is 77. Burn across the window
    // is the counter's movement: the last value inside [start, end) minus the
    // last value strictly before start, clamped at 0. A baseline ABOVE the
    // window's max means the counter reset (watch's local midnight) in
    // between: the window started from 0, so treat the baseline as 0. Read
    // from raw_health_data, NOT measurements: the latter also holds the
    // noon-anchored `/health/days` full-day rows, whose value would get
    // counted as if it had accrued inside the window.
    let (calories,): (f64,) = sqlx::query_as(
        "SELECT GREATEST(
            COALESCE(end_v.v, 0)
          - CASE WHEN COALESCE(base_v.v, 0) <= COALESCE(end_v.v, 0)
                 THEN COALESCE(base_v.v, 0) ELSE 0 END,
            0)::float8
          FROM (SELECT MAX(active_calories) AS v FROM raw_health_data
                WHERE user_id = $1 AND active_calories IS NOT NULL
                  AND timestamp >= $2 AND timestamp < $3) end_v,
               (SELECT (SELECT active_calories FROM raw_health_data
                        WHERE user_id = $1 AND active_calories IS NOT NULL
                          AND timestamp < $2
                        ORDER BY timestamp DESC LIMIT 1) AS v) base_v",
    )
    .bind(user.0)
    .bind(session.start_time)
    .bind(end)
    .fetch_one(&pool)
    .await?;

    // Fallback when the window holds no raw counter rows (the watch pushed
    // nothing during this session — 15 of the 27 sessions in the live DB):
    // report the watch's own Stop-summary delta instead of a misleading 0.
    // Mirrors the avg_hr precedence below.
    let calories = if calories > 0.0 {
        calories
    } else {
        session
            .workout_kcal
            .map(f64::from)
            .filter(|v| *v > 0.0)
            .unwrap_or(0.0)
    };

    let sets_agg: (i64, i64, f64) = sqlx::query_as(
        "SELECT
            COUNT(*)::int8,
            COALESCE(SUM(reps), 0)::int8,
            COALESCE(SUM(reps * weight_kg) FILTER (WHERE weight_kg IS NOT NULL), 0)::float8
         FROM exercise_sets
         WHERE session_id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_one(&pool)
    .await?;

    // Pulse source of truth: the session window's heart-rate series read
    // from raw_health_data (the fast path the timeline uses). avgHr/minHr/
    // maxHr are null and series is empty when the window has no HR rows.
    let pulse = session_pulse(&pool, user.0, session.start_time, end).await?;

    // avgHr precedence: the watch's stop-marker average when present, else
    // the average computed from raw_health_data, else the measurements
    // rollup (imported/non-Pebble sources), else 0.
    let avg_hr = session
        .avg_hr
        .filter(|v| *v > 0)
        .map(f64::from)
        .or_else(|| pulse["avgHr"].as_f64())
        .or_else(|| (agg.1 > 0.0).then_some(agg.1))
        .unwrap_or(0.0);
    // peakHr keeps the measurements max (exact per-minute values) and falls
    // back to the raw_health_data max when measurements have none.
    let peak_hr = if agg.2 > 0.0 {
        agg.2.round() as i64
    } else {
        pulse["maxHr"].as_i64().unwrap_or(0)
    };

    Ok(Json(json!({
        "durationSec": duration_sec,
        "activeSec": duration_sec - pause_sec,
        "pauseSec": pause_sec,
        "reps": agg.0.round() as i64,
        "calories": calories,
        "avgHr": avg_hr,
        "peakHr": peak_hr,
        "sets": sets_agg.0,
        "totalReps": sets_agg.1,
        "volumeKg": sets_agg.2,
        "pulse": pulse,
    })))
}

// ---------------------------------------------------------------------------
// Manual exercise sets: per-set rows (reps / weight / rest) for a session.
// An "exercise" is the group of rows sharing an exercise_name; its id is
// the id of the earliest set row in the group.
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct SetRow {
    id: Uuid,
    exercise_name: String,
    set_number: i32,
    reps: i32,
    weight_kg: Option<f64>,
    rest_sec: Option<i32>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseSetIn {
    pub set_number: i32,
    pub reps: i32,
    #[serde(default)]
    pub weight_kg: Option<f64>,
    #[serde(default)]
    pub rest_sec: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddExercise {
    pub name: String,
    pub sets: Vec<ExerciseSetIn>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateExercise {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sets: Option<Vec<ExerciseSetIn>>,
}

/// Build the wire shape `{"id", "name", "sets": [...]}` for one exercise
/// group; `rows` must be ordered by set_number.
fn exercise_json(group_id: Uuid, name: &str, rows: &[SetRow]) -> serde_json::Value {
    let sets = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "setNumber": r.set_number,
                "reps": r.reps,
                "weightKg": r.weight_kg,
                "restSec": r.rest_sec,
            })
        })
        .collect::<Vec<_>>();
    json!({ "id": group_id, "name": name, "sets": sets })
}

/// All exercises of a session: rows grouped by name in first-inserted
/// order, sets within an exercise ordered by set_number.
pub async fn exercises(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query_as::<_, AgogeSession>(
        "SELECT * FROM agoge_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("session {id} not found")))?;

    let rows: Vec<SetRow> = sqlx::query_as(
        "SELECT id, exercise_name, set_number, reps, weight_kg, rest_sec, created_at,
                MIN(created_at) OVER (PARTITION BY exercise_name) AS group_start,
                MIN(set_number) OVER (PARTITION BY exercise_name) AS group_first_set
         FROM exercise_sets
         WHERE session_id = $1 AND user_id = $2
         ORDER BY group_start, group_first_set, set_number, id",
    )
    .bind(id)
    .bind(user.0)
    .fetch_all(&pool)
    .await?;

    let mut groups: Vec<Vec<SetRow>> = Vec::new();
    for r in rows {
        match groups.last_mut() {
            Some(g) if g[0].exercise_name == r.exercise_name => g.push(r),
            _ => groups.push(vec![r]),
        }
    }
    let exercises = groups
        .iter()
        .map(|g| {
            let first = g.iter().min_by_key(|r| (r.created_at, r.set_number)).unwrap();
            exercise_json(first.id, &first.exercise_name, g)
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "exercises": exercises })))
}

/// Append a new exercise (and its sets) to the session.
pub async fn add_exercise(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddExercise>,
) -> ApiResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, AgogeSession>(
        "SELECT * FROM agoge_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("session {id} not found")))?;
    if body.sets.is_empty() {
        return Err(ApiError::BadRequest("sets must not be empty".to_string()));
    }

    let mut tx = pool.begin().await?;
    for s in &body.sets {
        sqlx::query(
            "INSERT INTO exercise_sets (session_id, user_id, exercise_name, set_number, reps, weight_kg, rest_sec)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(user.0)
        .bind(&body.name)
        .bind(s.set_number)
        .bind(s.reps)
        .bind(s.weight_kg)
        .bind(s.rest_sec)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let rows: Vec<SetRow> = sqlx::query_as(
        "SELECT id, exercise_name, set_number, reps, weight_kg, rest_sec, created_at
         FROM exercise_sets
         WHERE session_id = $1 AND user_id = $2 AND exercise_name = $3
         ORDER BY set_number",
    )
    .bind(id)
    .bind(user.0)
    .bind(&body.name)
    .fetch_all(&pool)
    .await?;
    let first = rows.iter().min_by_key(|r| (r.created_at, r.set_number)).unwrap();
    Ok((
        axum::http::StatusCode::CREATED,
        Json(exercise_json(first.id, &body.name, &rows)),
    ))
}

/// Rename and/or wholesale-replace an exercise's sets.
pub async fn update_exercise(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path((_, eid)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateExercise>,
) -> ApiResult<Json<serde_json::Value>> {
    let (session_id, old_name): (Uuid, String) = sqlx::query_as(
        "SELECT es.session_id, es.exercise_name
         FROM exercise_sets es
         JOIN agoge_sessions s ON s.id = es.session_id
         WHERE es.id = $1 AND s.user_id = $2",
    )
    .bind(eid)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("exercise {eid} not found")))?;
    if body.sets.as_ref().is_some_and(Vec::is_empty) {
        return Err(ApiError::BadRequest("sets must not be empty".to_string()));
    }

    let new_name = body.name.clone().unwrap_or(old_name.clone());
    let mut tx = pool.begin().await?;
    if body.name.is_some() {
        sqlx::query(
            "UPDATE exercise_sets SET exercise_name = $3
             WHERE session_id = $1 AND user_id = $2 AND exercise_name = $4",
        )
        .bind(session_id)
        .bind(user.0)
        .bind(&new_name)
        .bind(&old_name)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(sets) = &body.sets {
        sqlx::query(
            "DELETE FROM exercise_sets
             WHERE session_id = $1 AND user_id = $2 AND exercise_name = $3",
        )
        .bind(session_id)
        .bind(user.0)
        .bind(&new_name)
        .execute(&mut *tx)
        .await?;
        for s in sets {
            sqlx::query(
                "INSERT INTO exercise_sets (session_id, user_id, exercise_name, set_number, reps, weight_kg, rest_sec)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(session_id)
            .bind(user.0)
            .bind(&new_name)
            .bind(s.set_number)
            .bind(s.reps)
            .bind(s.weight_kg)
            .bind(s.rest_sec)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;

    let rows: Vec<SetRow> = sqlx::query_as(
        "SELECT id, exercise_name, set_number, reps, weight_kg, rest_sec, created_at
         FROM exercise_sets
         WHERE session_id = $1 AND user_id = $2 AND exercise_name = $3
         ORDER BY set_number",
    )
    .bind(session_id)
    .bind(user.0)
    .bind(&new_name)
    .fetch_all(&pool)
    .await?;
    let first = rows.iter().min_by_key(|r| (r.created_at, r.set_number)).unwrap();
    Ok(Json(exercise_json(first.id, &new_name, &rows)))
}

/// Remove an exercise and all of its sets.
pub async fn delete_exercise(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path((_, eid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    let (session_id, name): (Uuid, String) = sqlx::query_as(
        "SELECT es.session_id, es.exercise_name
         FROM exercise_sets es
         JOIN agoge_sessions s ON s.id = es.session_id
         WHERE es.id = $1 AND s.user_id = $2",
    )
    .bind(eid)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("exercise {eid} not found")))?;

    sqlx::query(
        "DELETE FROM exercise_sets
         WHERE session_id = $1 AND user_id = $2 AND exercise_name = $3",
    )
    .bind(session_id)
    .bind(user.0)
    .bind(&name)
    .execute(&pool)
    .await?;
    Ok(Json(json!({ "deleted": eid })))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_times_accepts_valid_ranges() {
        let start = Utc::now();
        assert!(validate_times(start, None).is_ok());
        assert!(validate_times(start, Some(start + chrono::Duration::minutes(30))).is_ok());
        // A closed-at-instant session (end == start) is legal.
        assert!(validate_times(start, Some(start)).is_ok());
    }

    #[test]
    fn validate_times_rejects_end_before_start() {
        let start = Utc::now();
        let err = validate_times(start, Some(start - chrono::Duration::seconds(1))).unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
    }
}
