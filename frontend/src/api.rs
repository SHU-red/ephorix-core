//! Typed API client for the EphoriX backend. Every call sends the POC token
//! via the X-EphoriX-Token header.

use gloo_net::http::Request;
use js_sys::Date;
use serde::Deserialize;
use serde_json::Value;
use wasm_bindgen::JsValue;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgogeType {
    pub id: String,
    pub name: String,
    pub color_code: String,
    pub icon: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgogeSession {
    pub id: String,
    pub type_id: Option<String>,
    pub start_time: String,
    pub end_time: Option<String>,
    pub status: String,
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

#[derive(Debug, Deserialize)]
pub struct TimelineResponse {
    pub bucket: String,
    pub points: Vec<TimelinePoint>,
    pub sessions: Vec<AgogeSession>,
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

pub async fn post_json(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let req = Request::post(&format!("{base}{path}"))
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
