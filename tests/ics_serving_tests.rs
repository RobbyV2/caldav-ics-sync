use std::sync::{Arc, Mutex};

use axum::http::{Request, StatusCode, header};
use axum::middleware;
use base64::Engine;
use caldav_ics_sync::api::AppState;
use caldav_ics_sync::auto_sync;
use caldav_ics_sync::db::{self, CreateSource, CreateSourcePath};
use caldav_ics_sync::server::auth::{AuthConfig, basic_auth_middleware};
use caldav_ics_sync::server::build_router;
use http_body_util::BodyExt;
use tower::ServiceExt;

const VCALENDAR: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR";
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

fn insert_source(
    state: &AppState,
    ics_path: &str,
    public_ics: bool,
    public_ics_path: Option<&str>,
) -> i64 {
    let db = state.db.lock().unwrap();
    db::create_source(
        &db,
        &CreateSource {
            name: "Test".into(),
            caldav_url: "https://example.com/dav".into(),
            username: "user".into(),
            password: "pass".into(),
            ics_path: ics_path.into(),
            sync_interval_secs: 0,
            public_ics,
            public_ics_path: public_ics_path.map(str::to_owned),
        },
    )
    .unwrap()
}

fn save_ics(state: &AppState, source_id: i64, content: &str) {
    let db = state.db.lock().unwrap();
    db::save_ics_data(&db, source_id, content).unwrap();
}

fn insert_source_path(state: &AppState, source_id: i64, path: &str, is_public: bool) -> i64 {
    let db = state.db.lock().unwrap();
    db::create_source_path(
        &db,
        source_id,
        &CreateSourcePath {
            path: path.into(),
            is_public,
        },
    )
    .unwrap()
}

async fn router_no_auth(state: AppState) -> axum::Router {
    build_router(state, PROXY_URL).await
}

async fn router_with_auth(state: AppState) -> axum::Router {
    let auth_config = AuthConfig::PlainText {
        username: "test".into(),
        password: "test".into(),
    };
    build_router(state.clone(), PROXY_URL)
        .await
        .layer(middleware::from_fn(basic_auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(axum::Extension(state))
}

fn basic_auth_header(user: &str, pass: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, pass));
    format!("Basic {}", encoded)
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ---------------------------------------------------------------------------
// ICS Serving (no auth)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ics_existing_path_returns_200() {
    let state = test_state();
    let id = insert_source(&state, "test-path", false, None);
    save_ics(&state, id, VCALENDAR);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/test-path")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "text/calendar");
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn ics_nonexistent_returns_404() {
    let state = test_state();
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/nonexistent")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_ics_returns_200_when_enabled() {
    let state = test_state();
    let id = insert_source(&state, "src-path", true, Some("custom-public"));
    save_ics(&state, id, VCALENDAR);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/custom-public")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn public_ics_returns_404_when_disabled() {
    let state = test_state();
    let id = insert_source(&state, "priv-path", false, None);
    save_ics(&state, id, VCALENDAR);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/priv-path")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_ics_rejects_path_traversal() {
    let state = test_state();
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/traversal/../etc")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ics_via_source_path_returns_200() {
    let state = test_state();
    let id = insert_source(&state, "main-path", false, None);
    save_ics(&state, id, VCALENDAR);
    insert_source_path(&state, id, "alias-path", false);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/alias-path")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn public_ics_via_source_path_returns_200() {
    let state = test_state();
    let id = insert_source(&state, "sp-main", false, None);
    save_ics(&state, id, VCALENDAR);
    insert_source_path(&state, id, "sp-public-alias", true);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/sp-public-alias")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn non_public_source_path_via_public_route_returns_404() {
    let state = test_state();
    let id = insert_source(&state, "sp-priv-main", false, None);
    save_ics(&state, id, VCALENDAR);
    insert_source_path(&state, id, "sp-private-alias", false);
    let app = router_no_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/sp-private-alias")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Auth Middleware
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_health_exempt_returns_200() {
    let state = test_state();
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/api/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_sources_without_credentials_returns_401() {
    let state = test_state();
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/api/sources")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_sources_with_valid_credentials_returns_200() {
    let state = test_state();
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/api/sources")
                .header(header::AUTHORIZATION, basic_auth_header("test", "test"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_sources_with_wrong_credentials_returns_401() {
    let state = test_state();
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/api/sources")
                .header(header::AUTHORIZATION, basic_auth_header("test", "wrong"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_public_ics_bypasses_auth() {
    let state = test_state();
    let id = insert_source(&state, "auth-src", true, Some("auth-pub"));
    save_ics(&state, id, VCALENDAR);
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/public/auth-pub")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_private_ics_without_credentials_returns_401() {
    let state = test_state();
    let id = insert_source(&state, "private-ics", false, None);
    save_ics(&state, id, VCALENDAR);
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/private-ics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_private_ics_with_credentials_returns_200() {
    let state = test_state();
    let id = insert_source(&state, "auth-priv-ics", false, None);
    save_ics(&state, id, VCALENDAR);
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/auth-priv-ics")
                .header(header::AUTHORIZATION, basic_auth_header("test", "test"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn auth_public_standard_ics_no_custom_path_bypasses_auth() {
    let state = test_state();
    let id = insert_source(&state, "std-pub-ics", true, None);
    save_ics(&state, id, VCALENDAR);
    let app = router_with_auth(state).await;

    let resp = app
        .oneshot(
            Request::get("/ics/std-pub-ics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("BEGIN:VCALENDAR"));
}
