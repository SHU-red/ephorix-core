//! AI layer ("PYTHIA"), the oracle of the EphoriX training and health app.
//!
//! Endpoints:
//! - `POST /api/v1/ai/parse` — the user describes a meal or a drink in plain
//!   language; the provider returns a structured nutrition estimate, which
//!   the web UI then logs via `/api/v1/nutrition`.
//! - `POST /api/v1/ai/chat` — answers questions about the user's
//!   training/health data, given an app-state digest in the request. If the
//!   model recommends concrete settings changes it must end its reply with a
//!   `[PYTHIA]{...}[/PYTHIA]` line; that block is extracted, validated
//!   (whitelisted keys, type-checked, clamped) and returned separately as
//!   proposals the UI can apply in one tap.
//! - `POST /api/v1/ai/test` — reachability check for the provider config
//!   form, before the config is saved.
//!
//! Proposal shapes:
//! - Settings keys (`settings.*`) keep the shape
//!   `{key, label, current, proposed, reason}` — `current` is read from the
//!   user's settings JSONB.
//! - Action keys log new data instead of changing settings and carry
//!   `{key, label, current, proposed, reason, action, metric, value}`:
//!   - `action.measurement.weight_kg` → `action: "measurement"`,
//!     `metric: "weight_kg"`, `value` the clamped kg number, `current` the
//!     latest logged weight or null.
//!   - `action.measurement.body_fat_pct` → same shape with
//!     `metric: "body_fat_pct"` (percent).
//!   - `action.meal` → `action: "meal"`, `metric: "meal"`, `value` the
//!     estimated kcal, `current` always null (meals are events, not values).
//!   The UI maps `action` + `metric` + `value` to `POST /api/v1/measurements`
//!   (weight_kg / body_fat_pct) or `POST /api/v1/nutrition` (meal → kind
//!   `food`, amount = kcal, note from `reason`).
//!
//! Provider config lives in the per-user settings JSONB under `aiProvider`:
//!   { "provider": "llamacpp" | "ollama" | "openai",
//!     "baseUrl": "http://localhost:8080/v1", "model": "llama3", "apiKey": "" }
//! `provider` defaults to `"openai"`. `baseUrl` is the chat-completions root
//! (no trailing /chat/completions). Local (llama.cpp server, Ollama, LM
//! Studio) and remote (OpenAI and other OpenAI-compatible) providers are all
//! supported — everything stays local if you point it at a local provider.

use axum::{
    extract::{Extension, State},
    Json,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::LazyLock;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiResult},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseRequest {
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// Bounded digest of app state the frontend assembles; embedded compactly
    /// into the system prompt.
    #[serde(default)]
    pub context: Value,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderRequest {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiProvider {
    /// Provider family: "llamacpp", "ollama", or "openai" (OpenAI-compatible).
    #[serde(default = "default_provider_name")]
    provider: String,
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: String,
}

fn default_provider_name() -> String {
    "openai".to_string()
}

pub async fn parse(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<ParseRequest>,
) -> ApiResult<Json<Value>> {
    let text = req.text.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest("text must not be empty".to_string()));
    }

    let provider = load_provider(&pool, user.0).await?;

    let prompt = format!(
        "Extract nutrition from this description. Reply with ONLY a JSON object, \
         no markdown, no prose: {{\"kind\": \"food\"|\"water\", \"amount\": number}} \
         where amount is kcal for food and ml for water. Estimate sensibly for a \
         single serving/glass if not specified.\n\nDescription: {text}"
    );

    let content =
        provider_chat(&provider, &[json!({ "role": "user", "content": prompt })], 0.0).await?;

    let (kind, amount) = extract_nutrition(&content)?;

    Ok(Json(json!({
        "kind": kind,
        "amount": amount,
        "note": text,
    })))
}

pub async fn chat(
    State(pool): State<PgPool>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<ChatRequest>,
) -> ApiResult<Json<Value>> {
    if req.messages.is_empty() {
        return Err(ApiError::BadRequest("messages must not be empty".to_string()));
    }
    for m in &req.messages {
        if m.role != "user" && m.role != "assistant" {
            return Err(ApiError::BadRequest(format!(
                "invalid message role: {}",
                m.role
            )));
        }
    }

    let provider = load_provider(&pool, user.0).await?;

    let mut messages: Vec<Value> = Vec::with_capacity(req.messages.len() + 1);
    messages.push(json!({ "role": "system", "content": system_prompt(&req.context) }));
    for m in &req.messages {
        messages.push(json!({ "role": m.role, "content": m.content }));
    }

    let content = provider_chat(&provider, &messages, 0.3).await?;

    let (reply, block) = extract_pythia_block(&content);
    let proposals = match block.as_deref() {
        Some(inner) => {
            let settings = load_settings(&pool, user.0).await?;
            validate_proposals(inner, &settings, &pool, user.0).await?
        }
        None => Vec::new(),
    };

    Ok(Json(json!({ "reply": reply, "proposals": proposals })))
}

