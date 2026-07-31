use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use hyper::upgrade::OnUpgrade;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};

use crate::config::RateLimit;

type ProxyClient = Client<HttpConnector, Body>;

/// Everything the proxy fallback needs, resolved once at router-build time.
#[derive(Clone)]
struct ProxyCtx {
    base: Arc<str>,
    host: Option<HeaderValue>,
    client: ProxyClient,
}

impl ProxyCtx {
    fn new(proxy_url: &str) -> Self {
        // Resolve the upstream Host header once instead of re-parsing the proxy URL on
        // every single proxied request.
        let host = match proxy_url.parse::<hyper::Uri>() {
            Ok(uri) => uri.host().and_then(|host| {
                let value = match uri.port_u16() {
                    Some(port) => format!("{}:{}", host, port),
                    None => host.to_string(),
                };
                value.parse().ok()
            }),
            Err(e) => {
                tracing::warn!("Invalid proxy URL {}: {}", proxy_url, e);
                None
            }
        };

        Self {
            base: Arc::from(proxy_url),
            host,
            // Build the client once so its connection pool is reused across every
            // proxied request rather than allocating a fresh pool per request.
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }
}

async fn proxy_to_nextjs(proxy: ProxyCtx, mut req: Request) -> Response {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(path);

    let new_uri = format!("{}{}", proxy.base, path_query);
    match new_uri.parse() {
        Ok(uri) => *req.uri_mut() = uri,
        Err(e) => {
            tracing::error!("Failed to parse URI {}: {}", new_uri, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid URI").into_response();
        }
    }

    if let Some(host) = proxy.host.clone() {
        req.headers_mut().insert(hyper::header::HOST, host);
    }

    // Take the client-side upgrade handle before forwarding so the two connections can be
    // spliced together if the frontend answers with a protocol upgrade (e.g. WebSockets,
    // including the Next.js dev-server HMR socket).
    let client_upgrade = req.extensions_mut().remove::<OnUpgrade>();

    match proxy.client.request(req).await {
        Ok(mut response) => {
            if response.status() == StatusCode::SWITCHING_PROTOCOLS
                && let Some(client_upgrade) = client_upgrade
            {
                let backend_upgrade = hyper::upgrade::on(&mut response);
                tokio::spawn(async move {
                    match (client_upgrade.await, backend_upgrade.await) {
                        (Ok(client_io), Ok(backend_io)) => {
                            let mut client_io = TokioIo::new(client_io);
                            let mut backend_io = TokioIo::new(backend_io);
                            let _ = tokio::io::copy_bidirectional(&mut client_io, &mut backend_io)
                                .await;
                        }
                        (Err(e), _) | (_, Err(e)) => tracing::error!("Upgrade error: {}", e),
                    }
                });
            }
            response.into_response()
        }
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

fn apply_rate_limit(
    router: Router<crate::api::AppState>,
    limit: RateLimit,
) -> Router<crate::api::AppState> {
    // `GovernorConfigBuilder::per_second(n)` sets the token *replenish interval* to n
    // seconds (so `per_second(2)` means one request every 2s, i.e. 0.5 req/s) — the
    // opposite of what the name suggests. Configure the interval in milliseconds so
    // `RATE_LIMIT_PER_SECOND` means what it says.
    let replenish_ms = (1000 / limit.per_second.max(1)).max(1);
    let builder = || {
        let mut b = GovernorConfigBuilder::default();
        b.per_millisecond(replenish_ms).burst_size(limit.burst);
        b
    };

    if limit.trust_proxy {
        let conf = builder()
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("invalid rate limit config");
        router.layer(GovernorLayer::new(conf))
    } else {
        let conf = builder().finish().expect("invalid rate limit config");
        router.layer(GovernorLayer::new(conf))
    }
}

pub async fn register_routes(
    state: crate::api::AppState,
    proxy_url: &str,
    rate_limit: Option<RateLimit>,
) -> Router {
    let api_routes = crate::api::routes();
    let proxy = ProxyCtx::new(proxy_url);

    let mut routes = Router::new()
        .nest("/api", api_routes)
        .route("/ics/public/{*path}", get(serve_public_ics))
        .route("/ics/{*path}", get(serve_ics));

    // Applied before the fallback is registered so it covers only the API and ICS
    // routes — a single UI page load pulls dozens of static assets through the proxy
    // fallback and would otherwise exhaust the bucket immediately.
    if let Some(limit) = rate_limit {
        routes = apply_rate_limit(routes, limit);
    }

    routes.with_state(state).fallback(move |req: Request| {
        let proxy = proxy.clone();
        async move { proxy_to_nextjs(proxy, req).await }
    })
}
