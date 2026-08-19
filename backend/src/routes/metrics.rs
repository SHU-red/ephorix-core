//! Derived metrics over the normalized `measurements` store:
//!   - body battery (sleep recharge vs activity drain, Garmin-style)
//!   - automated workout detection + acceptance: contiguous elevated-HR
//!     windows are proposed, classified against the user's own historical
//!     sessions, and the user accepts (→ Agoge session) or rejects them.
//! All are source-agnostic — they only read canonical metric names.

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth::AuthUser, error::{ApiError, ApiResult}, models::AgogeSession};

// --- body battery -----------------------------------------------------------

const RECHARGE_PER_HOUR: f64 = 20.0;
const MAX_RECHARGE: f64 = 80.0;
const DRAIN_PER_KCAL: f64 = 0.06;
const DRAIN_PER_10K_STEPS: f64 = 25.0;

#[derive(Debug, sqlx::FromRow)]
struct DayAggregate {
    day: NaiveDate,
    sleep_s: f64,
    kcal: f64,
    steps: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BodyEnergyPoint {
    day: NaiveDate,
    score: f64,
    recharge: f64,
    drain: f64,
}

#[derive(Debug, Deserialize)]
pub struct RangeQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

pub async fn body_battery(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(92) {
        return Err(ApiError::BadRequest("body battery range exceeds 92 days".to_string()));
    }

    let days: Vec<DayAggregate> = sqlx::query_as(
        "SELECT
            date_trunc('day', ts)::date AS day,
            COALESCE(SUM(value) FILTER (WHERE metric = 'sleep_seconds'), 0)::float8 AS sleep_s,
            COALESCE(SUM(value) FILTER (WHERE metric = 'active_calories'), 0)::float8 AS kcal,
            COALESCE(SUM(value) FILTER (WHERE metric = 'steps'), 0)::float8 AS steps
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('sleep_seconds', 'active_calories', 'steps')
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&pool)
    .await?;

    let mut points = Vec::with_capacity(days.len());
    for d in &days {
        let recharge = (d.sleep_s / 3600.0 * RECHARGE_PER_HOUR).min(MAX_RECHARGE);
        let drain = (d.kcal * DRAIN_PER_KCAL) + (d.steps / 10_000.0 * DRAIN_PER_10K_STEPS);
        let score = (100.0 + recharge - drain).clamp(0.0, 100.0);
        points.push(BodyEnergyPoint { day: d.day, score, recharge, drain });
        sqlx::query(
            "INSERT INTO body_energy (user_id, day, score, recharge, drain, updated_at)
             VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (user_id, day)
             DO UPDATE SET score = EXCLUDED.score, recharge = EXCLUDED.recharge,
                           drain = EXCLUDED.drain, updated_at = now()",
        )
        .bind(user.0)
        .bind(d.day)
        .bind(score)
        .bind(recharge)
        .bind(drain)
        .execute(&pool)
        .await?;
    }

    Ok(Json(json!({ "days": points })))
}

// --- workout detection + acceptance -----------------------------------------

const DETECT_BUCKET_SECS: i64 = 300; // 5-minute buckets
const DETECT_HR_THRESHOLD: f64 = 120.0; // bpm
const DETECT_MIN_BUCKETS: usize = 2; // 10 minutes of sustained effort
const CLASSIFY_MAX_DIST: f64 = 1.5; // nearest-type distance cap

#[derive(Debug, sqlx::FromRow)]
struct HrBucket {
    ts: f64,
    avg_hr: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct TypeProfile {
    type_id: Uuid,
    avg_hr: f64,
    movement_per_min: f64,
    steps_per_min: f64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct DetectionRow {
    id: Uuid,
    detected_start: DateTime<Utc>,
    detected_end: DateTime<Utc>,
    confidence: f64,
    status: String,
    proposed_type_id: Option<Uuid>,
    metrics: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Detection {
    id: Uuid,
    start: f64,
    end: f64,
    peak_hr: f64,
    confidence: f64,
    status: String,
    proposed_type_id: Option<Uuid>,
    proposed_type_name: Option<String>,
}

pub async fn workouts(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    validate_range(&q)?;

    let buckets: Vec<HrBucket> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM time_bucket($1::interval, ts)) * 1000)::float8 AS ts,
            AVG(value)::float8 AS avg_hr
         FROM measurements
         WHERE user_id = $2 AND metric = 'heart_rate' AND ts >= $3 AND ts < $4
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(format!("{DETECT_BUCKET_SECS} seconds"))
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&pool)
    .await?;

