use std::net::SocketAddr;

use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

mod auth;
mod config;
mod error;
mod models;
mod routes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let cfg = config::Config::from_env();

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;
    tracing::info!("connected to database");

    // Run embedded migrations on boot (idempotent).
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("migrations applied");

    let app = routes::app(pool, cfg.cors_origins);

    let addr: SocketAddr = cfg.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("EphoriX API listening on http://{addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
