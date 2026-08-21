//! Derived metrics over the normalized `measurements` store:
//!   - body battery (sleep recharge vs activity drain, Garmin-style)
//!   - automated workout detection + acceptance: contiguous elevated-HR
//!     windows are proposed, classified against the user's own historical
//!     sessions, and the user accepts (→ Agoge session) or rejects them.
//! All are source-agnostic — they only read canonical metric names.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    models::AgogeSession,
    normalize::{METRIC_HRV, METRIC_RESTING_HR},
};

// --- body battery -----------------------------------------------------------
// Full = 300 (the 300 Spartans). Recharge from sleep, discharge from activity
// and stress (an HR-elevation strain score — no HRV on the watch).

const RECHARGE_PER_HOUR: f64 = 60.0;
const MAX_RECHARGE: f64 = 300.0;
const DRAIN_PER_KCAL: f64 = 0.12;
const DRAIN_PER_10K_STEPS: f64 = 50.0;
const DRAIN_PER_STRESS: f64 = 0.5; // stress → battery points (300 stress ≈ 150 drain/hr)
const DRAIN_PER_MOVE: f64 = 0.005; // movement intensity (au) → battery points
const STRESS_RESTING_HR: f64 = 55.0; // bpm below which stress is 0
const STRESS_FULL_HR: f64 = 120.0;   // bpm at which the daily 0..100 stress saturates
const STRESS_SERIES_FULL_HR: f64 = 175.0; // bpm at which series stress is 300 (0..300 scale)

