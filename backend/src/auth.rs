use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// Header carrying the POC auth token. Multi-user capable: the token maps to a
/// `users.id`, and every handler authorizes against that user id.
pub const AUTH_HEADER: &str = "x-ephorix-token";

/// Authenticated user id, injected into request extensions by `require_auth`.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser(pub Uuid);

/// Dummy-token middleware. Stateless: one indexed lookup per request against
/// `users.token`. Swap for real auth (JWT/OIDC) behind this same extension.
pub async fn require_auth(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    mut req: Request,
    next: Next,
) -> ApiResult<Response> {
    let token = headers
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::Unauthorized("missing X-EphoriX-Token header".to_string()))?;

    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE token = $1")
        .bind(token)
        .fetch_optional(&pool)
        .await?;

    let Some((user_id,)) = row else {
        return Err(ApiError::Unauthorized("unknown token".to_string()));
    };

    req.extensions_mut().insert(AuthUser(user_id));
    Ok(next.run(req).await)
}
