//! AI-assisted data entry. The user describes a meal or a drink in plain
//! language; the configured provider (local or remote, OpenAI-compatible
//! chat completions) returns a structured nutrition estimate, which the web
//! UI then logs via `/api/v1/nutrition`.
//!
//! Provider config lives in the per-user settings JSONB under `aiProvider`:
//!   { "baseUrl": "http://localhost:11434/v1", "model": "llama3", "apiKey": "" }
//! `baseUrl` is the chat-completions root (no trailing /chat/completions).
//! Local (Ollama/LM Studio/llama.cpp) and remote (OpenAI/etc.) are the same
//! protocol — only the URL differs. Everything stays local if you point it at
//! a local provider.

use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

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
struct AiProvider {
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: String,
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

    let body = json!({
        "model": provider.model,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.0,
        "stream": false,
    });

    let url = format!("{}/chat/completions", provider.base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req_builder = client.post(&url).json(&body);
    if !provider.api_key.is_empty() {
        req_builder = req_builder.bearer_auth(&provider.api_key);
    }
    let resp = req_builder
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("AI provider unreachable: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        return Err(ApiError::BadRequest(format!(
            "AI provider returned HTTP {status}: {}",
            detail.chars().take(200).collect::<String>()
        )));
    }

    let parsed: Value = resp
        .json()
        .await
        .map_err(|e| ApiError::BadRequest(format!("AI provider returned invalid JSON: {e}")))?;

    let content = parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("AI response missing content".to_string()))?;

    let (kind, amount) = extract_nutrition(content)?;

    Ok(Json(json!({
        "kind": kind,
        "amount": amount,
        "note": text,
    })))
}

async fn load_provider(pool: &PgPool, user_id: uuid::Uuid) -> ApiResult<AiProvider> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT settings FROM user_settings WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

    let Some((settings,)) = row else {
        return Err(ApiError::BadRequest(
            "no AI provider configured — set aiProvider in settings".to_string(),
        ));
    };

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
