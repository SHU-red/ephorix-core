use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Uniform API error envelope: `{"error": {"code", "message"}}`.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Serialize(#[from] serde_json::Error),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, "unauthorized", m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m),
            ApiError::Db(e) => {
                tracing::error!("database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "database error".to_string(),
                )
            }
            ApiError::Serialize(e) => {
                tracing::error!("serialization error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "response serialization failed".to_string(),
                )
            }
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}
