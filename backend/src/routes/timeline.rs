//! Timeline aggregation for the web UI. Raw data is bucketed into contiguous
//! intervals (`generate_series` + LEFT JOIN over TimescaleDB, so empty buckets
//! still emit rows), and Agoge sessions overlapping the range are returned for
//! overlay rendering.

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
    models::AgogeSession,
};

const MAX_SPAN_DAYS: i64 = 366;

/// Raw-point aggregation for `get_timeline`. Hoisted so tests can execute the
/// exact production SQL (regression coverage for the NULL-counter baselines).
///
/// steps/active_calories on the watch are CUMULATIVE day totals
/// (`health_service_sum(metric, today, now)` per sample), so summing the rows
/// per bucket would double-count. Convert each row to the delta accrued since
/// the previous sample (first-of-day: since midnight) and spread that delta
/// evenly over its interval — a row at t with delta d covers [prev_t, t), a
/// first-of-day row covers [midnight, t). Rows pushed without counters (HR-only
/// history, steps/active_calories NULL) must not reset the baseline, so the
/// baseline is the running MAX over the strictly preceding rows (LAG has no
/// IGNORE NULLS); for the same reason the interval start is the previous row
/// where THAT metric is non-NULL (`step_lo`/`kcal_lo`) — the previous row alone
/// would squeeze a multi-minute delta into the seconds since an HR-only push
/// and spike the bucket at the snapshot boundary. The query pulls one
/// extra day before `from` so the first in-window row's delta is computed
/// against its true predecessor.
pub(crate) const TIMELINE_SQL: &str = "WITH raw AS (
        SELECT timestamp, heart_rate,
               GREATEST(steps - COALESCE(MAX(steps) OVER (PARTITION BY d ORDER BY timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), 0), 0)::bigint AS step_delta,
               GREATEST(active_calories - COALESCE(MAX(active_calories) OVER (PARTITION BY d ORDER BY timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), 0.0), 0.0)::float8 AS kcal_delta,
               COALESCE(MAX(timestamp) FILTER (WHERE steps IS NOT NULL) OVER (PARTITION BY d ORDER BY timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), d) AS step_lo,
               COALESCE(MAX(timestamp) FILTER (WHERE active_calories IS NOT NULL) OVER (PARTITION BY d ORDER BY timestamp ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), d) AS kcal_lo,
               d
        FROM (SELECT *, date_trunc('day', timestamp) AS d FROM raw_health_data
              WHERE user_id = $2
                AND timestamp >= $3 - interval '1 day'
                AND timestamp <  $4) r
     ),
     spread AS (
        SELECT gs.b,
               COALESCE(SUM(r.step_delta * GREATEST(EXTRACT(EPOCH FROM (LEAST(gs.b + $1::interval, r.timestamp) - GREATEST(gs.b, r.step_lo))), 0)
                   / GREATEST(EXTRACT(EPOCH FROM (r.timestamp - r.step_lo)), 1)), 0)::bigint AS steps,
               COALESCE(SUM(r.kcal_delta * GREATEST(EXTRACT(EPOCH FROM (LEAST(gs.b + $1::interval, r.timestamp) - GREATEST(gs.b, r.kcal_lo))), 0)
                   / GREATEST(EXTRACT(EPOCH FROM (r.timestamp - r.kcal_lo)), 1)), 0)::float8 AS active_calories
        FROM generate_series($3, $4 - $1::interval, $1::interval) AS gs(b)
        LEFT JOIN raw r ON gs.b < r.timestamp AND gs.b + $1::interval > LEAST(r.step_lo, r.kcal_lo)
        GROUP BY gs.b
     )
     SELECT
        (EXTRACT(EPOCH FROM gs.b) * 1000)::float8 AS ts,
        AVG(r.heart_rate)::float8 AS heart_rate,
        sp.steps,
        sp.active_calories
     FROM generate_series($3, $4 - $1::interval, $1::interval) AS gs(b)
     LEFT JOIN raw r
       ON r.timestamp >= gs.b
       AND r.timestamp < gs.b + $1::interval
     LEFT JOIN spread sp ON sp.b = gs.b
     GROUP BY gs.b, sp.steps, sp.active_calories
     ORDER BY gs.b";

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// time_bucket interval, e.g. "10 seconds", "1 minute", "1 hour".
    /// Defaults to a bucket that keeps the response bounded.
    #[serde(default)]
    pub bucket: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePoint {
    pub ts: f64, // epoch ms
    pub heart_rate: Option<f64>,
    pub steps: Option<i64>,
    pub active_calories: Option<f64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NutritionEvent {
    pub ts: f64, // epoch ms
    pub kind: String,
    pub amount: f64,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SleepDay {
    pub ts: f64, // epoch ms of the day start (UTC)
    pub sleep_seconds: f64,
    pub restful_seconds: f64,
}

pub async fn get_timeline(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<TimelineQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    if q.to <= q.from {
        return Err(ApiError::BadRequest("'to' must be after 'from'".to_string()));
    }
    let span = q.to - q.from;
    if span > Duration::days(MAX_SPAN_DAYS) {
        return Err(ApiError::BadRequest(format!(
            "range exceeds {MAX_SPAN_DAYS} days"
        )));
    }

    let bucket = validate_bucket(q.bucket.as_deref(), span)?;

    // Contiguous buckets: `generate_series` emits every bucket in [from, to)
    // and LEFT JOIN keeps empty ones (null HR, zeroed counters) so gaps render
    // as honest breaks instead of lines spanning missing data. See
    // `TIMELINE_SQL` for the cumulative-counter delta handling.
    let points: Vec<TimelinePoint> = sqlx::query_as(TIMELINE_SQL)
        .bind(&bucket)
        .bind(user.0)
        .bind(q.from)
        .bind(q.to)
        .fetch_all(&pool)
        .await?;

    let sessions: Vec<AgogeSession> = sqlx::query_as(
        "SELECT * FROM agoge_sessions
         WHERE user_id = $1
           AND start_time < $2
           AND (end_time IS NULL OR end_time >= $3)
         ORDER BY start_time",
    )
    .bind(user.0)
    .bind(q.to)
    .bind(q.from)
    .fetch_all(&pool)
    .await?;

    // Nutrition events (meals / water) for overlay markers.
    let nutrition: Vec<NutritionEvent> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM consumed_at) * 1000)::float8 AS ts,
            kind, amount, note
         FROM nutrition_log
         WHERE user_id = $1 AND consumed_at >= $2 AND consumed_at < $3
         ORDER BY consumed_at",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&pool)
    .await?;

    // Daily sleep totals (sleep is a daily sum; the client renders night bands).
    let sleep: Vec<SleepDay> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM date_trunc('day', ts)) * 1000)::float8 AS ts,
            COALESCE(MAX(value) FILTER (WHERE metric = 'sleep_seconds'), 0)::float8 AS sleep_seconds,
            COALESCE(MAX(value) FILTER (WHERE metric = 'restful_sleep_seconds'), 0)::float8 AS restful_seconds
         FROM measurements
         WHERE user_id = $1 AND ts >= $2 AND ts < $3
           AND metric IN ('sleep_seconds', 'restful_sleep_seconds')
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(user.0)
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&pool)
    .await?;

    Ok(Json(json!({
        "bucket": bucket,
        "points": points,
        "sessions": sessions,
        "nutrition": nutrition,
        "sleep": sleep,
    })))
}