pub async fn test_provider(Json(req): Json<TestProviderRequest>) -> ApiResult<Json<Value>> {
    if req.base_url.trim().is_empty() {
        return Err(ApiError::BadRequest("baseUrl is required".to_string()));
    }
    if req.model.trim().is_empty() {
        return Err(ApiError::BadRequest("model is required".to_string()));
    }

    let provider = AiProvider {
        provider: req.provider.trim().to_ascii_lowercase(),
        base_url: req.base_url,
        model: req.model,
        api_key: req.api_key,
    };

    let content = provider_chat(
        &provider,
        &[json!({ "role": "user", "content": "Reply with exactly one word: OK" })],
        0.0,
    )
    .await?;

    Ok(Json(json!({ "ok": true, "reply": content.trim().to_string() })))
}

/// One chat-completion round trip against the configured provider.
///
/// Routes by `provider.provider`: `ollama` speaks its native `/api/chat`
/// protocol, `llamacpp`/`openai` (and anything unrecognized) speak the
/// OpenAI-compatible `/chat/completions` protocol. Returns the assistant's
/// message content.
async fn provider_chat(
    provider: &AiProvider,
    messages: &[Value],
    temperature: f64,
) -> ApiResult<String> {
    let base = provider.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(ApiError::BadRequest(
            "AI provider baseUrl is empty".to_string(),
        ));
    }

    let is_ollama = provider.provider.eq_ignore_ascii_case("ollama");
    let name = if is_ollama {
        "ollama"
    } else if provider.provider.eq_ignore_ascii_case("llamacpp") {
        "llama.cpp"
    } else {
        "openai"
    };

    let (url, body) = if is_ollama {
        (
            format!("{base}/api/chat"),
            json!({
                "model": provider.model,
                "messages": messages,
                "stream": false,
                "options": { "temperature": temperature },
            }),
        )
    } else {
        (
            format!("{base}/chat/completions"),
            json!({
                "model": provider.model,
                "messages": messages,
                "temperature": temperature,
                "stream": false,
            }),
        )
    };

    let client = reqwest::Client::new();
    let mut req_builder = client.post(&url).json(&body);
    if !provider.api_key.trim().is_empty() {
        req_builder = req_builder.bearer_auth(provider.api_key.trim());
    }
    let resp = req_builder
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("{name} unreachable: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        return Err(ApiError::BadRequest(format!(
            "{name} returned HTTP {status}: {}",
            detail.chars().take(200).collect::<String>()
        )));
    }

    let parsed: Value = resp
        .json()
        .await
        .map_err(|e| ApiError::BadRequest(format!("{name} returned invalid JSON: {e}")))?;

    // OpenAI-compatible: /choices/0/message/content — Ollama: /message/content.
    parsed
        .pointer("/choices/0/message/content")
        .or_else(|| parsed.pointer("/message/content"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| ApiError::BadRequest(format!("{name} response missing content")))
}

async fn load_settings(pool: &PgPool, user_id: uuid::Uuid) -> ApiResult<Value> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT settings FROM user_settings WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let v = row.map(|(s,)| s).unwrap_or_else(|| json!({}));
    Ok(if v.is_object() { v } else { json!({}) })
}

async fn load_provider(pool: &PgPool, user_id: uuid::Uuid) -> ApiResult<AiProvider> {
    let settings = load_settings(pool, user_id).await?;
    settings
        .get("aiProvider")
        .and_then(|v| serde_json::from_value::<AiProvider>(v.clone()).ok())
        .ok_or_else(|| {
            ApiError::BadRequest(
                "no AI provider configured — set settings.aiProvider = { baseUrl, model, apiKey }"
                    .to_string(),
            )
        })
}

