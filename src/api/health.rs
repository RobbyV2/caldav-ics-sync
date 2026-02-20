use crate::api::AppState;
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Serialize, ToSchema)]
pub struct DetailedHealthResponse {
    pub status: String,
    pub uptime_seconds: u64,
    pub source_count: usize,
    pub db_ok: bool,
}

#[utoipa::path(get, path = "/api/health", responses((status = 200, body = HealthResponse)))]
pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok".into(),
        }),
    )
}

#[utoipa::path(get, path = "/api/health/detailed", responses((status = 200, body = DetailedHealthResponse)))]
pub async fn health_detailed(State(state): State<AppState>) -> impl IntoResponse {
    let (source_count, db_ok) = {
        let db = state.db.lock().unwrap();
        match crate::db::list_sources(&db) {
            Ok(sources) => (sources.len(), true),
            Err(_) => (0, false),
        }
    };
    let uptime = state.start_time.elapsed().as_secs();
    (
        StatusCode::OK,
        Json(DetailedHealthResponse {
            status: if db_ok { "ok" } else { "degraded" }.into(),
            uptime_seconds: uptime,
            source_count,
            db_ok,
        }),
    )
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/health/detailed", get(health_detailed))
}
