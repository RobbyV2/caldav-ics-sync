use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use rusqlite::Connection;
use serde_json::Value;
use tower::ServiceExt;

use caldav_ics_sync::api::AppState;
use caldav_ics_sync::auto_sync;
use caldav_ics_sync::db;

fn test_state() -> AppState {
    let conn = Connection::open_in_memory().expect("in-memory DB");
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .expect("enable FK");
    db::init_db(&conn).expect("init_db");
    AppState {
        db: Arc::new(Mutex::new(conn)),
        start_time: Instant::now(),
        sync_tasks: auto_sync::new_registry(),
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .nest("/api", caldav_ics_sync::api::routes())
        .with_state(state)
}

async fn body_json(body: Body) -> Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn source_json() -> Value {
    serde_json::json!({
        "name": "Test Source",
        "caldav_url": "https://caldav.example.com/dav",
        "username": "user",
        "password": "pass",
        "ics_path": "test.ics",
        "sync_interval_secs": 0
    })
}

fn destination_json() -> Value {
    serde_json::json!({
        "name": "Test Dest",
        "ics_url": "https://example.com/cal.ics",
        "caldav_url": "https://caldav.example.com/dav",
        "calendar_name": "TestCal",
        "username": "user",
        "password": "pass",
        "sync_interval_secs": 0
    })
}

// ---------- Sources: create ----------

#[tokio::test]
async fn create_source_returns_201() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("content-type", "application/json")
                .body(Body::from(source_json().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["status"], "success");
    assert!(json["source"]["id"].as_i64().is_some());
    assert_eq!(json["source"]["name"], "Test Source");
}

#[tokio::test]
async fn create_source_missing_fields_returns_400() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "X",
                        "caldav_url": "https://example.com",
                        "username": "u",
                        "password": "",
                        "ics_path": "a.ics",
                        "sync_interval_secs": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_source_invalid_json_returns_422() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"X"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------- Sources: list ----------

#[tokio::test]
async fn list_sources_returns_created() {
    let state = test_state();

    {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
    }

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/sources")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["sources"].as_array().unwrap().len(), 1);
    assert_eq!(json["sources"][0]["name"], "Test Source");
}

// ---------- Sources: update ----------

#[tokio::test]
async fn update_source_returns_200() {
    let state = test_state();

    let id = {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/sources/{}", id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"name": "Renamed"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["source"]["name"], "Renamed");
}

#[tokio::test]
async fn update_source_nonexistent_returns_404() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/sources/999")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({"name": "X"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Sources: delete ----------

#[tokio::test]
async fn delete_source_returns_200() {
    let state = test_state();

    let id = {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/sources/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn delete_source_nonexistent_returns_404() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/sources/999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Source Paths: create ----------

#[tokio::test]
async fn create_source_path_returns_201() {
    let state = test_state();

    let source_id = {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{}/paths", source_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"path": "alt.ics"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["path"]["path"], "alt.ics");
}

// ---------- Source Paths: list ----------

#[tokio::test]
async fn list_source_paths_returns_200() {
    let state = test_state();

    let source_id = {
        let db = state.db.lock().unwrap();
        let sid = db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
        db::create_source_path(
            &db,
            sid,
            &serde_json::from_value(serde_json::json!({"path": "extra.ics"})).unwrap(),
        )
        .unwrap();
        sid
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(format!("/api/sources/{}/paths", source_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["paths"].as_array().unwrap().len(), 1);
}

// ---------- Source Paths: update ----------

#[tokio::test]
async fn update_source_path_returns_200() {
    let state = test_state();

    let (source_id, path_id) = {
        let db = state.db.lock().unwrap();
        let sid = db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
        let pid = db::create_source_path(
            &db,
            sid,
            &serde_json::from_value(serde_json::json!({"path": "extra.ics"})).unwrap(),
        )
        .unwrap();
        (sid, pid)
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/sources/{}/paths/{}", source_id, path_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"path": "renamed.ics"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["path"]["path"], "renamed.ics");
}

#[tokio::test]
async fn update_source_path_wrong_source_returns_404() {
    let state = test_state();

    let (_source_id, path_id) = {
        let db = state.db.lock().unwrap();
        let sid = db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
        let pid = db::create_source_path(
            &db,
            sid,
            &serde_json::from_value(serde_json::json!({"path": "extra.ics"})).unwrap(),
        )
        .unwrap();
        (sid, pid)
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/sources/9999/paths/{}", path_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({"path": "x.ics"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Source Paths: delete ----------

#[tokio::test]
async fn delete_source_path_returns_200() {
    let state = test_state();

    let (source_id, path_id) = {
        let db = state.db.lock().unwrap();
        let sid = db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
        let pid = db::create_source_path(
            &db,
            sid,
            &serde_json::from_value(serde_json::json!({"path": "extra.ics"})).unwrap(),
        )
        .unwrap();
        (sid, pid)
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/sources/{}/paths/{}", source_id, path_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------- Destinations: create ----------

#[tokio::test]
async fn create_destination_returns_201() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/destinations")
                .header("content-type", "application/json")
                .body(Body::from(destination_json().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["status"], "success");
    assert!(json["destination"]["id"].as_i64().is_some());
}

// ---------- Destinations: list ----------

#[tokio::test]
async fn list_destinations_returns_created() {
    let state = test_state();

    {
        let db = state.db.lock().unwrap();
        db::create_destination(&db, &serde_json::from_value(destination_json()).unwrap()).unwrap();
    }

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/destinations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["destinations"].as_array().unwrap().len(), 1);
}

// ---------- Destinations: update ----------

#[tokio::test]
async fn update_destination_returns_200() {
    let state = test_state();

    let id = {
        let db = state.db.lock().unwrap();
        db::create_destination(&db, &serde_json::from_value(destination_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/destinations/{}", id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"name": "Renamed Dest"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["destination"]["name"], "Renamed Dest");
}

#[tokio::test]
async fn update_destination_nonexistent_returns_404() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/destinations/999")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({"name": "X"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Destinations: delete ----------

#[tokio::test]
async fn delete_destination_returns_200() {
    let state = test_state();

    let id = {
        let db = state.db.lock().unwrap();
        db::create_destination(&db, &serde_json::from_value(destination_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/destinations/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn delete_destination_nonexistent_returns_404() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/destinations/999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Health ----------

#[tokio::test]
async fn health_returns_200() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_detailed_returns_200() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/health/detailed")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert!(json["db_ok"].as_bool().unwrap());
    assert!(json["uptime_seconds"].as_u64().is_some());
}

// ---------- OpenAPI ----------

#[tokio::test]
async fn openapi_json_returns_200_with_paths() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp.into_body()).await;
    assert!(json["paths"].as_object().is_some());
    assert!(!json["paths"].as_object().unwrap().is_empty());
}

// ---------- Validation ----------

#[tokio::test]
async fn create_source_with_public_ics_path_returns_400() {
    let state = test_state();
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "Bad",
                        "caldav_url": "https://example.com",
                        "username": "u",
                        "password": "p",
                        "ics_path": "public",
                        "sync_interval_secs": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp.into_body()).await;
    assert!(json["message"].as_str().unwrap().contains("public"));
}

#[tokio::test]
async fn create_source_duplicate_ics_path_returns_400() {
    let state = test_state();

    {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap();
    }

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("content-type", "application/json")
                .body(Body::from(source_json().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp.into_body()).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("duplicate")
    );
}

#[tokio::test]
async fn create_source_path_with_public_prefix_returns_400() {
    let state = test_state();

    let source_id = {
        let db = state.db.lock().unwrap();
        db::create_source(&db, &serde_json::from_value(source_json()).unwrap()).unwrap()
    };

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{}/paths", source_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"path": "public/foo"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = body_json(resp.into_body()).await;
    assert!(json["message"].as_str().unwrap().contains("public"));
}
