use std::env;

/// Runtime configuration. All values have POC-friendly defaults so the API
/// boots against the docker-compose stack with zero env fiddling.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub cors_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://ephorix:ephorix@localhost:5432/ephorix".to_string()
            });
        let bind = env::var("EPHORIX_BIND").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let cors_origins = env::var("EPHORIX_CORS_ORIGINS")
            .unwrap_or_else(|_| {
                "http://localhost:8080,http://127.0.0.1:8080,http://localhost:3000".to_string()
            })
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Config {
            database_url,
            bind,
            cors_origins,
        }
    }
}
