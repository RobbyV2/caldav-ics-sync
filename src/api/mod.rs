use axum::Router;
use std::sync::{Arc, Mutex};

pub mod destinations;
pub mod health;
pub mod openapi;
pub mod reverse_sync;
pub mod sources;
pub mod sync;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub start_time: std::time::Instant,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(sources::routes())
        .merge(destinations::routes())
        .merge(health::routes())
        .merge(openapi::routes())
}
