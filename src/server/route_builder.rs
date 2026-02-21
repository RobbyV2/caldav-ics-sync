use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

async fn proxy_to_nextjs(State(proxy_url): State<Arc<String>>, mut req: Request) -> Response {
    let proxy_uri = match proxy_url.parse::<hyper::Uri>() {
        Ok(uri) => uri,
        Err(e) => {
            tracing::error!("Invalid proxy URL {}: {}", proxy_url, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid proxy configuration",
            )
                .into_response();
        }
    };

    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(path);

    let new_uri = format!("{}{}", proxy_url, path_query);
    match new_uri.parse() {
        Ok(uri) => *req.uri_mut() = uri,
        Err(e) => {
            tracing::error!("Failed to parse URI {}: {}", new_uri, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid URI").into_response();
        }
    }

    if let Some(host) = proxy_uri.host() {
        let host_value = if let Some(port) = proxy_uri.port_u16() {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        };
        if let Ok(header_value) = host_value.parse() {
            req.headers_mut().insert(hyper::header::HOST, header_value);
        }
    }

    let client = Client::builder(TokioExecutor::new()).build_http();

    match client.request(req).await {
        Ok(response) => response.into_response(),
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, "Server not available").into_response()
        }
    }
}

fn ics_response(result: anyhow::Result<Option<String>>) -> Response {
    match result {
        Ok(Some(content)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/calendar")
            .body(axum::body::Body::from(content))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Ok(None) => (StatusCode::NOT_FOUND, "ICS not found").into_response(),
        Err(e) => {
            tracing::error!("Error serving ICS: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
        }
    }
}

async fn serve_ics(
    State(state): State<crate::api::AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    let Ok(db) = state.db.lock() else {
        tracing::error!("DB lock poisoned serving ICS /{}", path);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    };
    ics_response(crate::db::get_ics_data_by_path(&db, &path))
}

async fn serve_public_ics(
    State(state): State<crate::api::AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    if path.contains("..") || path.starts_with('/') {
        return (StatusCode::BAD_REQUEST, "Invalid path").into_response();
    }
    let Ok(db) = state.db.lock() else {
        tracing::error!("DB lock poisoned serving public ICS /{}", path);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    };
    ics_response(crate::db::get_ics_data_by_public_path(&db, &path))
}

pub async fn register_routes(state: crate::api::AppState, proxy_url: &str) -> Router {
    let api_routes = crate::api::routes();
    let proxy_url = Arc::new(proxy_url.to_owned());

    let fallback_router = Router::new()
        .fallback(proxy_to_nextjs)
        .with_state(proxy_url);

    Router::new()
        .nest("/api", api_routes)
        .route("/ics/public/{*path}", get(serve_public_ics))
        .route("/ics/{*path}", get(serve_ics))
        .merge(fallback_router)
        .with_state(state)
}
