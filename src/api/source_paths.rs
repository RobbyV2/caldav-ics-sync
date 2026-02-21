use crate::api::AppState;
use crate::db;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SourcePathResponse {
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<db::SourcePath>,
}

#[derive(Serialize, ToSchema)]
pub struct SourcePathListResponse {
    paths: Vec<db::SourcePath>,
}

#[utoipa::path(
    get,
    path = "/api/sources/{source_id}/paths",
    params(("source_id" = i64, Path, description = "Source ID")),
    responses((status = 200, body = SourcePathListResponse))
)]
pub async fn list_source_paths(
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::list_source_paths(&db, source_id) {
        Ok(paths) => (StatusCode::OK, Json(SourcePathListResponse { paths })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SourcePathResponse {
                status: "error".into(),
                message: e.to_string(),
                path: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/sources/{source_id}/paths",
    params(("source_id" = i64, Path, description = "Source ID")),
    request_body = db::CreateSourcePath,
    responses((status = 201, body = SourcePathResponse))
)]
pub async fn create_source_path(
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
    Json(body): Json<db::CreateSourcePath>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::create_source_path(&db, source_id, &body) {
        Ok(id) => {
            let sp = db::get_source_path(&db, id).ok().flatten();
            (
                StatusCode::CREATED,
                Json(SourcePathResponse {
                    status: "success".into(),
                    message: format!("Path created with id {}", id),
                    path: sp,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(SourcePathResponse {
                status: "error".into(),
                message: e.to_string(),
                path: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/api/sources/{source_id}/paths/{path_id}",
    params(
        ("source_id" = i64, Path, description = "Source ID"),
        ("path_id" = i64, Path, description = "Path ID"),
    ),
    request_body = db::UpdateSourcePath,
    responses((status = 200, body = SourcePathResponse))
)]
pub async fn update_source_path(
    State(state): State<AppState>,
    Path((source_id, path_id)): Path<(i64, i64)>,
    Json(body): Json<db::UpdateSourcePath>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::get_source_path(&db, path_id) {
        Ok(Some(sp)) if sp.source_id != source_id => {
            return (
                StatusCode::NOT_FOUND,
                Json(SourcePathResponse {
                    status: "error".into(),
                    message: "Path not found".into(),
                    path: None,
                }),
            )
                .into_response();
        }
        _ => {}
    }
    match db::update_source_path(&db, path_id, &body) {
        Ok(true) => {
            let sp = db::get_source_path(&db, path_id).ok().flatten();
            (
                StatusCode::OK,
                Json(SourcePathResponse {
                    status: "success".into(),
                    message: "Path updated".into(),
                    path: sp,
                }),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(SourcePathResponse {
                status: "error".into(),
                message: "Path not found".into(),
                path: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(SourcePathResponse {
                status: "error".into(),
                message: e.to_string(),
                path: None,
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/api/sources/{source_id}/paths/{path_id}",
    params(
        ("source_id" = i64, Path, description = "Source ID"),
        ("path_id" = i64, Path, description = "Path ID"),
    ),
    responses((status = 200, body = SourcePathResponse))
)]
pub async fn delete_source_path(
    State(state): State<AppState>,
    Path((source_id, path_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db::get_source_path(&db, path_id) {
        Ok(Some(sp)) if sp.source_id != source_id => {
            return (
                StatusCode::NOT_FOUND,
                Json(SourcePathResponse {
                    status: "error".into(),
                    message: "Path not found".into(),
                    path: None,
                }),
            )
                .into_response();
        }
        _ => {}
    }
    match db::delete_source_path(&db, path_id) {
        Ok(true) => (
            StatusCode::OK,
            Json(SourcePathResponse {
                status: "success".into(),
                message: "Path deleted".into(),
                path: None,
            }),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(SourcePathResponse {
                status: "error".into(),
                message: "Path not found".into(),
                path: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SourcePathResponse {
                status: "error".into(),
                message: e.to_string(),
                path: None,
            }),
        )
            .into_response(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sources/{source_id}/paths",
            get(list_source_paths).post(create_source_path),
        )
        .route(
            "/sources/{source_id}/paths/{path_id}",
            axum::routing::put(update_source_path).delete(delete_source_path),
        )
}