    // Group contiguous above-threshold buckets into candidate windows.
    #[derive(Debug)]
    struct Window {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        peak_hr: f64,
        avg_hr: f64,
    }
    let bucket_ms = (DETECT_BUCKET_SECS as f64) * 1000.0;
    let mut windows: Vec<Window> = Vec::new();
    let mut run: Vec<&HrBucket> = Vec::new();

    let flush = |run: &mut Vec<&HrBucket>, windows: &mut Vec<Window>| {
        if run.len() >= DETECT_MIN_BUCKETS {
            let peak = run.iter().map(|b| b.avg_hr).fold(0.0, f64::max);
            let avg = run.iter().map(|b| b.avg_hr).sum::<f64>() / run.len() as f64;
            let start = DateTime::from_timestamp_millis(run.first().unwrap().ts as i64).unwrap();
            let end = DateTime::from_timestamp_millis(
                run.last().unwrap().ts as i64 + bucket_ms as i64,
            )
            .unwrap();
            windows.push(Window { start, end, peak_hr: peak, avg_hr: avg });
        }
        run.clear();
    };

    for b in &buckets {
        if b.avg_hr >= DETECT_HR_THRESHOLD {
            if let Some(prev) = run.last() {
                if b.ts - prev.ts > bucket_ms * 2.0 {
                    flush(&mut run, &mut windows);
                }
            }
            run.push(b);
        } else if !run.is_empty() {
            flush(&mut run, &mut windows);
        }
    }
    flush(&mut run, &mut windows);

    let profiles = type_profiles(&pool, user.0).await?;

    for w in &windows {
        let confidence = ((w.peak_hr - DETECT_HR_THRESHOLD) / 40.0).clamp(0.4, 0.98);
        let proposed = classify(&pool, user.0, &profiles, w.start, w.end, w.avg_hr).await?;
        let peak_hr = w.peak_hr;
        sqlx::query(
            "INSERT INTO workout_detections
                (user_id, detected_start, detected_end, confidence, metrics, status, proposed_type_id)
             VALUES ($1, $2, $3, $4, $5, 'proposed', $6)
             ON CONFLICT (user_id, detected_start)
             DO UPDATE SET detected_end = EXCLUDED.detected_end,
                           confidence = EXCLUDED.confidence,
                           metrics = EXCLUDED.metrics,
                           proposed_type_id = EXCLUDED.proposed_type_id",
        )
        .bind(user.0)
        .bind(w.start)
        .bind(w.end)
        .bind(confidence)
        .bind(json!({ "peakHr": peak_hr, "avgHr": w.avg_hr }))
        .bind(proposed)
        .execute(&pool)
        .await?;
    }

