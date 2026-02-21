use axum::Router;
use std::sync::{Arc, Mutex};

use crate::auto_sync::AutoSyncRegistry;

pub mod destinations;
pub mod health;
pub mod openapi;
pub mod reverse_sync;
pub mod source_paths;
pub mod sources;
pub mod sync;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub start_time: std::time::Instant,
    pub sync_tasks: AutoSyncRegistry,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(sources::routes())
        .merge(source_paths::routes())
        .merge(destinations::routes())
        .merge(health::routes())
        .merge(openapi::routes())
}
