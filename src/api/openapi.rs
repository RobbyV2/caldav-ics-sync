use crate::api::AppState;
use crate::api::destinations::{DestinationListResponse, DestinationResponse, ReverseSyncResult};
use crate::api::health::{DetailedHealthResponse, HealthResponse};
use crate::api::sources::{SourceListResponse, SourceResponse, SyncResult};
use crate::db::{
    CreateDestination, CreateSource, Destination, Source, UpdateDestination, UpdateSource,
};
use axum::{Json, Router, response::IntoResponse, routing::get};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::sources::list_sources,
        crate::api::sources::create_source,
        crate::api::sources::update_source,
        crate::api::sources::delete_source_handler,
        crate::api::sources::sync_source,
        crate::api::sources::source_status,
        crate::api::destinations::list_destinations,
        crate::api::destinations::create_destination,
        crate::api::destinations::update_destination,
        crate::api::destinations::delete_destination,
        crate::api::destinations::sync_destination,
        crate::api::health::health,
        crate::api::health::health_detailed,
    ),
    components(schemas(
        Source,
        CreateSource,
        UpdateSource,
        SourceResponse,
        SourceListResponse,
        SyncResult,
        Destination,
        CreateDestination,
        UpdateDestination,
        DestinationResponse,
        DestinationListResponse,
        ReverseSyncResult,
        HealthResponse,
        DetailedHealthResponse,
    )),
    info(
        title = "CalDAV/ICS Sync API",
        version = "0.2.0",
        description = "Bidirectional CalDAV and ICS synchronization API. Manage CalDAV-to-ICS sources (pull events and serve as ICS) and ICS-to-CalDAV destinations (push ICS events to CalDAV servers)."
    )
)]
pub struct ApiDoc;

async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/openapi.json", get(openapi_json))
}