    list_detections_inner(&pool, user.0, q.from, q.to).await
}

pub async fn accept_detection(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AgogeSession>> {
    let det: Option<(Uuid, DateTime<Utc>, DateTime<Utc>, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT id, detected_start, detected_end, proposed_type_id, status
         FROM workout_detections WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.0)
    .fetch_optional(&pool)
    .await?;

    let Some((_id, start, end, proposed_type_id, status)) = det else {
        return Err(ApiError::NotFound("detection not found".to_string()));
    };
    if status != "proposed" {
        return Err(ApiError::BadRequest(format!("detection already {status}")));
    }

    let session: AgogeSession = sqlx::query_as(
        "INSERT INTO agoge_sessions (user_id, type_id, start_time, end_time, status)
         VALUES ($1, $2, $3, $4, 'closed')
         RETURNING id, user_id, type_id, start_time, end_time, status, created_at, updated_at",
    )
    .bind(user.0)
    .bind(proposed_type_id)
    .bind(start)
    .bind(end)
    .fetch_one(&pool)
    .await?;

    sqlx::query(
        "UPDATE workout_detections SET status = 'accepted', session_id = $1 WHERE id = $2",
    )
    .bind(session.id)
    .bind(id)
    .execute(&pool)
    .await?;

    Ok(Json(session))
}

pub async fn reject_detection(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let res = sqlx::query(
        "UPDATE workout_detections SET status = 'rejected'
         WHERE id = $1 AND user_id = $2 AND status = 'proposed'",
    )
    .bind(id)
    .bind(user.0)
    .execute(&pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound(
            "proposed detection not found for this user".to_string(),
        ));
    }
    Ok(Json(json!({ "rejected": true })))
}

/// Lists persisted detections (proposed/accepted/rejected) in range.
async fn list_detections_inner(
    pool: &PgPool,
    user_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> ApiResult<Json<Value>> {
    let rows: Vec<DetectionRow> = sqlx::query_as(
        "SELECT id, detected_start, detected_end, confidence, status, proposed_type_id, metrics
         FROM workout_detections
         WHERE user_id = $1 AND detected_start >= $2 AND detected_start < $3
         ORDER BY detected_start DESC",
    )
    .bind(user_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let peak_hr = r.metrics.get("peakHr").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let type_name = match r.proposed_type_id {
            Some(tid) => {
                let name: Option<(String,)> = sqlx::query_as(
                    "SELECT name FROM agoge_types WHERE id = $1",
                )
                .bind(tid)
                .fetch_optional(pool)
                .await?;
                name.map(|n| n.0)
            }
            None => None,
        };
        out.push(Detection {
            id: r.id,
            start: r.detected_start.timestamp_millis() as f64,
            end: r.detected_end.timestamp_millis() as f64,
            peak_hr,
            confidence: r.confidence,
            status: r.status.clone(),
            proposed_type_id: r.proposed_type_id,
            proposed_type_name: type_name,
        });
    }

    Ok(Json(json!({ "detections": out })))
}

/// Historical per-type signal profiles from the user's own closed sessions.
async fn type_profiles(pool: &PgPool, user_id: Uuid) -> ApiResult<Vec<TypeProfile>> {
    let rows: Vec<TypeProfile> = sqlx::query_as(
        "WITH feat AS (
            SELECT s.type_id,
                   AVG(m.value) FILTER (WHERE m.metric = 'heart_rate') AS avg_hr,
                   COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'movement_intensity'), 0) AS movement,
                   COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'steps'), 0) AS steps,
                   (EXTRACT(EPOCH FROM (s.end_time - s.start_time)) / 60.0) AS mins
            FROM agoge_sessions s
            JOIN measurements m ON m.user_id = s.user_id AND m.ts >= s.start_time AND m.ts < s.end_time
            WHERE s.user_id = $1 AND s.end_time IS NOT NULL AND s.type_id IS NOT NULL
            GROUP BY s.id, s.type_id, s.start_time, s.end_time
        )
        SELECT type_id,
               AVG(avg_hr) AS avg_hr,
               AVG(movement / GREATEST(mins, 1.0)) AS movement_per_min,
               AVG(steps / GREATEST(mins, 1.0)) AS steps_per_min
        FROM feat
        WHERE mins >= 3
        GROUP BY type_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Matches a detected window to the nearest historical type profile.
async fn classify(
    pool: &PgPool,
    user_id: Uuid,
    profiles: &[TypeProfile],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    avg_hr: f64,
) -> ApiResult<Option<Uuid>> {
    if profiles.is_empty() {
        return Ok(None);
    }

    let mins = (end - start).num_seconds() as f64 / 60.0;
    let mins = mins.max(1.0);

    let feat: (Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT
            COALESCE(SUM(value) FILTER (WHERE metric = 'movement_intensity'), 0)::float8,
            COALESCE(SUM(value) FILTER (WHERE metric = 'steps'), 0)::float8
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3",
    )
    .bind(user_id)
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await?;

    let movement_per_min = feat.0.unwrap_or(0.0) / mins;
    let steps_per_min = feat.1.unwrap_or(0.0) / mins;

    let mut best: Option<(f64, Uuid)> = None;
    for p in profiles {
        let d_hr = (avg_hr - p.avg_hr) / 40.0;
        let d_mov = (movement_per_min - p.movement_per_min) / 50.0;
        let d_steps = (steps_per_min - p.steps_per_min) / 20.0;
        let dist = (d_hr * d_hr + d_mov * d_mov + d_steps * d_steps).sqrt();
        if best.map(|(bd, _)| dist < bd).unwrap_or(true) {
            best = Some((dist, p.type_id));
        }
    }

    Ok(best.and_then(|(d, id)| (d <= CLASSIFY_MAX_DIST).then_some(id)))
}

fn validate_range(q: &RangeQuery) -> ApiResult<()> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(92) {
        return Err(ApiError::BadRequest("range exceeds 92 days".to_string()));
    }
    Ok(())
}