/// Settings keys PYTHIA is allowed to propose, plus the action keys that log
/// data. The system prompt advertises exactly this list; `build_proposal`
/// enforces it.
const PYTHIA_ALLOWED_KEYS: [&str; 20] = [
    "settings.rangeDays",
    "settings.series.heartRate",
    "settings.series.steps",
    "settings.series.calories",
    "settings.targets.steps",
    "settings.targets.kcal",
    "settings.targets.sleepH",
    "settings.targets.intensityHoursPerWeek",
    "settings.targets.weightKg",
    "settings.targets.bodyFatPct",
    "settings.nutrition.waterGoalMl",
    "settings.nutrition.kcalGoal",
    "settings.nutrition.proteinGoal",
    "settings.nutrition.carbGoal",
    "settings.nutrition.fatGoal",
    "settings.aiProvider.provider",
    "settings.aiProvider.model",
    "action.measurement.weight_kg",
    "action.measurement.body_fat_pct",
    "action.meal",
];

fn system_prompt(context: &Value) -> String {
    let app_state = if context.is_object() {
        serde_json::to_string(context).unwrap_or_else(|_| "{}".to_string())
    } else {
        "(no app state provided)".to_string()
    };
    format!(
        "You are PYTHIA, the oracle of the EphoriX training and health app.\n\
         The user is asking about their own training, health, and nutrition data.\n\
         \n\
         App state (JSON object of the user's current settings and recent data;\n\
         dotted keys are paths into it):\n{app_state}\n\
         \n\
         Rules:\n\
         - Be concise, direct, and evidence-based. Use only the app state above;\n\
         - never invent numbers that are not given.\n\
         - ALWAYS propose concrete values when recommending a change; NEVER ask\n\
         - clarifying questions — pick a sensible default for anything unspecified.\n\
         - Assume metric units throughout: kilograms (kg), kilocalories (kcal),\n\
         - hours (h), millilitres (ml). For meals, estimate calories from the\n\
         - user's description even when portion amounts are not stated.\n\
         - You may recommend concrete changes to exactly these keys:\n\
         - {keys}\n\
         - Settings keys (prefix \"settings.\") update the user's stored preferences.\n\
         - Action keys log new data instead of changing settings:\n\
         -   [PYTHIA]{{\"action.measurement.weight_kg\": 82.5}}[/PYTHIA]\n\
         -     logs a weight measurement (kg);\n\
         -   [PYTHIA]{{\"action.measurement.body_fat_pct\": 18}}[/PYTHIA]\n\
         -     logs a body-fat measurement (percent);\n\
         -   [PYTHIA]{{\"action.meal\": 650}}[/PYTHIA]\n\
         -     logs a meal estimated at 650 kcal.\n\
         - IF (and only if) you recommend concrete changes, end your reply with\n\
         - EXACTLY one line of the form\n\
         -   [PYTHIA]{{\"settings.rangeDays\": 30, \"action.meal\": 650}}[/PYTHIA]\n\
         - containing a single flat JSON object of dotted keys mapped to\n\
         - their new values (numbers, booleans, or short strings) and nothing else.\n\
         - No other brackets or text on that line.\n\
         - Never invent keys outside the list; the backend drops unknown keys.\n",
        keys = PYTHIA_ALLOWED_KEYS.join(", "),
    )
}

/// Splits a model reply into (visible text, settings block). Tolerates
/// markdown code fences around the `[PYTHIA]...[/PYTHIA]` line.
fn extract_pythia_block(content: &str) -> (String, Option<String>) {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)\[PYTHIA\]\s*(\{.*\})\s*\[/PYTHIA\]").unwrap());

    let Some(cap) = RE.captures(content) else {
        return (content.trim().to_string(), None);
    };

    let block = cap[1].to_string();
    let full = cap.get(0).expect("full match is always present");
    let (start, end) = (full.start(), full.end());
    let mut reply = format!("{}{}", &content[..start], &content[end..]);
    // Drop code-fence markers the model sometimes wraps the block in.
    reply = reply.replace("```json", "").replace("```", "");
    (reply.trim().to_string(), Some(block))
}

/// Validates the flat JSON object inside a `[PYTHIA]...[/PYTHIA]` line.
/// Unknown keys are dropped; out-of-range values are clamped and noted in
/// `reason`. `current` is read from the user's own settings JSONB (settings
/// keys) or the latest logged measurement (action keys).
async fn validate_proposals(
    block: &str,
    settings: &Value,
    pool: &PgPool,
    user_id: uuid::Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    let Some(obj) = serde_json::from_str::<Value>(block).ok().filter(|v| v.is_object()) else {
        return Ok(Vec::new());
    };
    let mut proposals = Vec::new();
    for (key, raw) in obj.as_object().unwrap() {
        if let Some(p) = build_proposal(key, raw, settings, pool, user_id).await? {
            proposals.push(p);
        }
    }
    Ok(proposals)
}

