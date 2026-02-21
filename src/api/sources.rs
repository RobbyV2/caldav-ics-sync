use crate::api::AppState;
use crate::auto_sync::{self, AutoSyncKey};
use crate::db;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SourceResponse {
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<db::Source>,
}

#[derive(Serialize, ToSchema)]
pub struct SourceListResponse {
    sources: Vec<db::Source>,
}

#[derive(Serialize, ToSchema)]
pub struct SyncResult {
    status: String,
    message: String,
    events: usize,
    calendars: usize,
}

#[utoipa::path(get, path = "/api/sources", responses((status = 200, body = SourceListResponse)))]
async fn list_sources(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::list_sources(&db) {
        Ok(sources) => (StatusCode::OK, Json(SourceListResponse { sources })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SourceResponse {
                status: "error".into(),
                message: e.to_string(),
                source: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(post, path = "/api/sources", request_body = db::CreateSource, responses((status = 201, body = SourceResponse)))]
async fn create_source(
    State(state): State<AppState>,
    Json(body): Json<db::CreateSource>,
) -> impl IntoResponse {
    let (id, source) = {
        let db = state.db.lock().unwrap();
        match db::create_source(&db, &body) {
            Ok(id) => {
                let source = db::get_source(&db, id).ok().flatten();
                (id, source)
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SourceResponse {
                        status: "error".into(),
                        message: e.to_string(),
                        source: None,
                    }),
                )
                    .into_response();
            }
        }
    };

    if let Some(ref s) = source {
        auto_sync::register_source(&state.sync_tasks, &state, s);
    }

    (
        StatusCode::CREATED,
        Json(SourceResponse {
            status: "success".into(),
            message: format!("Source created with id {}", id),
            source,
        }),
    )
        .into_response()
}

#[utoipa::path(put, path = "/api/sources/{id}", request_body = db::UpdateSource, responses((status = 200, body = SourceResponse)))]
async fn update_source(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<db::UpdateSource>,
) -> impl IntoResponse {
    let source = {
        let db = state.db.lock().unwrap();
        match db::update_source(&db, id, &body) {
            Ok(true) => db::get_source(&db, id).ok().flatten(),
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(SourceResponse {
                        status: "error".into(),
                        message: "Source not found".into(),
                        source: None,
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SourceResponse {
                        status: "error".into(),
                        message: e.to_string(),
                        source: None,
                    }),
                )
                    .into_response();
            }
        }
    };

    if let Some(ref s) = source {
        auto_sync::register_source(&state.sync_tasks, &state, s);
    }

    (
        StatusCode::OK,
        Json(SourceResponse {
            status: "success".into(),
            message: "Source updated".into(),
            source,
        }),
    )
        .into_response()
}

#[utoipa::path(delete, path = "/api/sources/{id}", responses((status = 200, body = SourceResponse)))]
async fn delete_source_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = {
        let db = state.db.lock().unwrap();
        db::delete_source(&db, id)
    };

    match result {
        Ok(true) => {
            auto_sync::cancel(&state.sync_tasks, &AutoSyncKey::Source(id));
            (
                StatusCode::OK,
                Json(SourceResponse {
                    status: "success".into(),
                    message: "Source deleted".into(),
                    source: None,
                }),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(SourceResponse {
                status: "error".into(),
                message: "Source not found".into(),
                source: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SourceResponse {
                status: "error".into(),
                message: e.to_string(),
                source: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(post, path = "/api/sources/{id}/sync", responses((status = 200, body = SyncResult)))]
async fn sync_source(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    let (caldav_url, username, password) = {
        let db = state.db.lock().unwrap();
        match db::get_source(&db, id) {
            Ok(Some(s)) => (s.caldav_url, s.username, s.password),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(SyncResult {
                        status: "error".into(),
                        message: "Source not found".into(),
                        events: 0,
                        calendars: 0,
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(SyncResult {
                        status: "error".into(),
                        message: e.to_string(),
                        events: 0,
                        calendars: 0,
                    }),
                )
                    .into_response();
            }
        }
    };

    match crate::api::sync::run_sync(&caldav_url, &username, &password).await {
        Ok((events, calendars, ics_data)) => {
            let db = state.db.lock().unwrap();
            if let Err(e) = db::save_ics_data(&db, id, &ics_data) {
                tracing::error!("Failed to save ICS data: {}", e);
            }
            if let Err(e) = db::update_last_synced(&db, id) {
                tracing::error!("Failed to update last_synced: {}", e);
            }
            let _ = db::update_sync_status(&db, id, "ok", None);
            (
                StatusCode::OK,
                Json(SyncResult {
                    status: "success".into(),
                    message: format!(
                        "Synchronized {} events from {} calendars",
                        events, calendars
                    ),
                    events,
                    calendars,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Sync error for source {}: {}", id, e);
            let db = state.db.lock().unwrap();
            let _ = db::update_sync_status(&db, id, "error", Some(&e.to_string()));
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SyncResult {
                    status: "error".into(),
                    message: e.to_string(),
                    events: 0,
                    calendars: 0,
                }),
            )
                .into_response()
        }
    }
}

#[utoipa::path(get, path = "/api/sources/{id}/status", responses((status = 200, body = SourceResponse)))]
async fn source_status(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::get_source(&db, id) {
        Ok(Some(s)) => (
            StatusCode::OK,
            Json(SourceResponse {
                status: "success".into(),
                message: format!(
                    "Last synced: {}",
                    s.last_synced.as_deref().unwrap_or("never")
                ),
                source: Some(s),
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(SourceResponse {
                status: "error".into(),
                message: "Source not found".into(),
                source: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SourceResponse {
                status: "error".into(),
                message: e.to_string(),
                source: None,
            }),
        )
            .into_response(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sources", get(list_sources).post(create_source))
        .route(
            "/sources/{id}",
            put(update_source).delete(delete_source_handler),
        )
        .route("/sources/{id}/sync", post(sync_source))
        .route("/sources/{id}/status", get(source_status))
}
