use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use caldav_ics_sync::api::AppState;
use caldav_ics_sync::auto_sync;
use caldav_ics_sync::config::RateLimit;
use caldav_ics_sync::db;
use caldav_ics_sync::server::build_router;

/// Port nothing is listening on, so proxied requests fail fast with 502.
const PROXY_URL: &str = "http://127.0.0.1:19999";

fn test_state() -> AppState {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    db::init_db(&conn).unwrap();
    AppState {
        db: Arc::new(Mutex::new(conn)),
        start_time: std::time::Instant::now(),
        sync_tasks: auto_sync::new_registry(),
    }
}

/// Serve the router on an ephemeral port with connect info, as `main` does. The rate
/// limiter keys on the peer address, so it only works through a real socket.
async fn spawn_server(rate_limit: Option<RateLimit>) -> SocketAddr {
    let app = build_router(test_state(), PROXY_URL, rate_limit).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    addr
}

async fn get_status(client: &reqwest::Client, url: &str) -> u16 {
    client.get(url).send().await.unwrap().status().as_u16()
}

#[tokio::test]
async fn rate_limit_rejects_burst_and_allows_first_request() {
    let addr = spawn_server(Some(RateLimit {
        per_second: 1,
        burst: 2,
        trust_proxy: false,
    }))
    .await;

    let client = reqwest::Client::new();
    let url = format!("http://{}/api/health", addr);

    let mut statuses = Vec::new();
    for _ in 0..8 {
        statuses.push(get_status(&client, &url).await);
    }

    assert_eq!(
        statuses[0], 200,
        "first request must be served, got {:?}",
        statuses
    );
    assert!(
        statuses.contains(&429),
        "a burst of 8 requests against burst=2 must be throttled, got {:?}",
        statuses
    );
}

#[tokio::test]
async fn rate_limit_disabled_by_default() {
    let addr = spawn_server(None).await;

    let client = reqwest::Client::new();
    let url = format!("http://{}/api/health", addr);

    for _ in 0..20 {
        assert_eq!(get_status(&client, &url).await, 200);
    }
}

/// The limiter is registered before the proxy fallback so it covers only /api and /ics.
/// A single page load pulls dozens of assets through the fallback, which would otherwise
/// drain the bucket and lock out the API.
#[tokio::test]
async fn rate_limit_does_not_cover_proxy_fallback() {
    let addr = spawn_server(Some(RateLimit {
        per_second: 1,
        burst: 1,
        trust_proxy: false,
    }))
    .await;

    let client = reqwest::Client::new();

    // Drain the bucket.
    let api_url = format!("http://{}/api/health", addr);
    for _ in 0..5 {
        get_status(&client, &api_url).await;
    }
    assert_eq!(
        get_status(&client, &api_url).await,
        429,
        "bucket should be drained"
    );

    // The fallback still reaches the (absent) frontend rather than being throttled.
    let asset_url = format!("http://{}/some-static-asset.js", addr);
    for _ in 0..5 {
        assert_eq!(
            get_status(&client, &asset_url).await,
            502,
            "proxy fallback must not be rate limited"
        );
    }
}

/// `RATE_LIMIT_PER_SECOND` is a request rate, not a replenish interval in seconds.
/// tower-governor's `per_second(n)` means the latter, which would make a configured
/// "10 req/s" actually allow one request every 10 seconds.
#[tokio::test]
async fn rate_limit_replenishes_at_configured_rate() {
    let addr = spawn_server(Some(RateLimit {
        per_second: 20,
        burst: 1,
        trust_proxy: false,
    }))
    .await;

    let client = reqwest::Client::new();
    let url = format!("http://{}/api/health", addr);

    // Drain the single-token bucket.
    get_status(&client, &url).await;

    // At 20 req/s a token replenishes every 50ms. Had the interval been misread as
    // 20 seconds, nothing would be available here.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert_eq!(
        get_status(&client, &url).await,
        200,
        "a token should have replenished within 250ms at 20 req/s"
    );
}
