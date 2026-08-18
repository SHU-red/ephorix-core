//! Timeline aggregation for the web UI. Raw data is bucketed with the
//! TimescaleDB `time_bucket` function (server-side downsampling), and Agoge
//! sessions overlapping the range are returned for overlay rendering.

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

    let points: Vec<TimelinePoint> = sqlx::query_as(
        "SELECT
            (EXTRACT(EPOCH FROM time_bucket($1::interval, timestamp)) * 1000)::float8 AS ts,
            AVG(heart_rate)::float8 AS heart_rate,
            SUM(steps)::bigint AS steps,
            SUM(active_calories)::float8 AS active_calories
         FROM raw_health_data
         WHERE user_id = $2 AND timestamp >= $3 AND timestamp < $4
         GROUP BY ts
         ORDER BY ts",
    )
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

    Ok(Json(json!({
        "bucket": bucket,
        "points": points,
        "sessions": sessions,
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
