use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// Shared row models. All user-scoped tables are joined against
/// `AuthUser` — never trust a client-supplied user id.

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgogeType {
    pub id: Uuid,
    pub name: String,
    pub color_code: String,
    pub icon: String,
    pub category: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgogeSession {
    pub id: Uuid,
    pub user_id: Uuid,
    /// NULL == "Undefined Agoge"
    pub type_id: Option<Uuid>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    /// 'active' (open) | 'closed'
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgogeMarker {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_id: Option<Uuid>,
    pub kind: String,
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    pub meta: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}