/// Normalizes a client bucket string; rejects buckets too fine for the span
/// (browser lag guard) and caps the returned point count.
fn validate_bucket(bucket: Option<&str>, span: Duration) -> ApiResult<String> {
    let parsed = bucket.unwrap_or("1 minute").trim();
    let seconds = parse_interval_seconds(parsed).ok_or_else(|| {
        ApiError::BadRequest(format!("unparseable bucket '{parsed}'; use e.g. '5 seconds', '1 minute', '1 hour'"))
    })?;
    if seconds <= 0 {
        return Err(ApiError::BadRequest("bucket must be positive".to_string()));
    }
    let span_seconds = span.num_seconds().max(1) as f64;
    let point_count = span_seconds / seconds as f64;
    // Keep responses lean for the browser: >= 2000 buckets would stall uPlot.
    if point_count > 2000.0 {
        let coarse_seconds = (span_seconds / 1000.0).ceil().max(1.0) as i64;
        let coarse = format_interval_seconds(coarse_seconds);
        return Err(ApiError::BadRequest(format!(
            "bucket '{parsed}' would produce {point_count:.0} points; use at least '{coarse}'"
        )));
    }
    Ok(parsed.to_string())
}

fn parse_interval_seconds(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }
    let n: i64 = parts[0].parse().ok()?;
    let secs = match parts[1].to_ascii_lowercase().as_str() {
        "seconds" | "second" | "sec" | "s" => n,
        "minutes" | "minute" | "min" | "m" => n * 60,
        "hours" | "hour" | "h" => n * 3600,
        "days" | "day" | "d" => n * 86400,
        _ => return None,
    };
    Some(secs)
}