/// Whitelists one proposed key (settings or action). Returns `None` for
/// unknown keys or values that cannot be coerced to the expected type
/// (dropped silently).
async fn build_proposal(
    key: &str,
    raw: &Value,
    settings: &Value,
    pool: &PgPool,
    user_id: uuid::Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    if let Some((label, action, metric, proposed, note)) = action_proposal_fields(key, raw) {
        let current = if metric == "meal" {
            // Meals are events, not values — nothing to show as "current".
            Value::Null
        } else {
            match crate::routes::measurements::latest(pool, user_id, metric).await? {
                Some(v) => json!(v),
                None => Value::Null,
            }
        };
        return Ok(Some(json!({
            "key": key,
            "label": label,
            "current": current,
            "proposed": proposed,
            "reason": note.unwrap_or_else(|| "suggested by Pythia".to_string()),
            "action": action,
            "metric": metric,
            "value": proposed,
        })));
    }
    Ok(build_settings_proposal(key, raw, settings))
}

/// Static fields of an action proposal — label, action kind, metric and the
/// clamped proposed value — without touching the DB. The `current` field is
/// resolved separately from the latest logged measurement.
fn action_proposal_fields(
    key: &str,
    raw: &Value,
) -> Option<(&'static str, &'static str, &'static str, Value, Option<String>)> {
    match key {
        "action.measurement.weight_kg" => {
            let (p, note) = clamp_float(raw, 20.0, 400.0)?;
            Some(("Weight (kg)", "measurement", "weight_kg", p, note))
        }
        "action.measurement.body_fat_pct" => {
            let (p, note) = clamp_float(raw, 3.0, 60.0)?;
            Some(("Body fat (%)", "measurement", "body_fat_pct", p, note))
        }
        "action.meal" => {
            let (p, note) = clamp_float(raw, 0.0, 20000.0)?;
            Some(("Meal (kcal)", "meal", "meal", p, note))
        }
        _ => None,
    }
}

/// Whitelists one proposed settings key (no DB access). Returns `None` for
/// unknown keys or values that cannot be coerced to the expected type
/// (dropped silently).
fn build_settings_proposal(key: &str, raw: &Value, settings: &Value) -> Option<Value> {
    let (label, proposed, note) = match key {
        "settings.rangeDays" => {
            let v = as_num(raw)?;
            let c = nearest_range_days(v);
            let note = if (c as f64 - v).abs() > 1e-9 {
                Some(format!("out of range — set to nearest allowed {c} (requested {v})"))
            } else {
                None
            };
            ("Timeline range (days)", json!(c), note)
        }
        "settings.series.heartRate" => {
            let b = as_bool(raw)?;
            ("Heart rate series", json!(b), None)
        }
        "settings.series.steps" => {
            let b = as_bool(raw)?;
            ("Steps series", json!(b), None)
        }
        "settings.series.calories" => {
            let b = as_bool(raw)?;
            ("Calories series", json!(b), None)
        }
        "settings.targets.steps" => {
            let (p, note) = clamp_int(raw, 1, 100_000)?;
            ("Steps target", p, note)
        }
        "settings.targets.kcal" => {
            let (p, note) = clamp_int(raw, 1, 100_000)?;
            ("Calories target (kcal)", p, note)
        }
        "settings.targets.sleepH" => {
            let (p, note) = clamp_float(raw, 0.0, 24.0)?;
            ("Sleep target (hours)", p, note)
        }
        "settings.targets.intensityHoursPerWeek" => {
            let (p, note) = clamp_float(raw, 0.0, 168.0)?;
            ("Weekly intensity target (h)", p, note)
        }
        "settings.targets.weightKg" => {
            let (p, note) = clamp_float(raw, 20.0, 400.0)?;
            ("Weight target (kg)", p, note)
        }
        "settings.targets.bodyFatPct" => {
            let (p, note) = clamp_float(raw, 3.0, 60.0)?;
            ("Body fat target (%)", p, note)
        }
        "settings.nutrition.waterGoalMl" => {
            let (p, note) = clamp_int(raw, 100, 10_000)?;
            ("Water goal (ml)", p, note)
        }
        "settings.nutrition.kcalGoal" => {
            let (p, note) = clamp_int(raw, 500, 8000)?;
            ("Calories goal (kcal)", p, note)
        }
        "settings.nutrition.proteinGoal" => {
            let (p, note) = clamp_int(raw, 0, 2000)?;
            ("Protein goal (g)", p, note)
        }
        "settings.nutrition.carbGoal" => {
            let (p, note) = clamp_int(raw, 0, 2000)?;
            ("Carbs goal (g)", p, note)
        }
        "settings.nutrition.fatGoal" => {
            let (p, note) = clamp_int(raw, 0, 2000)?;
            ("Fat goal (g)", p, note)
        }
        "settings.aiProvider.provider" => {
            let s = raw.as_str()?.trim().to_ascii_lowercase();
            if !matches!(s.as_str(), "llamacpp" | "ollama" | "openai") {
                return None;
            }
            ("AI provider", json!(s), None)
        }
        "settings.aiProvider.model" => {
            let s = raw.as_str()?.trim();
            if s.is_empty() {
                return None;
            }
            let n = s.chars().count();
            if n > 128 {
                let t: String = s.chars().take(128).collect();
                (
                    "AI model",
                    json!(t),
                    Some(format!("truncated to 128 chars (requested {n} chars)")),
                )
            } else {
                ("AI model", json!(s.to_string()), None)
            }
        }
        // Unknown key: the backend never proposes settings it doesn't manage.
        _ => return None,
    };

    Some(json!({
        "key": key,
        "label": label,
        "current": current_value(settings, key),
        "proposed": proposed,
        "reason": note.unwrap_or_else(|| "suggested by Pythia".to_string()),
    }))
}

