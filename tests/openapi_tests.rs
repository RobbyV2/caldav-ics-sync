use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use caldav_ics_sync::api::AppState;
use caldav_ics_sync::auto_sync;
use caldav_ics_sync::db;
use caldav_ics_sync::server::build_router;

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

async fn spawn_server() -> SocketAddr {
    let app = build_router(test_state(), PROXY_URL, None).await;
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

#[tokio::test]
async fn serves_openapi_spec() {
    let addr = spawn_server().await;
    let res = reqwest::get(format!("http://{}/api/openapi.json", addr))
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let spec: serde_json::Value = res.json().await.unwrap();
    assert_eq!(spec["info"]["title"], "CalDAV/ICS Sync API");
    assert!(
        spec["paths"]["/api/sources"].is_object(),
        "spec should document the sources endpoint"
    );
}

/// Swagger UI is enabled unless SWAGGER_UI=false. The env var is read when routes are
/// built, and `std::env::set_var` is unsafe under edition 2024, so only the default
/// (enabled) path is exercised here.
#[tokio::test]
async fn serves_swagger_ui() {
    let addr = spawn_server().await;
    let res = reqwest::get(format!("http://{}/api/swagger-ui/", addr))
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(
        body.contains("swagger-ui"),
        "expected the Swagger UI page, got: {}",
        &body[..body.len().min(200)]
    );
}