#[derive(Debug, sqlx::FromRow)]
struct DayAggregate {
    day: NaiveDate,
    sleep_s: f64,
    kcal: f64,
    steps: f64,
    avg_hr: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BodyEnergyPoint {
    day: NaiveDate,
    score: f64,
    recharge: f64,
    drain: f64,
    stress: f64,
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
            COALESCE(SUM(value) FILTER (WHERE metric = 'steps'), 0)::float8 AS steps,
            COALESCE(AVG(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8 AS avg_hr
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('sleep_seconds', 'active_calories', 'steps', 'heart_rate')
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
        let stress = if d.avg_hr > STRESS_RESTING_HR {
            ((d.avg_hr - STRESS_RESTING_HR) / (STRESS_FULL_HR - STRESS_RESTING_HR) * 100.0)
                .clamp(0.0, 100.0)
        } else {
            0.0
        };
        let stress_drain = stress * DRAIN_PER_STRESS;
        let score = (300.0 + recharge - drain - stress_drain).clamp(0.0, 300.0);
        points.push(BodyEnergyPoint { day: d.day, score, recharge, drain, stress });
        sqlx::query(
            "INSERT INTO body_energy (user_id, day, score, recharge, drain, stress, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (user_id, day)
             DO UPDATE SET score = EXCLUDED.score, recharge = EXCLUDED.recharge,
                           drain = EXCLUDED.drain, stress = EXCLUDED.stress, updated_at = now()",
        )
        .bind(user.0)
        .bind(d.day)
        .bind(score)
        .bind(recharge)
        .bind(drain)
        .bind(stress)
        .execute(&pool)
        .await?;
    }

    Ok(Json(json!({ "days": points })))
}

// --- live body-battery series (integral over time) -------------------------

#[derive(Debug, Deserialize)]
pub struct SeriesQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(default)]
    pub bucket: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct SeriesBucket {
    ts: f64,
    sleep_s: f64,
    kcal: f64,
    steps: f64,
    movement: f64,
    avg_hr: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatterySeriesPoint {
    ts: f64,      // epoch ms
    stress: f64,  // 0..300
    battery: f64, // 0..300
}

/// A continuous ("always live") body battery: a running integral over
/// contiguous time buckets (empty ones forward-fill stress and hold the
/// battery flat). Sleep recharges, activity (kcal/steps/movement) and stress
/// (HR strain) discharge; the value is clamped to 0..300 each step.
pub async fn body_battery_series(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<SeriesQuery>,
) -> ApiResult<Json<Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(366) {
        return Err(ApiError::BadRequest("range exceeds 366 days".to_string()));
    }
    let bucket = q.bucket.unwrap_or_else(|| "1 hour".to_string());
    let points = battery_series_inner(&pool, user.0, &q.from, &q.to, &bucket).await?;
    Ok(Json(json!({ "series": points })))
}

/// Core of [`body_battery_series`]: contiguous buckets plus the
/// recharge/drain integral. Split out so readiness baselines can reuse the
/// exact same stress/battery values the series endpoint serves.
async fn battery_series_inner(
    pool: &PgPool,
    user_id: Uuid,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
    bucket: &str,
) -> ApiResult<Vec<BatterySeriesPoint>> {
    // Contiguous buckets: `generate_series` emits every bucket in [from, to)
    // and the LEFT JOIN keeps the empty ones (zeroed sums, null HR) so the
    // series has no gaps.
    let rows: Vec<SeriesBucket> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM gs.b) * 1000)::float8 AS ts,
            COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'sleep_seconds'), 0)::float8 AS sleep_s,
            COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'active_calories'), 0)::float8 AS kcal,
            COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'steps'), 0)::float8 AS steps,
            COALESCE(SUM(m.value) FILTER (WHERE m.metric = 'movement_intensity'), 0)::float8 AS movement,
            AVG(m.value) FILTER (WHERE m.metric = 'heart_rate')::float8 AS avg_hr
         FROM generate_series($3, $4 - $1::interval, $1::interval) AS gs(b)
         LEFT JOIN measurements m
           ON m.user_id = $2
           AND m.metric IN ('sleep_seconds', 'active_calories', 'steps', 'movement_intensity', 'heart_rate')
           AND m.ts >= gs.b
           AND m.ts < gs.b + $1::interval
         GROUP BY gs.b
         ORDER BY gs.b",
    )
    .bind(bucket)
    .bind(user_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let mut battery = 300.0;
    let mut last_stress = 0.0;
    let mut points = Vec::with_capacity(rows.len());
    for r in &rows {
        let empty = r.avg_hr.is_none()
            && r.sleep_s == 0.0
            && r.kcal == 0.0
            && r.steps == 0.0
            && r.movement == 0.0;
        let stress = match r.avg_hr {
            // Linear HR-elevation strain: 55 bpm → 0, 175 bpm → 300.
            Some(hr) if hr > STRESS_RESTING_HR =>
                ((hr - STRESS_RESTING_HR) / (STRESS_SERIES_FULL_HR - STRESS_RESTING_HR) * 300.0)
                    .clamp(0.0, 300.0),
            Some(_) => 0.0,
            None => last_stress, // no HR sample: hold the last stress, never null
        };
        // Empty buckets carry the battery forward unchanged (no drain, no
        // recharge); their stress was already forward-filled above.
        if !empty {
            let recharge = r.sleep_s / 3600.0 * RECHARGE_PER_HOUR;
            let drain = r.kcal * DRAIN_PER_KCAL
                + r.steps / 10_000.0 * DRAIN_PER_10K_STEPS
                + r.movement * DRAIN_PER_MOVE;
            battery = (battery + recharge - drain - stress * DRAIN_PER_STRESS).clamp(0.0, 300.0);
        }
        last_stress = stress;
        points.push(BatterySeriesPoint { ts: r.ts, stress, battery });
    }
    Ok(points)
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

// --- readiness + baselines ---------------------------------------------------
// The user's own trailing-90-day distributions (resting HR, stress, battery,
// sleep, active kcal) become percentiles; each day's readiness score is
// normalized against them: sleep recharges, stress and activity load drain a
// full 300.

const BASELINE_WINDOW_DAYS: i64 = 90;
const READINESS_FULL: f64 = 300.0;
const RECHARGE_PER_P50: f64 = 120.0; // points per 1.0x of the user's p50 sleep
const RECHARGE_CAP_RATIO: f64 = 1.5; // sleep above 1.5x p50 earns no more points
const STRESS_DRAIN_MAX: f64 = 100.0;
const ACTIVITY_DRAIN_MAX: f64 = 80.0;
const DEFAULT_SLEEP_P50_S: f64 = 8.0 * 3600.0; // no sleep history yet
const DEFAULT_STRESS_P90: f64 = 150.0; // 0..300 scale (see STRESS_SERIES_FULL_HR)
const DEFAULT_KCAL_P90: f64 = 400.0; // daily active kcal

/// Per-user baselines over the trailing window: (p10, p50, p90) triples for
/// the daily resting-HR proxy, the body-battery series stress and battery
/// values, plus the sleep p50 and active-kcal p90 readiness normalizes by.
#[derive(Debug)]
struct Baselines {
    resting_hr: Option<[f64; 3]>,
    stress: Option<[f64; 3]>,
    battery: Option<[f64; 3]>,
    sleep_s_p50: Option<f64>,
    kcal_p90: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
struct RestingRow {
    day: NaiveDate,
    proxy: Option<f64>,     // low-5th-percentile heart_rate of the day
    resting_hr: Option<f64>, // explicit resting_hr average for the day
    hrv: Option<f64>,
}

/// Linear-interpolated (p10, p50, p90), the same method as Postgres
/// `percentile_cont`. None when `values` is empty.
fn percentiles(values: &[f64]) -> Option<[f64; 3]> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |p: f64| {
        let idx = p * (v.len() - 1) as f64;
        let lo = idx.floor() as usize;
        let hi = lo.min(v.len() - 1);
        let frac = idx - lo as f64;
        if lo == hi {
            v[lo]
        } else {
            v[lo] * (1.0 - frac) + v[hi] * frac
        }
    };
    Some([at(0.10), at(0.50), at(0.90)])
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn percentile_obj(p: Option<[f64; 3]>) -> Value {
    match p {
        Some([p10, p50, p90]) => json!({ "p10": p10, "p50": p50, "p90": p90 }),
        None => Value::Null,
    }
}

/// Per-UTC-day aggregates over [from, to) for the metrics readiness and
/// baselines normalize against (days with no such data simply have no row).
async fn fetch_day_aggregates(
    pool: &PgPool,
    user_id: Uuid,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
) -> ApiResult<Vec<DayAggregate>> {
    let rows: Vec<DayAggregate> = sqlx::query_as(
        "SELECT
            date_trunc('day', ts)::date AS day,
            COALESCE(SUM(value) FILTER (WHERE metric = 'sleep_seconds'), 0)::float8 AS sleep_s,
            COALESCE(SUM(value) FILTER (WHERE metric = 'active_calories'), 0)::float8 AS kcal,
            COALESCE(SUM(value) FILTER (WHERE metric = 'steps'), 0)::float8 AS steps,
            COALESCE(AVG(value) FILTER (WHERE metric = 'heart_rate'), 0)::float8 AS avg_hr
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('sleep_seconds', 'active_calories', 'steps', 'heart_rate')
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(user_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Per-UTC-day resting-HR proxy (5th percentile of heart_rate), explicit
/// resting_hr average, and HRV average.
async fn fetch_resting_rows(
    pool: &PgPool,
    user_id: Uuid,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
) -> ApiResult<Vec<RestingRow>> {
    let rows: Vec<RestingRow> = sqlx::query_as(&format!(
        "WITH base AS (
            SELECT date_trunc('day', ts)::date AS day, metric, value
            FROM measurements
            WHERE user_id = $1 AND ts >= $2 AND ts < $3
              AND metric IN ('heart_rate', '{METRIC_HRV}', '{METRIC_RESTING_HR}')
         )
         SELECT b.day,
                (SELECT PERCENTILE_CONT(0.05) WITHIN GROUP (ORDER BY h.value)
                   FROM base h
                   WHERE h.day = b.day AND h.metric = 'heart_rate')::float8 AS proxy,
                (SELECT AVG(r.value)
                   FROM base r
                   WHERE r.day = b.day AND r.metric = '{METRIC_RESTING_HR}')::float8 AS resting_hr,
                AVG(b.value) FILTER (WHERE b.metric = '{METRIC_HRV}')::float8 AS hrv
         FROM base b
         GROUP BY 1
         ORDER BY 1",
    ))
    .bind(user_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Baselines over the trailing window ending at `to`. Stress and battery
/// percentiles come straight out of the body-battery series (1-hour buckets)
/// so they always agree with what the series endpoint shows.
async fn compute_baselines(
    pool: &PgPool,
    user_id: Uuid,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
) -> ApiResult<Baselines> {
    let daily = fetch_day_aggregates(pool, user_id, from, to).await?;
    let resting = fetch_resting_rows(pool, user_id, from, to).await?;

    // Daily resting HR: the explicit metric wins, else the low-percentile
    // heart_rate proxy.
    let resting_hr: Vec<f64> = resting.iter().filter_map(|r| r.resting_hr.or(r.proxy)).collect();
    let sleep_s: Vec<f64> = daily.iter().filter(|d| d.sleep_s > 0.0).map(|d| d.sleep_s).collect();
    let kcal: Vec<f64> = daily.iter().filter(|d| d.kcal > 0.0).map(|d| d.kcal).collect();

    let series = battery_series_inner(pool, user_id, from, to, "1 hour").await?;
    let stress: Vec<f64> = series.iter().map(|p| p.stress).collect();
    let battery: Vec<f64> = series.iter().map(|p| p.battery).collect();

    // The series is never empty (generate_series fills the whole window);
    // only report its percentiles when the user actually has data in it.
    let has_data = !daily.is_empty() || !resting.is_empty();
    let (stress_p, battery_p) = if has_data {
        (percentiles(&stress), percentiles(&battery))
    } else {
        (None, None)
    };

    Ok(Baselines {
        resting_hr: percentiles(&resting_hr),
        stress: stress_p,
        battery: battery_p,
        sleep_s_p50: percentiles(&sleep_s).map(|p| p[1]),
        kcal_p90: percentiles(&kcal).map(|p| p[2]),
    })
}

/// GET /api/v1/metrics/baselines — the user's own trailing-90-day
/// p10/p50/p90 for resting HR, stress and body battery (null for a signal the
/// user has no data for yet).
pub async fn baselines(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
) -> ApiResult<Json<Value>> {
    let to = Utc::now();
    let from = to - Duration::days(BASELINE_WINDOW_DAYS);
    let b = compute_baselines(&pool, user.0, &from, &to).await?;
    Ok(Json(json!({
        "restingHr": percentile_obj(b.resting_hr),
        "stress": percentile_obj(b.stress),
        "battery": percentile_obj(b.battery),
    })))
}

/// GET /api/v1/metrics/readiness?from&to — per-day 0..300 readiness score,
/// adaptive to the user's own baselines. HRV rides along when present,
/// null otherwise (graceful degradation for HRV-less sources).
pub async fn readiness(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<SeriesQuery>,
) -> ApiResult<Json<Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(366) {
        return Err(ApiError::BadRequest("range exceeds 366 days".to_string()));
    }

    // One baseline pass per request: the trailing 90 days ending at `to`,
    // so every day normalizes against the same distributions.
    let base_from = q.from - Duration::days(BASELINE_WINDOW_DAYS);
    let b = compute_baselines(&pool, user.0, &base_from, &q.to).await?;
    let sleep_p50 = b.sleep_s_p50.unwrap_or(DEFAULT_SLEEP_P50_S);
    let stress_p90 = b.stress.map(|p| p[2]).filter(|v| *v > 0.0).unwrap_or(DEFAULT_STRESS_P90);
    let kcal_p90 = b.kcal_p90.filter(|v| *v > 0.0).unwrap_or(DEFAULT_KCAL_P90);

    let daily = fetch_day_aggregates(&pool, user.0, &q.from, &q.to).await?;
    let resting = fetch_resting_rows(&pool, user.0, &q.from, &q.to).await?;
    let day_map: HashMap<NaiveDate, &DayAggregate> = daily.iter().map(|d| (d.day, d)).collect();
    let rest_map: HashMap<NaiveDate, &RestingRow> =
        resting.iter().map(|r| (r.day, r)).collect();

    let mut days = Vec::new();
    let mut d = q.from.date_naive();
    let last = q.to.date_naive();
    while d <= last {
        let agg = day_map.get(&d).copied();
        let rest = rest_map.get(&d).copied();
        let sleep_s = agg.map(|a| a.sleep_s).unwrap_or(0.0);
        let kcal = agg.map(|a| a.kcal).unwrap_or(0.0);
        let avg_hr = agg.map(|a| a.avg_hr);

        // Recharge: sleep against the user's own p50 (1.0x = 120 pts, capped
        // at 1.5x). Drains: the day's HR strain against the stress p90
        // (0..100) and active kcal against the kcal p90 (0..80).
        let recharge = (sleep_s / sleep_p50).clamp(0.0, RECHARGE_CAP_RATIO) * RECHARGE_PER_P50;
        let stress = match avg_hr {
            Some(hr) if hr > STRESS_RESTING_HR =>
                ((hr - STRESS_RESTING_HR) / (STRESS_SERIES_FULL_HR - STRESS_RESTING_HR) * 300.0)
                    .clamp(0.0, 300.0),
            _ => 0.0,
        };
        let stress_drain = (stress / stress_p90).clamp(0.0, 1.0) * STRESS_DRAIN_MAX;
        let activity_drain = (kcal / kcal_p90).clamp(0.0, 1.0) * ACTIVITY_DRAIN_MAX;
        let score = (READINESS_FULL - stress_drain - activity_drain + recharge)
            .clamp(0.0, READINESS_FULL);

        let day = d;
        days.push(json!({
            "date": day.format("%Y-%m-%d").to_string(),
            "score": round1(score),
            "recharge": round1(recharge),
            "stressDrain": round1(stress_drain),
            "activityDrain": round1(activity_drain),
            "restingHr": rest.and_then(|r| r.resting_hr.or(r.proxy)).map(round1),
            "hrv": rest.and_then(|r| r.hrv).map(round1),
            "components": [
                json!({ "key": "sleep", "label": "Sleep recharge", "value": round1(recharge), "max": RECHARGE_PER_P50 * RECHARGE_CAP_RATIO, "direction": "+" }),
                json!({ "key": "stress", "label": "Stress", "value": round1(stress_drain), "max": STRESS_DRAIN_MAX, "direction": "-" }),
                json!({ "key": "activity", "label": "Activity", "value": round1(activity_drain), "max": ACTIVITY_DRAIN_MAX, "direction": "-" })
            ],
        }));
        d += Duration::days(1);
    }

    Ok(Json(json!({ "days": days })))
}
