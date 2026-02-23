use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::AppState;
use crate::auto_sync::{self, AutoSyncKey};
use crate::db;

#[derive(Serialize, ToSchema)]
pub struct DestinationResponse {
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination: Option<db::Destination>,
}

#[derive(Serialize, ToSchema)]
pub struct DestinationListResponse {
    destinations: Vec<db::Destination>,
}

#[derive(Serialize, ToSchema)]
pub struct ReverseSyncResult {
    status: String,
    message: String,
    uploaded: usize,
    skipped: usize,
    deleted: usize,
    total: usize,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/destinations", get(list_destinations))
        .route("/destinations", post(create_destination))
        .route("/destinations/check-overlap", get(check_overlap))
        .route("/destinations/{id}", put(update_destination))
        .route("/destinations/{id}", delete(delete_destination))
        .route("/destinations/{id}/sync", post(sync_destination))
}

#[utoipa::path(get, path = "/api/destinations", responses((status = 200, body = DestinationListResponse)))]
pub async fn list_destinations(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::list_destinations(&db) {
        Ok(destinations) => (
            StatusCode::OK,
            Json(DestinationListResponse { destinations }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DestinationResponse {
                status: "error".into(),
                message: e.to_string(),
                destination: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(post, path = "/api/destinations", request_body = db::CreateDestination, responses((status = 201, body = DestinationResponse)))]
pub async fn create_destination(
    State(state): State<AppState>,
    Json(body): Json<db::CreateDestination>,
) -> impl IntoResponse {
    let (id, dest) = {
        let db = state.db.lock().unwrap();
        match db::create_destination(&db, &body) {
            Ok(id) => {
                let dest = db::get_destination(&db, id).ok().flatten();
                (id, dest)
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(DestinationResponse {
                        status: "error".into(),
                        message: e.to_string(),
                        destination: None,
                    }),
                )
                    .into_response();
            }
        }
    };

    if let Some(ref d) = dest {
        auto_sync::register_destination(&state.sync_tasks, &state, d);
    }

    (
        StatusCode::CREATED,
        Json(DestinationResponse {
            status: "success".into(),
            message: format!("Destination created with id {}", id),
            destination: dest,
        }),
    )
        .into_response()
}

#[utoipa::path(put, path = "/api/destinations/{id}", request_body = db::UpdateDestination, responses((status = 200, body = DestinationResponse)))]
pub async fn update_destination(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<db::UpdateDestination>,
) -> impl IntoResponse {
    let dest = {
        let db = state.db.lock().unwrap();
        match db::update_destination(&db, id, &body) {
            Ok(true) => db::get_destination(&db, id).ok().flatten(),
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(DestinationResponse {
                        status: "error".into(),
                        message: "Destination not found".into(),
                        destination: None,
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(DestinationResponse {
                        status: "error".into(),
                        message: e.to_string(),
                        destination: None,
                    }),
                )
                    .into_response();
            }
        }
    };

    if let Some(ref d) = dest {
        auto_sync::register_destination(&state.sync_tasks, &state, d);
    }

    (
        StatusCode::OK,
        Json(DestinationResponse {
            status: "success".into(),
            message: "Destination updated".into(),
            destination: dest,
        }),
    )
        .into_response()
}

#[utoipa::path(delete, path = "/api/destinations/{id}", responses((status = 200, body = DestinationResponse)))]
pub async fn delete_destination(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = {
        let db = state.db.lock().unwrap();
        db::delete_destination(&db, id)
    };

    match result {
        Ok(true) => {
            auto_sync::cancel(&state.sync_tasks, &AutoSyncKey::Destination(id));
            (
                StatusCode::OK,
                Json(DestinationResponse {
                    status: "success".into(),
                    message: "Destination deleted".into(),
                    destination: None,
                }),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(DestinationResponse {
                status: "error".into(),
                message: "Destination not found".into(),
                destination: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DestinationResponse {
                status: "error".into(),
                message: e.to_string(),
                destination: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(post, path = "/api/destinations/{id}/sync", responses((status = 200, body = ReverseSyncResult)))]
pub async fn sync_destination(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let (ics_url, caldav_url, calendar_name, username, password, sync_all, keep_local) = {
        let db = state.db.lock().unwrap();
        match db::get_destination(&db, id) {
            Ok(Some(d)) => (
                d.ics_url,
                d.caldav_url,
                d.calendar_name,
                d.username,
                d.password,
                d.sync_all,
                d.keep_local,
            ),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ReverseSyncResult {
                        status: "error".into(),
                        message: "Destination not found".into(),
                        uploaded: 0,
                        skipped: 0,
                        deleted: 0,
                        total: 0,
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ReverseSyncResult {
                        status: "error".into(),
                        message: e.to_string(),
                        uploaded: 0,
                        skipped: 0,
                        deleted: 0,
                        total: 0,
                    }),
                )
                    .into_response();
            }
        }
    };

    match crate::api::reverse_sync::run_reverse_sync(
        &ics_url,
        &caldav_url,
        &calendar_name,
        &username,
        &password,
        sync_all,
        keep_local,
    )
    .await
    {
        Ok(stats) => {
            let db = state.db.lock().unwrap();
            let _ = db::update_destination_sync_status(&db, id, "ok", None);
            (
                StatusCode::OK,
                Json(ReverseSyncResult {
                    status: "success".into(),
                    message: format!(
                        "Uploaded {} of {} events ({} unchanged, {} deleted)",
                        stats.uploaded, stats.total, stats.skipped, stats.deleted
                    ),
                    uploaded: stats.uploaded,
                    skipped: stats.skipped,
                    deleted: stats.deleted,
                    total: stats.total,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Reverse sync error for destination {}: {}", id, e);
            let db = state.db.lock().unwrap();
            let _ = db::update_destination_sync_status(&db, id, "error", Some(&e.to_string()));
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReverseSyncResult {
                    status: "error".into(),
                    message: e.to_string(),
                    uploaded: 0,
                    skipped: 0,
                    deleted: 0,
                    total: 0,
                }),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct OverlapQuery {
    caldav_url: String,
    calendar_name: String,
    exclude_id: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct OverlapEntry {
    id: i64,
    name: String,
    ics_url: String,
    sync_all: bool,
    keep_local: bool,
}

#[derive(Serialize, ToSchema)]
pub struct OverlapResponse {
    overlapping: Vec<OverlapEntry>,
}

#[utoipa::path(
    get,
    path = "/api/destinations/check-overlap",
    params(
        ("caldav_url" = String, Query, description = "CalDAV URL to check"),
        ("calendar_name" = String, Query, description = "Calendar name to check"),
        ("exclude_id" = Option<i64>, Query, description = "Destination ID to exclude"),
    ),
    responses((status = 200, body = OverlapResponse))
)]
pub async fn check_overlap(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<OverlapQuery>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::find_overlapping_destinations(&db, &q.caldav_url, &q.calendar_name, q.exclude_id) {
        Ok(dests) => (
            StatusCode::OK,
            Json(OverlapResponse {
                overlapping: dests
                    .into_iter()
                    .map(|d| OverlapEntry {
                        id: d.id,
                        name: d.name,
                        ics_url: d.ics_url,
                        sync_all: d.sync_all,
                        keep_local: d.keep_local,
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to check destination overlap: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(OverlapResponse {
                    overlapping: vec![],
                }),
            )
                .into_response()
        }
    }
}
