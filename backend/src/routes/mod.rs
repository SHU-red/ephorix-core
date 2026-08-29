pub mod actions;
pub mod agoge_sessions;
pub mod agoge_types;
pub mod ai;
pub mod brand;
pub mod events;
pub mod health;
pub mod import;
pub mod ingest;
pub mod measurements;
pub mod metrics;
pub mod nutrition;
pub mod settings;
pub mod timeline;

use axum::{
    http::{HeaderName, Method},
    middleware,
    routing::{get, patch, post, put},
    Router,
};
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};

use crate::auth;

/// Public liveness probe (no auth).
async fn healthz() -> &'static str {
    "ok"
}

pub fn app(pool: PgPool, cors_origins: Vec<String>) -> Router {
    let cors = if cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_headers([HeaderName::from_static(auth::AUTH_HEADER), "content-type".parse().unwrap()])
            .allow_methods(Any)
    } else {
        let origins: Vec<axum::http::HeaderValue> = cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_headers([HeaderName::from_static(auth::AUTH_HEADER), "content-type".parse().unwrap()])
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
    };

    // The auth middleware guards the whole /api/v1 subtree. /healthz stays
    // public for orchestration probes (build it on a separate router so the
    // route_layer cannot touch it).
    let api = Router::new()
        .route(
            "/api/v1/health/batch",
            post(health::ingest_batch),
        )
        .route("/api/v1/health/days", post(health::ingest_days))
        .route("/api/v1/events/marker", post(events::ingest_marker))
        .route("/api/v1/events/markers", get(events::list_markers))
        .route(
            "/api/v1/agoge-types",
            get(agoge_types::list).post(agoge_types::create),
        )
        .route(
            "/api/v1/agoge-types/reorder",
            post(agoge_types::reorder),
        )
        .route(
            "/api/v1/agoge-types/{id}",
            put(agoge_types::update).delete(agoge_types::delete),
        )
        .route(
            "/api/v1/agoge-sessions",
            get(agoge_sessions::list).post(agoge_sessions::create),
        )
        .route(
            "/api/v1/agoge-sessions/{id}",
            patch(agoge_sessions::update).delete(agoge_sessions::delete),
        )
        .route(
            "/api/v1/agoge-sessions/{id}/stats",
            get(agoge_sessions::stats),
        )
        .route(
            "/api/v1/agoge-sessions/{id}/exercises",
            get(agoge_sessions::exercises).post(agoge_sessions::add_exercise),
        )
        .route(
            "/api/v1/agoge-sessions/{id}/exercises/{eid}",
            patch(agoge_sessions::update_exercise).delete(agoge_sessions::delete_exercise),
        )
        .route("/api/v1/timeline", get(timeline::get_timeline))
        .route(
            "/api/v1/settings",
            get(settings::get_settings).put(settings::put_settings),
        )
        .route("/api/v1/ingest", post(ingest::ingest))
        .route(
            "/api/v1/measurements",
            get(ingest::list_measurements).post(measurements::add_measurement),
        )
        .route("/api/v1/import", post(import::import))
        .route(
            "/api/v1/nutrition",
            get(nutrition::list_nutrition).post(nutrition::add_nutrition),
        )
        .route("/api/v1/nutrition/daily", get(nutrition::daily))
        .route("/api/v1/actions", get(actions::list_actions))
        .route("/api/v1/actions/{id}/revert", post(actions::revert_action))
        .route(
            "/api/v1/metrics/body-battery",
            get(metrics::body_battery),
        )
        .route(
            "/api/v1/metrics/body-battery-series",
            get(metrics::body_battery_series),
        )
        .route(
            "/api/v1/metrics/baselines",
            get(metrics::baselines),
        )
        .route("/api/v1/metrics/readiness", get(metrics::readiness))
        .route("/api/v1/metrics/workouts", get(metrics::workouts))
        .route(
            "/api/v1/metrics/workouts/{id}/accept",
            post(metrics::accept_detection),
        )
        .route(
            "/api/v1/metrics/workouts/{id}/reject",
            post(metrics::reject_detection),
        )
        .route("/api/v1/ai/parse", post(ai::parse))
        .route("/api/v1/ai/chat", post(ai::chat))
        .route("/api/v1/ai/test", post(ai::test_provider))
        .route_layer(middleware::from_fn_with_state(pool.clone(), auth::require_auth));

    Router::new()
        .route("/healthz", get(healthz))
        // Public brand assets (no auth): the app icon, hotlinkable from the
        // web app and external surfaces.
        .route("/api/brand/icon.svg", get(brand::icon_svg))
        .merge(api)
        .with_state(pool)
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http())
}