fn format_interval_seconds(secs: i64) -> String {
    if secs >= 86400 && secs % 86400 == 0 {
        format!("{} day", secs / 86400)
    } else if secs >= 3600 && secs % 3600 == 0 {
        format!("{} hour", secs / 3600)
    } else if secs >= 60 && secs % 60 == 0 {
        format!("{} minute", secs / 60)
    } else {
        format!("{secs} seconds")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the 2026-08-29 production defect: `LAG(steps)` has no
    /// IGNORE NULLS, so an HR-only push (steps NULL) between two cumulative
    /// snapshots made the later one look like a full-day total. Measured live:
    /// summed steps 49,849 vs the true 8,402 (~5.9x inflation).
    ///
    /// The fixture uses those exact numbers. The baseline row sits on the window
    /// start, so its own accrued interval [midnight, 06:00) falls outside the
    /// single bucket (the join is `gs.b < r.timestamp`, strict) while still
    /// supplying the running-MAX baseline for the 08:00 row: the summed delta is
    /// 49,849 - 41,447 = 8,402. The NULL-blind baseline instead reads the 07:00
    /// HR-only row, gets NULL -> 0, and reports the full 49,849.
    ///
    /// DB-gated: skipped unless EPHORIX_TEST_DATABASE_URL is set.
    #[tokio::test]
    async fn step_deltas_ignore_null_rows() {
        let url = match std::env::var("EPHORIX_TEST_DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping: EPHORIX_TEST_DATABASE_URL unset");
                return;
            }
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect test db");
        // Rolled back at the end, so the synthetic user and rows never persist.
        let mut tx = pool.begin().await.expect("begin test tx");
        // `date_trunc('day', ...)` follows the session TimeZone; pin it so the
        // fixture's day partition is deterministic.
        sqlx::query("SET LOCAL timezone = 'UTC'")
            .execute(&mut *tx)
            .await
            .expect("pin session timezone");
        let user_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO users (token, display_name) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("timeline-test-{}", uuid::Uuid::new_v4()))
        .bind("timeline regression test")
        .fetch_one(&mut *tx)
        .await
        .expect("insert synthetic user");

        // 2026-08-29 pattern: cumulative snapshot, HR-only row with NULL
        // counters, later cumulative snapshot. Kept clear of midnight so every
        // row shares the one day partition (session TimeZone pinned to UTC).
        for (ts, hr, steps, kcal) in [
            ("2026-08-29T06:00:00Z", 100i16, Some(41447i32), Some(1200.0f32)),
            ("2026-08-29T07:00:00Z", 120i16, None, None),
            ("2026-08-29T08:00:00Z", 110i16, Some(49849i32), Some(1277.0f32)),
        ] {
            sqlx::query(
                "INSERT INTO raw_health_data (timestamp, user_id, heart_rate, steps, active_calories)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(ts.parse::<DateTime<Utc>>().expect("parse fixture ts"))
            .bind(user_id)
            .bind(hr)
            .bind(steps)
            .bind(kcal)
            .execute(&mut *tx)
            .await
            .expect("insert fixture row");
        }

        // Window starts on the baseline row so the single "1 day" bucket holds
        // only the later rows' deltas.
        let from = "2026-08-29T06:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let to = "2026-08-30T06:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let points: Vec<TimelinePoint> = sqlx::query_as(TIMELINE_SQL)
            .bind("1 day")
            .bind(user_id)
            .bind(from)
            .bind(to)
            .fetch_all(&mut *tx)
            .await
            .expect("run TIMELINE_SQL");

        let steps: i64 = points.iter().map(|p| p.steps.unwrap_or(0)).sum();
        let kcal: f64 = points.iter().map(|p| p.active_calories.unwrap_or(0.0)).sum();
        // NULL-blind LAG would report the full 49,849 here, and 1,277 kcal.
        assert_eq!(steps, 8402, "step deltas must ignore NULL-counter rows");
        assert!(
            (kcal - 77.0).abs() < 0.01,
            "kcal deltas must ignore NULL-counter rows, got {kcal}"
        );

        // Same fixture at a bucket finer than the snapshot gap: the delta is
        // accrued over [step_lo, timestamp) = [06:00, 08:00), so each hourly
        // bucket carries half (8,402/2). A row-based interval start instead
        // reads the 07:00 HR-only row as `lo` and crams the whole 8,402 into
        // the 07:00 bucket — but the daily totals above stay identical, so
        // only this finer-bucket check catches it.
        let hourly: Vec<TimelinePoint> = sqlx::query_as(TIMELINE_SQL)
            .bind("1 hour")
            .bind(user_id)
            .bind(from)
            .bind("2026-08-29T08:00:00Z".parse::<DateTime<Utc>>().unwrap())
            .fetch_all(&mut *tx)
            .await
            .expect("run TIMELINE_SQL hourly");
        let hourly_steps: Vec<i64> = hourly.iter().map(|p| p.steps.unwrap_or(0)).collect();
        assert_eq!(
            hourly_steps,
            vec![4201, 4201],
            "step deltas must spread over the previous same-metric row, not the previous row"
        );

        tx.rollback().await.expect("rollback test tx");
    }

    #[test]
    fn bucket_rejects_fine_granularity_over_long_span() {
        let span = Duration::days(30); // 2.6M secs
        let err = validate_bucket(Some("1 second"), span).unwrap_err();
        assert!(err.to_string().contains("use at least"));
    }

    #[test]
    fn bucket_accepts_reasonable_granularity() {
        let span = Duration::days(30);
        assert!(validate_bucket(Some("1 hour"), span).is_ok());
        let span = Duration::hours(2);
        assert!(validate_bucket(Some("5 seconds"), span).is_ok());
    }

    #[test]
    fn bucket_parses_units() {
        assert_eq!(parse_interval_seconds("2 minutes"), Some(120));
        assert_eq!(parse_interval_seconds("1 hour"), Some(3600));
        assert_eq!(parse_interval_seconds("1 day"), Some(86400));
        assert_eq!(parse_interval_seconds("banana"), None);
    }
}
