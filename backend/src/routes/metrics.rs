//! Derived metrics over the normalized `measurements` store:
//!   - body battery (sleep recharge vs activity drain, Garmin-style)
//!   - automated workout detection (contiguous elevated-HR windows)
//! Both are source-agnostic — they only read canonical metric names.

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::{auth::AuthUser, error::{ApiError, ApiResult}};

// --- body battery -----------------------------------------------------------

// Model constants (documented; tuned for a wearable day).
const RECHARGE_PER_HOUR: f64 = 20.0; // points/hour of sleep, capped below
const MAX_RECHARGE: f64 = 80.0;
const DRAIN_PER_KCAL: f64 = 0.06; // ~500 kcal active ≈ 30 points
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
) -> ApiResult<Json<serde_json::Value>> {
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
        // Cache for later reads.
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

// --- workout detection ------------------------------------------------------

const DETECT_BUCKET_SECS: i64 = 300; // 5-minute buckets
const DETECT_HR_THRESHOLD: f64 = 120.0; // bpm
const DETECT_MIN_BUCKETS: usize = 2; // 10 minutes of sustained effort

#[derive(Debug, sqlx::FromRow)]
struct HrBucket {
    ts: f64, // epoch ms
    avg_hr: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Detection {
    start: f64,     // epoch ms
    end: f64,       // epoch ms
    peak_hr: f64,
    confidence: f64,
}

pub async fn workouts(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    if (q.to - q.from) > Duration::days(92) {
        return Err(ApiError::BadRequest("workout detection range exceeds 92 days".to_string()));
    }

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

    // Group contiguous above-threshold buckets (one-bucket gap tolerated).
    let mut detections: Vec<Detection> = Vec::new();
    let mut run: Vec<&HrBucket> = Vec::new();

    let flush = |run: &mut Vec<&HrBucket>, detections: &mut Vec<Detection>| {
        if run.len() >= DETECT_MIN_BUCKETS {
            let peak = run.iter().map(|b| b.avg_hr).fold(0.0, f64::max);
            let start = run.first().unwrap().ts;
            let end = run.last().unwrap().ts + (DETECT_BUCKET_SECS as f64) * 1000.0;
            let confidence = ((peak - DETECT_HR_THRESHOLD) / 40.0).clamp(0.4, 0.98);
            detections.push(Detection { start, end, peak_hr: peak, confidence });
        }
        run.clear();
    };

    let bucket_ms = (DETECT_BUCKET_SECS as f64) * 1000.0;
    for b in &buckets {
        if b.avg_hr >= DETECT_HR_THRESHOLD {
            if let Some(prev) = run.last() {
                // allow a single missing bucket as a gap
                if b.ts - prev.ts > bucket_ms * 2.0 {
                    flush(&mut run, &mut detections);
                }
            }
            run.push(b);
        } else if !run.is_empty() {
            flush(&mut run, &mut detections);
        }
    }
    flush(&mut run, &mut detections);

    // Persist detections (idempotent per window: replace overlapping rows).
    for d in &detections {
        let start: DateTime<Utc> = DateTime::from_timestamp_millis(d.start as i64).unwrap();
        let end: DateTime<Utc> = DateTime::from_timestamp_millis(d.end as i64).unwrap();
        sqlx::query(
            "INSERT INTO workout_detections (user_id, detected_start, detected_end, confidence, metrics)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT DO NOTHING",
        )
        .bind(user.0)
        .bind(start)
        .bind(end)
        .bind(d.confidence)
        .bind(json!({ "peakHr": d.peak_hr }))
        .execute(&pool)
        .await?;
    }

    Ok(Json(json!({ "detections": detections })))
}