/// Reads the current value of a dotted settings key (e.g.
/// "settings.rangeDays" -> user_settings.settings.rangeDays).
fn current_value(settings: &Value, dotted: &str) -> Value {
    let path = dotted
        .strip_prefix("settings.")
        .unwrap_or(dotted)
        .split('.')
        .collect::<Vec<_>>()
        .join("/");
    settings.pointer(&format!("/{path}")).cloned().unwrap_or(Value::Null)
}

/// Coerces a model-supplied value to a finite f64 (accepts numbers and
/// numeric strings).
fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "on" => Some(true),
            "false" | "0" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Rounds and clamps a model-supplied number into [lo, hi] as an i64.
fn clamp_int(raw: &Value, lo: i64, hi: i64) -> Option<(Value, Option<String>)> {
    let v = as_num(raw)?;
    let c = (v.round() as i64).clamp(lo, hi);
    let note = if (c as f64 - v).abs() > 1e-9 {
        Some(format!("out of range — clamped to {c} (requested {v})"))
    } else {
        None
    };
    Some((json!(c), note))
}

/// Clamps a model-supplied number into [lo, hi] as a float.
fn clamp_float(raw: &Value, lo: f64, hi: f64) -> Option<(Value, Option<String>)> {
    let v = as_num(raw)?;
    let c = v.clamp(lo, hi);
    let note = if (c - v).abs() > 1e-9 {
        Some(format!("out of range — clamped to {c} (requested {v})"))
    } else {
        None
    };
    Some((json!(c), note))
}

/// Timeline range snaps to the allowed set {1, 7, 30, 365}.
fn nearest_range_days(v: f64) -> i64 {
    [1, 7, 30, 365].into_iter().min_by(|a, b| {
        let da = (v - *a as f64).abs();
        let db = (v - *b as f64).abs();
        da.partial_cmp(&db)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.cmp(a))
    }).unwrap()
}

