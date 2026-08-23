//! Typed API client for the EphoriX backend. Every call sends the POC token
//! via the X-EphoriX-Token header.

use gloo_net::http::Request;
use js_sys::Date;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wasm_bindgen::JsValue;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgogeType {
    pub id: String,
    pub name: String,
    pub color_code: String,
    pub icon: String,
    pub category: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default = "default_hr_interval")]
    pub hr_sampling_interval: i64,
}

fn default_hr_interval() -> i64 {
    60
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgogeSession {
    pub id: String,
    pub type_id: Option<String>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub status: String,
    // Watch stop summary (StopSummaryJson -> agoge_sessions columns, all
    // NULL until the watch reports a workout end). SessionDetails shows
    // these directly; /stats keeps computing live from measurements.
    #[serde(default)]
    pub duration_sec: Option<i64>,
    #[serde(default)]
    pub workout_kcal: Option<f64>,
    #[serde(default)]
    pub avg_hr: Option<i64>,
    #[serde(default)]
    pub reps: Option<i64>,
    #[serde(default)]
    pub movement_intensity: Option<f64>,
    #[serde(default)]
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePoint {
    /// epoch ms
    pub ts: f64,
    pub heart_rate: Option<f64>,
    pub steps: Option<i64>,
    pub active_calories: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NutritionEvent {
    pub ts: f64,
    pub kind: String,
    pub amount: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepDay {
    pub ts: f64,
    pub sleep_seconds: f64,
    pub restful_seconds: f64,
}

#[derive(Debug, Deserialize)]
pub struct TimelineResponse {
    pub bucket: String,
    pub points: Vec<TimelinePoint>,
    pub sessions: Vec<AgogeSession>,
    #[serde(default)]
    pub nutrition: Vec<NutritionEvent>,
    #[serde(default)]
    pub sleep: Vec<SleepDay>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub peak_hr: f64,
    pub confidence: f64,
    pub status: String,
    pub proposed_type_id: Option<String>,
    pub proposed_type_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulsePoint {
    /// ISO timestamp of the (possibly bucketed) sample.
    pub t: String,
    pub hr: i64,
}

/// Per-session pulse derived by the backend from raw_health_data: series
/// stats plus the (bucketed, ≤120 point) series itself. All fields are
/// absent/null when the session window has no heart-rate rows.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPulse {
    #[serde(default)]
    pub avg_hr: Option<f64>,
    #[serde(default)]
    pub min_hr: Option<i64>,
    #[serde(default)]
    pub max_hr: Option<i64>,
    #[serde(default)]
    pub series: Vec<PulsePoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub duration_sec: i64,
    pub active_sec: i64,
    pub pause_sec: i64,
    pub reps: i64,
    pub calories: f64,
    pub avg_hr: f64,
    pub peak_hr: i64,
    pub sets: i64,
    pub total_reps: i64,
    pub volume_kg: f64,
    #[serde(default)]
    pub pulse: SessionPulse,
}

// ---------------------------------------------------------------------------
// Timestamp helpers (avoid chrono in wasm)
// ---------------------------------------------------------------------------

pub fn iso_from_ms(ms: f64) -> String {
    Date::new(&JsValue::from_f64(ms))
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

pub fn ms_from_iso(s: &str) -> Option<f64> {
    let v = Date::parse(s);
    if v.is_nan() { None } else { Some(v) }
}

/// Human-ish "YYYY-MM-DD HH:MM" for labels.
pub fn fmt_time(ms: f64) -> String {
    let iso = iso_from_ms(ms);
    iso.chars().take(16).collect()
}

/// Rounds a target bucket length (seconds) up to a whole unit so the server
/// never rejects it and the point count stays bounded (<= ~800 points).
pub fn nice_bucket(target_secs: f64) -> String {
    if target_secs >= 86_400.0 {
        format!("{} day", (target_secs / 86_400.0).ceil() as i64)
    } else if target_secs >= 3_600.0 {
        format!("{} hour", (target_secs / 3_600.0).ceil() as i64)
    } else if target_secs >= 60.0 {
        format!("{} minute", (target_secs / 60.0).ceil() as i64)
    } else {
        format!("{} seconds", target_secs.ceil() as i64)
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

async fn check(resp: gloo_net::http::Response) -> Result<Value, String> {
    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {text}"));
    }
    resp.json::<Value>().await.map_err(|e| format!("invalid json: {e}"))
}

pub async fn fetch_types(base: &str, token: &str) -> Result<Vec<AgogeType>, String> {
    let v = Request::get(&format!("{base}/api/v1/agoge-types"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["types"].clone()).map_err(|e| format!("types decode: {e}"))
}

pub async fn fetch_timeline(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
    bucket: &str,
) -> Result<TimelineResponse, String> {
    let from = iso_from_ms(from_ms);
    let to = iso_from_ms(to_ms);
    let v = Request::get(&format!("{base}/api/v1/timeline"))
        .header("X-EphoriX-Token", token)
        .query([("from", from.as_str()), ("to", to.as_str()), ("bucket", bucket)])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("timeline decode: {e}"))
}

pub async fn fetch_settings(base: &str, token: &str) -> Result<Value, String> {
    let v = Request::get(&format!("{base}/api/v1/settings"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    Ok(v["settings"].clone())
}

pub async fn put_settings(base: &str, token: &str, settings: &Value) -> Result<Value, String> {
    let req = Request::put(&format!("{base}/api/v1/settings"))
        .header("X-EphoriX-Token", token)
        .json(&json!({ "settings": settings }))
        .map_err(|e| format!("serialize body: {e}"))?;
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    check(resp).await
}

pub async fn post_json(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let req = Request::post(&format!("{base}{path}"))
        .header("X-EphoriX-Token", token)
        .json(body)
        .map_err(|e| format!("serialize body: {e}"))?;
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    check(resp).await
}

pub async fn put_json(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let req = Request::put(&format!("{base}{path}"))
        .header("X-EphoriX-Token", token)
        .json(body)
        .map_err(|e| format!("serialize body: {e}"))?;
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    check(resp).await
}

pub async fn patch_json(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let req = Request::patch(&format!("{base}{path}"))
        .header("X-EphoriX-Token", token)
        .json(body)
        .map_err(|e| format!("serialize body: {e}"))?;
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    check(resp).await
}

pub async fn delete_json(base: &str, token: &str, path: &str) -> Result<Value, String> {
    let resp = Request::delete(&format!("{base}{path}"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    check(resp).await
}

pub async fn fetch_workouts(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
) -> Result<Vec<Detection>, String> {
    let v = Request::get(&format!("{base}/api/v1/metrics/workouts"))
        .header("X-EphoriX-Token", token)
        .query([
            ("from", iso_from_ms(from_ms).as_str()),
            ("to", iso_from_ms(to_ms).as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["detections"].clone()).map_err(|e| format!("detections decode: {e}"))
}

pub async fn accept_detection(base: &str, token: &str, id: &str) -> Result<Value, String> {
    post_json(base, token, &format!("/api/v1/metrics/workouts/{id}/accept"), &json!({})).await
}

pub async fn reject_detection(base: &str, token: &str, id: &str) -> Result<Value, String> {
    post_json(base, token, &format!("/api/v1/metrics/workouts/{id}/reject"), &json!({})).await
}

pub async fn fetch_session_stats(base: &str, token: &str, id: &str) -> Result<SessionStats, String> {
    let v = Request::get(&format!("{base}/api/v1/agoge-sessions/{id}/stats"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("stats decode: {e}"))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseSet {
    pub id: String,
    pub set_number: i32,
    pub reps: i32,
    pub weight_kg: Option<f64>,
    pub rest_sec: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    pub id: String,
    pub name: String,
    pub sets: Vec<ExerciseSet>,
}

pub async fn fetch_exercises(base: &str, token: &str, session_id: &str) -> Result<Vec<Exercise>, String> {
    let v = Request::get(&format!("{base}/api/v1/agoge-sessions/{session_id}/exercises"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["exercises"].clone()).map_err(|e| format!("exercises decode: {e}"))
}

pub async fn add_exercise(base: &str, token: &str, session_id: &str, body: &Value) -> Result<Value, String> {
    post_json(base, token, &format!("/api/v1/agoge-sessions/{session_id}/exercises"), body).await
}

pub async fn update_exercise(
    base: &str,
    token: &str,
    session_id: &str,
    exercise_id: &str,
    body: &Value,
) -> Result<Value, String> {
    patch_json(base, token, &format!("/api/v1/agoge-sessions/{session_id}/exercises/{exercise_id}"), body).await
}

pub async fn delete_exercise(base: &str, token: &str, session_id: &str, exercise_id: &str) -> Result<Value, String> {
    delete_json(base, token, &format!("/api/v1/agoge-sessions/{session_id}/exercises/{exercise_id}")).await
}

pub async fn parse_ai(base: &str, token: &str, text: &str) -> Result<Value, String> {
    post_json(base, token, "/api/v1/ai/parse", &json!({ "text": text })).await
}
// ---------------------------------------------------------------------------
// Pythia oracle (AI chat about training state)
// ---------------------------------------------------------------------------

/// One message in an oracle chat. `role` is "user" or "assistant".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// A settings change the oracle proposes; the user reviews it in the UI
/// before it is persisted.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProposal {
    /// Dotted settings path, e.g. "settings.rangeDays" or "settings.series.heartRate".
    pub key: String,
    /// Human label, e.g. "Timeline range".
    pub label: String,
    #[serde(default)]
    pub current: Value,
    #[serde(default)]
    pub proposed: Value,
    #[serde(default)]
    pub reason: String,
    /// "measurement" | "meal" when this is an action proposal (accepted by
    /// POSTing to /measurements or /nutrition instead of a settings PUT).
    #[serde(default)]
    pub action: Option<String>,
    /// "weight_kg" | "body_fat_pct" for measurement proposals.
    #[serde(default)]
    pub metric: Option<String>,
    /// Suggested numeric value for action proposals (kcal for meals).
    #[serde(default)]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatResponse {
    pub reply: String,
    #[serde(default)]
    pub proposals: Vec<AiProposal>,
}

/// POST /api/v1/ai/chat with the message history + a bounded state context.
pub async fn ai_chat(
    base: &str,
    token: &str,
    messages: &[ChatMessage],
    context: &Value,
) -> Result<AiChatResponse, String> {
    let v = post_json(base, token, "/api/v1/ai/chat", &json!({ "messages": messages, "context": context }))
        .await?;
    serde_json::from_value(v).map_err(|e| format!("ai chat decode: {e}"))
}

/// POST /api/v1/ai/test — one cheap round-trip to the configured provider.
/// Returns {"ok":true,"reply":str} on success.
pub async fn ai_test(
    base: &str,
    token: &str,
    provider: &str,
    base_url: &str,
    model: &str,
    api_key: &str,
) -> Result<Value, String> {
    let mut body = json!({ "provider": provider, "baseUrl": base_url, "model": model });
    if !api_key.trim().is_empty() {
        body["apiKey"] = json!(api_key.trim());
    }
    post_json(base, token, "/api/v1/ai/test", &body).await
}


#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyEnergyDay {
    pub score: f64,
    pub recharge: f64,
    pub drain: f64,
    pub stress: f64,
}

pub async fn fetch_body_battery(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
) -> Result<Option<BodyEnergyDay>, String> {
    let v = Request::get(&format!("{base}/api/v1/metrics/body-battery"))
        .header("X-EphoriX-Token", token)
        .query([
            ("from", iso_from_ms(from_ms).as_str()),
            ("to", iso_from_ms(to_ms).as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    let day = v.get("days").and_then(|d| d.as_array()).and_then(|d| d.last()).cloned();
    Ok(day.and_then(|d| serde_json::from_value(d).ok()))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatterySeriesPoint {
    pub ts: f64,
    pub stress: f64,
    pub battery: f64,
}

pub async fn fetch_body_battery_series(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
    bucket: &str,
) -> Result<Vec<BatterySeriesPoint>, String> {
    let v = Request::get(&format!("{base}/api/v1/metrics/body-battery-series"))
        .header("X-EphoriX-Token", token)
        .query([
            ("from", iso_from_ms(from_ms).as_str()),
            ("to", iso_from_ms(to_ms).as_str()),
            ("bucket", bucket),
        ])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["series"].clone()).map_err(|e| format!("series decode: {e}"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessComponent {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub max: f64,
    pub direction: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessDay {
    pub date: String,
    pub score: f64,
    pub recharge: f64,
    pub stress_drain: f64,
    pub activity_drain: f64,
    pub resting_hr: Option<f64>,
    pub hrv: Option<f64>,
    pub components: Vec<ReadinessComponent>,
}

pub async fn fetch_readiness(
    base: &str,
    token: &str,
    from_ms: f64,
    to_ms: f64,
) -> Result<Vec<ReadinessDay>, String> {
    let from = iso_from_ms(from_ms);
    let to = iso_from_ms(to_ms);
    let v = Request::get(&format!("{base}/api/v1/metrics/readiness"))
        .header("X-EphoriX-Token", token)
        .query([("from", from.as_str()), ("to", to.as_str())])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["days"].clone()).map_err(|e| format!("readiness decode: {e}"))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselinePercentile {
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baselines {
    pub resting_hr: Option<BaselinePercentile>,
    pub stress: Option<BaselinePercentile>,
    pub battery: Option<BaselinePercentile>,
}

pub async fn fetch_baselines(base: &str, token: &str) -> Result<Baselines, String> {
    let v = Request::get(&format!("{base}/api/v1/metrics/baselines"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("baselines decode: {e}"))
}

// ---------------------------------------------------------------------------
// Nutrition daily log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NutritionMeal {
    pub id: String,
    /// water | food | meal (meal = food entry with a meal type)
    pub r#type: String,
    pub meal_type: Option<String>,
    /// kcal for food/meal, ml for water
    pub amount: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
    pub note: Option<String>,
    pub consumed_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NutritionDaily {
    pub date: String,
    pub kcal: f64,
    pub protein: f64,
    pub carbs: f64,
    pub fat: f64,
    pub water_ml: f64,
    pub water_goal_ml: f64,
    pub meals: Vec<NutritionMeal>,
}

pub async fn fetch_daily_nutrition(
    base: &str,
    token: &str,
    date_iso: &str,
) -> Result<NutritionDaily, String> {
    let v = Request::get(&format!("{base}/api/v1/nutrition/daily"))
        .header("X-EphoriX-Token", token)
        .query([("date", date_iso)])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v).map_err(|e| format!("nutrition decode: {e}"))
}

/// POST /api/v1/nutrition. `body` must carry `kind` ("water" | "food"),
/// `amount`, and optionally protein/carbs/fat (g), `mealType` (marks a food
/// entry as a meal), `note`, `consumedAt`.
pub async fn add_nutrition(base: &str, token: &str, body: &Value) -> Result<Value, String> {
    post_json(base, token, "/api/v1/nutrition", body).await
}

// ---------------------------------------------------------------------------
// User-logged measurements (weight / body fat) + persisted action log
// ---------------------------------------------------------------------------

/// One row from `GET /api/v1/measurements` (newest first when `limit` is set).
/// `ts` may be an epoch-ms number (the long-form store) or an ISO string —
/// `ts_ms()` normalizes both for display.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    #[serde(default)]
    pub id: Option<String>,
    pub metric: String,
    pub value: f64,
    #[serde(default)]
    pub ts: Value,
}

impl Measurement {
    pub fn ts_ms(&self) -> Option<f64> {
        match &self.ts {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => ms_from_iso(s),
            _ => None,
        }
    }
}

/// POST /api/v1/measurements {metric, value, ts?} — logs a user measurement.
pub async fn post_measurement(
    base: &str,
    token: &str,
    metric: &str,
    value: f64,
    ts: Option<String>,
) -> Result<Value, String> {
    let mut body = json!({ "metric": metric, "value": value });
    if let Some(t) = ts {
        body["ts"] = json!(t);
    }
    post_json(base, token, "/api/v1/measurements", &body).await
}

/// GET /api/v1/measurements?metric=&limit= — rows newest first.
pub async fn fetch_measurements(
    base: &str,
    token: &str,
    metric: &str,
    limit: usize,
) -> Result<Vec<Measurement>, String> {
    let limit_s = limit.to_string();
    let v = Request::get(&format!("{base}/api/v1/measurements"))
        .header("X-EphoriX-Token", token)
        .query([("metric", metric), ("limit", limit_s.as_str())])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    // The store returns rows under "points"; newer endpoints may use
    // "measurements" or a bare array — accept all three.
    let arr = v.get("measurements").or_else(|| v.get("points")).cloned().unwrap_or(v);
    serde_json::from_value(arr).map_err(|e| format!("measurements decode: {e}"))
}

/// One persisted action from `GET /api/v1/actions` (settings PUTs, nutrition
/// POSTs, measurement POSTs are auto-logged server-side).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionLogEntry {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: String,
    #[serde(default)]
    pub reverted_at: Option<String>,
}

pub async fn fetch_actions(base: &str, token: &str) -> Result<Vec<ActionLogEntry>, String> {
    let v = Request::get(&format!("{base}/api/v1/actions"))
        .header("X-EphoriX-Token", token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid json: {e}"))?;
    serde_json::from_value(v["actions"].clone()).map_err(|e| format!("actions decode: {e}"))
}

/// POST /api/v1/actions/{id}/revert — undoes one logged action.
pub async fn revert_action(base: &str, token: &str, id: &str) -> Result<Value, String> {
    post_json(base, token, &format!("/api/v1/actions/{id}/revert"), &json!({})).await
}

// ---------------------------------------------------------------------------
// Generic import (CSV / JSON / GPX exports flattened to canonical samples)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub inserted: usize,
    pub skipped: usize,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// POST /api/v1/import. `source` must be one of: csv, gpx, health_connect,
/// apple_health, garmin, manual, pebble, fitbit. `samples` are already
/// canonical ({timestamp, metric, value, unit?, meta?}); invalid ones are
/// skipped per-sample and reported in `errors` (capped at 20).
pub async fn import_samples(
    base: &str,
    token: &str,
    source: &str,
    device_id: Option<&str>,
    samples: &[Value],
) -> Result<ImportResult, String> {
    let body = json!({
        "source": source,
        "deviceId": device_id,
        "samples": samples,
    });
    let v = post_json(base, token, "/api/v1/import", &body).await?;
    serde_json::from_value(v).map_err(|e| format!("import decode: {e}"))
}