/// Leniently extracts `{kind, amount}` from an LLM reply (tolerates code
/// fences and surrounding prose).
fn extract_nutrition(content: &str) -> ApiResult<(String, f64)> {
    let stripped = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let start = stripped.find('{').unwrap_or(0);
    let end = stripped.rfind('}').map(|i| i + 1).unwrap_or(stripped.len());
    let json_str = &stripped[start..end];

    let v: Value = serde_json::from_str(json_str)
        .map_err(|_| ApiError::BadRequest("could not parse AI nutrition response".to_string()))?;

    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if kind != "food" && kind != "water" {
        return Err(ApiError::BadRequest("AI returned an unsupported kind".to_string()));
    }

    let amount = v
        .get("amount")
        .and_then(|a| a.as_f64())
        .filter(|a| a.is_finite() && *a > 0.0)
        .ok_or_else(|| ApiError::BadRequest("AI returned no valid amount".to_string()))?;

    Ok((kind, amount))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(key: &str) -> Value {
        match key {
            "settings.rangeDays" => json!(7),
            "settings.series.heartRate" => json!(true),
            "settings.series.steps" => json!(true),
            "settings.series.calories" => json!(false),
            "settings.targets.steps" => json!(8000),
            "settings.targets.kcal" => json!(2500),
            "settings.targets.sleepH" => json!(8.0),
            "settings.targets.intensityHoursPerWeek" => json!(10.0),
            "settings.targets.weightKg" => json!(80.0),
            "settings.targets.bodyFatPct" => json!(20.0),
            "settings.nutrition.waterGoalMl" => json!(2500),
            "settings.nutrition.kcalGoal" => json!(2200),
            "settings.nutrition.proteinGoal" => json!(140),
            "settings.nutrition.carbGoal" => json!(220),
            "settings.nutrition.fatGoal" => json!(70),
            "settings.aiProvider.provider" => json!("ollama"),
            "settings.aiProvider.model" => json!("llama3"),
            "action.measurement.weight_kg" => json!(82.5),
            "action.measurement.body_fat_pct" => json!(18.0),
            "action.meal" => json!(650),
            other => panic!("no sample for {other}"),
        }
    }

    /// Sync settings-only stand-in for the async, DB-backed
    /// `validate_proposals` — the unit tests never touch a database.
    fn validate_settings(block: &str, settings: &Value) -> Vec<Value> {
        let Some(obj) = serde_json::from_str::<Value>(block).ok().filter(|v| v.is_object())
        else {
            return Vec::new();
        };
        obj.as_object()
            .expect("object in test")
            .iter()
            .filter_map(|(k, v)| build_settings_proposal(k, v, settings))
            .collect()
    }

    #[test]
    fn every_advertised_key_is_validatable() {
        let settings = json!({
            "rangeDays": 7,
            "series": { "heartRate": true, "steps": true, "calories": false },
            "targets": {
                "steps": 8000, "kcal": 2500, "sleepH": 8,
                "intensityHoursPerWeek": 10, "weightKg": 80, "bodyFatPct": 20
            },
            "nutrition": {
                "waterGoalMl": 2000,
                "kcalGoal": 2200,
                "proteinGoal": 130,
                "carbGoal": 200,
                "fatGoal": 70
            },
            "aiProvider": { "provider": "openai", "model": "gpt-4o-mini" }
        });
        for key in PYTHIA_ALLOWED_KEYS {
            if key.starts_with("action.") {
                let (label, action, metric, proposed, note) = action_proposal_fields(
                    key,
                    &sample(key),
                )
                .unwrap_or_else(|| panic!("whitelist key {key} was rejected"));
                assert!(!label.is_empty());
                assert!(action == "measurement" || action == "meal");
                assert!(!metric.is_empty());
                assert!(proposed.is_number());
                assert!(note.is_none());
            } else {
                let p = build_settings_proposal(key, &sample(key), &settings)
                    .unwrap_or_else(|| panic!("whitelist key {key} was rejected"));
                assert_eq!(p["key"], json!(key));
                assert!(p["label"].is_string());
                assert!(p["reason"].is_string());
            }
        }
    }

    #[test]
    fn system_prompt_advertises_the_whitelist() {
        let prompt = system_prompt(&json!({ "rangeDays": 7 }));
        for key in PYTHIA_ALLOWED_KEYS {
            assert!(prompt.contains(key), "prompt missing {key}");
        }
        assert!(prompt.contains("PYTHIA"));
    }

    #[test]
    fn unknown_keys_are_dropped() {
        let settings = json!({ "rangeDays": 7 });
        let ps = validate_settings(
            r#"{"settings.rangeDays": 30, "settings.bogus": 1, "nope": true, "settings.targets.heartsRate": 9}"#,
            &settings,
        );
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0]["key"], json!("settings.rangeDays"));
        assert_eq!(ps[0]["proposed"], json!(30));
        assert_eq!(ps[0]["current"], json!(7));
    }

    #[test]
    fn out_of_range_is_clamped_and_noted() {
        let settings = json!({
            "nutrition": { "waterGoalMl": 2000 },
            "targets": { "steps": 8000, "sleepH": 8 },
        });
        let ps = validate_settings(
            r#"{"settings.nutrition.waterGoalMl": 50000, "settings.targets.steps": 0, "settings.targets.sleepH": 40}"#,
            &settings,
        );
        assert_eq!(ps.len(), 3);
        let water = ps.iter().find(|p| p["key"] == "settings.nutrition.waterGoalMl").unwrap();
        assert_eq!(water["proposed"], json!(10_000));
        assert!(water["reason"].as_str().unwrap().contains("clamped"));
        let steps = ps.iter().find(|p| p["key"] == "settings.targets.steps").unwrap();
        assert_eq!(steps["proposed"], json!(1));
        assert!(steps["reason"].as_str().unwrap().contains("clamped"));
        let sleep = ps.iter().find(|p| p["key"] == "settings.targets.sleepH").unwrap();
        assert_eq!(sleep["proposed"], json!(24.0));
    }

    #[test]
    fn new_target_keys_are_clamped() {
        let settings = json!({});
        let ps = validate_settings(
            r#"{"settings.targets.intensityHoursPerWeek": 200, "settings.targets.weightKg": 10, "settings.targets.bodyFatPct": 70}"#,
            &settings,
        );
        assert_eq!(ps.len(), 3);
        let hours = ps.iter().find(|p| p["key"] == "settings.targets.intensityHoursPerWeek").unwrap();
        assert_eq!(hours["proposed"], json!(168.0));
        let weight = ps.iter().find(|p| p["key"] == "settings.targets.weightKg").unwrap();
        assert_eq!(weight["proposed"], json!(20.0));
        let fat = ps.iter().find(|p| p["key"] == "settings.targets.bodyFatPct").unwrap();
        assert_eq!(fat["proposed"], json!(60.0));
    }

    #[test]
    fn action_proposals_carry_action_metric_value() {
        let (label, action, metric, proposed, _) =
            action_proposal_fields("action.measurement.weight_kg", &json!(82.5)).unwrap();
        assert_eq!(label, "Weight (kg)");
        assert_eq!(action, "measurement");
        assert_eq!(metric, "weight_kg");
        assert_eq!(proposed, json!(82.5));

        let (_, action, metric, proposed, _) =
            action_proposal_fields("action.measurement.body_fat_pct", &json!(18)).unwrap();
        assert_eq!(action, "measurement");
        assert_eq!(metric, "body_fat_pct");
        assert_eq!(proposed, json!(18.0));

        let (_, action, metric, proposed, _) =
            action_proposal_fields("action.meal", &json!(650)).unwrap();
        assert_eq!(action, "meal");
        assert_eq!(metric, "meal");
        assert_eq!(proposed, json!(650.0));
    }

    #[test]
    fn action_values_are_clamped() {
        let (_, _, _, proposed, note) =
            action_proposal_fields("action.measurement.weight_kg", &json!(500.0)).unwrap();
        assert_eq!(proposed, json!(400.0));
        assert!(note.unwrap().contains("clamped"));

        let (_, _, _, proposed, _) =
            action_proposal_fields("action.measurement.body_fat_pct", &json!(1.0)).unwrap();
        assert_eq!(proposed, json!(3.0));

        let (_, _, _, proposed, _) = action_proposal_fields("action.meal", &json!(-5.0)).unwrap();
        assert_eq!(proposed, json!(0.0));
    }

    #[test]
    fn range_days_snaps_to_allowed_set() {
        let settings = json!({ "rangeDays": 30 });
        for (in_v, out_v) in [
            (2.0f64, 1i64),
            (36.0, 30),
            (300.0, 365),
            (-5.0, 1),
            (365.0, 365),
        ] {
            let ps =
                validate_settings(&format!(r#"{{"settings.rangeDays": {in_v}}}"#), &settings);
            assert_eq!(ps.len(), 1, "for input {in_v}");
            assert_eq!(ps[0]["proposed"], json!(out_v), "for input {in_v}");
        }
    }

    #[test]
    fn model_truncation_is_noted() {
        let settings = json!({ "aiProvider": { "provider": "openai", "model": "short" } });
        let long: String = "m".repeat(200);
        let ps = validate_settings(
            &format!(r#"{{"settings.aiProvider.model": "{long}"}}"#),
            &settings,
        );
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0]["proposed"].as_str().unwrap().len(), 128);
        assert!(ps[0]["reason"].as_str().unwrap().contains("truncated"));
        assert_eq!(ps[0]["current"], json!("short"));
    }

    #[test]
    fn block_extraction_tolerates_code_fences() {
        let (reply, block) = extract_pythia_block(
            "Sure — 30 days gives you a fuller picture.\n```json\n[PYTHIA]{\"settings.rangeDays\": 30}[/PYTHIA]\n```\nLet me know!",
        );
        assert_eq!(block.as_deref(), Some(r#"{"settings.rangeDays": 30}"#));
        assert!(!reply.contains("[PYTHIA]"));
        assert!(!reply.contains("```"));
        assert!(reply.contains("Sure"));
    }

    #[test]
    fn no_block_means_plain_reply() {
        let (reply, block) = extract_pythia_block("You sleep 7h on average, a touch low.");
        assert!(block.is_none());
        assert_eq!(reply, "You sleep 7h on average, a touch low.");
    }

    #[test]
    fn malformed_block_yields_no_proposals_but_is_stripped() {
        let (reply, block) = extract_pythia_block("Done. [PYTHIA]{not json}[/PYTHIA] Bye.");
        assert_eq!(block.as_deref(), Some("{not json}"));
        assert_eq!(validate_settings(block.as_deref().unwrap(), &json!({})).len(), 0);
        assert!(!reply.contains("[PYTHIA]"));
    }
}

/// Wire-level tests: real TCP round trips against one-shot mock provider
/// servers, verifying URL routing, auth, request shapes and content
/// extraction for each provider family.
#[cfg(test)]
mod wire_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::oneshot;

    fn http_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// Serves exactly one HTTP request on an ephemeral loopback port,
    /// capturing the raw request for later assertions.
    async fn serve_once(
        status: &str,
        body: &str,
    ) -> (String, oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = oneshot::channel::<String>();
        let response = http_response(status, body);
        tokio::spawn(async move {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let mut reader = BufReader::new(socket);
            let mut lines: Vec<String> = Vec::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(_) => return,
                }
                let t = line.trim_end_matches(['\r', '\n']);
                if t.is_empty() {
                    break;
                }
                lines.push(t.to_string());
            }
            let len = lines.iter().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim().eq_ignore_ascii_case("content-length"))
                    .then(|| v.trim().parse::<usize>().ok())
                    .flatten()
            }).unwrap_or(0);
            let mut body = vec![0u8; len];
            if reader.read_exact(&mut body).await.is_err() {
                return;
            }
            let request = format!(
                "{}\n{}",
                lines.join("\n"),
                String::from_utf8_lossy(&body)
            );
            let socket = reader.into_inner();
            let mut socket = socket;
            if socket.write_all(response.as_bytes()).await.is_err() {
                return;
            }
            let _ = socket.shutdown().await;
            let _ = tx.send(request);
        });
        (addr, rx)
    }

    fn provider(base_url: String, provider: &str, api_key: &str) -> AiProvider {
        AiProvider {
            provider: provider.to_string(),
            base_url,
            model: "model-1".to_string(),
            api_key: api_key.to_string(),
        }
    }

    #[tokio::test]
    async fn openai_compatible_round_trip() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"pong"}}]}"#;
        let (addr, rx) = serve_once("200 OK", body).await;
        let p = provider(addr, "openai", "sekrit");
        let out = provider_chat(&p, &[json!({"role": "user", "content": "hi"})], 0.0)
            .await
            .unwrap();
        assert_eq!(out, "pong");
        let req = rx.await.unwrap().to_ascii_lowercase();
        assert!(req.starts_with("post /chat/completions http/1.1"), "{req}");
        assert!(req.contains(r#""model":"model-1""#));
        assert!(req.contains("authorization: bearer sekrit"));
    }

    #[tokio::test]
    async fn llamacpp_routes_like_openai() {
        let body = r#"{"choices":[{"message":{"content":"pong"}}]}"#;
        let (addr, rx) = serve_once("200 OK", body).await;
        let p = provider(format!("  {addr}/  "), "llamacpp", "");
        let out = provider_chat(&p, &[json!({"role": "user", "content": "hi"})], 0.0)
            .await
            .unwrap();
        assert_eq!(out, "pong");
        let req = rx.await.unwrap();
        assert!(req.starts_with("POST /chat/completions HTTP/1.1"), "{req}");
        assert!(!req.to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn ollama_native_round_trip() {
        let body = r#"{"message":{"role":"assistant","content":"ping"}}"#;
        let (addr, rx) = serve_once("200 OK", body).await;
        let p = provider(addr, "ollama", "");
        let out = provider_chat(&p, &[json!({"role": "user", "content": "hi"})], 0.2)
            .await
            .unwrap();
        assert_eq!(out, "ping");
        let req = rx.await.unwrap();
        assert!(req.starts_with("POST /api/chat HTTP/1.1"), "{req}");
        assert!(req.contains(r#""stream":false"#));
    }

    #[tokio::test]
    async fn unreachable_reports_provider_name() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        drop(listener); // port is now closed
        let p = provider(addr, "llamacpp", "");
        let err = provider_chat(&p, &[], 0.0)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("llama.cpp unreachable"), "{err}");
    }

    #[tokio::test]
    async fn http_error_includes_status() {
        let (addr, _rx) = serve_once("500 Internal Server Error", "model not found").await;
        let p = provider(addr, "openai", "");
        let err = provider_chat(&p, &[json!({"role": "user", "content": "hi"})], 0.0)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("openai returned HTTP 500"), "{err}");
        assert!(err.contains("model not found"));
    }
}
