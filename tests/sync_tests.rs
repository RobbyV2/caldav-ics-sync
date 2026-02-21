use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use caldav_ics_sync::api::reverse_sync::run_reverse_sync;
use caldav_ics_sync::api::sync::{fetch_calendars, fetch_events, run_sync, toggle_slash};
use reqwest::{Client, header};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Mock CalDAV XML builders
// ---------------------------------------------------------------------------

fn mock_propfind_response(calendar_paths: &[&str]) -> String {
    let mut responses = String::new();
    for path in calendar_paths {
        responses.push_str(&format!(
            r#"<d:response>
  <d:href>{path}</d:href>
  <d:propstat>
    <d:prop>
      <d:resourcetype>
        <d:collection/>
        <c:calendar/>
      </d:resourcetype>
      <d:displayname>cal</d:displayname>
    </d:prop>
    <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
</d:response>"#,
        ));
    }

    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  {responses}
</d:multistatus>"#,
    )
}

fn mock_report_response(events: &[(&str, &str, &str, &str)]) -> String {
    let mut responses = String::new();
    for (uid, summary, dtstart, dtend) in events {
        let ics = format!(
            "BEGIN:VCALENDAR\r\n\
             VERSION:2.0\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             SUMMARY:{summary}\r\n\
             DTSTART:{dtstart}\r\n\
             DTEND:{dtend}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR"
        );
        responses.push_str(&format!(
            r#"<d:response>
  <d:href>/cal/{uid}.ics</d:href>
  <d:propstat>
    <d:prop>
      <d:getetag>"{uid}"</d:getetag>
      <c:calendar-data>{ics}</c:calendar-data>
    </d:prop>
    <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
</d:response>"#,
        ));
    }

    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  {responses}
</d:multistatus>"#,
    )
}

fn mock_ics_feed(events: &[(&str, &str, &str, &str)]) -> String {
    let mut body = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
    for (uid, summary, dtstart, dtend) in events {
        body.push_str(&format!(
            "BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             SUMMARY:{summary}\r\n\
             DTSTART:{dtstart}\r\n\
             DTEND:{dtend}\r\n\
             END:VEVENT\r\n"
        ));
    }
    body.push_str("END:VCALENDAR\r\n");
    body
}

// ---------------------------------------------------------------------------
// Mock server helpers
// ---------------------------------------------------------------------------

struct MockState {
    propfind_body: String,
    report_body: String,
    put_status: StatusCode,
}

async fn caldav_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<MockState>>,
    req: Request<Body>,
) -> Response {
    match req.method().as_str() {
        "PROPFIND" => (StatusCode::MULTI_STATUS, state.propfind_body.clone()).into_response(),
        "REPORT" => (StatusCode::MULTI_STATUS, state.report_body.clone()).into_response(),
        "PUT" => (state.put_status, "").into_response(),
        "GET" => {
            // Serve ICS feed for reverse_sync
            (StatusCode::OK, state.report_body.clone()).into_response()
        }
        _ => (StatusCode::METHOD_NOT_ALLOWED, "").into_response(),
    }
}

async fn start_mock_server(state: std::sync::Arc<MockState>) -> SocketAddr {
    let app = Router::new()
        .fallback(any(caldav_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn build_client(username: &str, password: &str) -> Client {
    let auth = format!("{}:{}", username, password);
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &auth);
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_str(&format!("Basic {}", encoded)).unwrap(),
    );
    Client::builder().default_headers(headers).build().unwrap()
}

// ---------------------------------------------------------------------------
// toggle_slash tests
// ---------------------------------------------------------------------------

#[test]
fn toggle_slash_adds_trailing_slash() {
    assert_eq!(
        toggle_slash("http://example.com/dav"),
        "http://example.com/dav/"
    );
}

#[test]
fn toggle_slash_removes_trailing_slash() {
    assert_eq!(
        toggle_slash("http://example.com/dav/"),
        "http://example.com/dav"
    );
}

#[test]
fn toggle_slash_roundtrips() {
    let original = "http://example.com/dav";
    let toggled = toggle_slash(original);
    let back = toggle_slash(&toggled);
    assert_eq!(back, original);
}

// ---------------------------------------------------------------------------
// fetch_calendars tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_calendars_returns_calendar_urls() {
    let state = std::sync::Arc::new(MockState {
        propfind_body: mock_propfind_response(&[
            "/dav/calendars/personal/",
            "/dav/calendars/work/",
        ]),
        report_body: String::new(),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");

    let cals = fetch_calendars(&client, &format!("http://{}/dav/", addr))
        .await
        .unwrap();

    assert_eq!(cals.len(), 2);
    assert!(cals.contains(&"/dav/calendars/personal/".to_string()));
    assert!(cals.contains(&"/dav/calendars/work/".to_string()));
}

#[tokio::test]
async fn fetch_calendars_retries_with_toggled_slash() {
    // The mock server always succeeds, but we verify the function handles
    // the URL with or without trailing slash by calling it both ways.
    let state = std::sync::Arc::new(MockState {
        propfind_body: mock_propfind_response(&["/cal/"]),
        report_body: String::new(),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");

    // Without trailing slash
    let cals = fetch_calendars(&client, &format!("http://{}/dav", addr))
        .await
        .unwrap();
    assert_eq!(cals.len(), 1);

    // With trailing slash
    let cals = fetch_calendars(&client, &format!("http://{}/dav/", addr))
        .await
        .unwrap();
    assert_eq!(cals.len(), 1);
}

#[tokio::test]
async fn fetch_calendars_returns_empty_when_no_calendars() {
    let state = std::sync::Arc::new(MockState {
        propfind_body: r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
</d:multistatus>"#
            .to_string(),
        report_body: String::new(),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");

    let cals = fetch_calendars(&client, &format!("http://{}/dav/", addr))
        .await
        .unwrap();

    assert!(cals.is_empty());
}

// ---------------------------------------------------------------------------
// fetch_events tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_events_returns_vevent_strings() {
    let events = [("uid-1", "Meeting", "20250101T100000Z", "20250101T110000Z")];
    let state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: mock_report_response(&events),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");
    let base = format!("http://{}", addr);

    let result = fetch_events(&client, &base, "/cal/").await.unwrap();

    assert_eq!(result.len(), 1);
    assert!(result[0].contains("BEGIN:VEVENT"));
    assert!(result[0].contains("SUMMARY:Meeting"));
}

#[tokio::test]
async fn fetch_events_handles_non_standard_port() {
    let events = [(
        "uid-port",
        "Port Test",
        "20250201T090000Z",
        "20250201T100000Z",
    )];
    let state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: mock_report_response(&events),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");

    // base_url includes the non-standard port; calendar_path is relative
    let base = format!("http://127.0.0.1:{}", addr.port());
    let result = fetch_events(&client, &base, "/cal/").await.unwrap();

    assert_eq!(result.len(), 1);
    assert!(result[0].contains("UID:uid-port"));
}

#[tokio::test]
async fn fetch_events_returns_empty_on_empty_calendar() {
    let state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: r#"<?xml version="1.0" encoding="utf-8" ?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
</d:multistatus>"#
            .to_string(),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;
    let client = build_client("user", "pass");
    let base = format!("http://{}", addr);

    let result = fetch_events(&client, &base, "/cal/").await.unwrap();

    assert!(result.is_empty());
}

// ---------------------------------------------------------------------------
// run_sync tests (full pipeline)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_sync_returns_correct_counts() {
    let events = [
        ("uid-a", "Alpha", "20250301T080000Z", "20250301T090000Z"),
        ("uid-b", "Beta", "20250301T100000Z", "20250301T110000Z"),
    ];
    let state = std::sync::Arc::new(MockState {
        propfind_body: mock_propfind_response(&["/cal/default/"]),
        report_body: mock_report_response(&events),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;

    let (event_count, calendar_count, _ics) =
        run_sync(&format!("http://{}/dav/", addr), "user", "pass")
            .await
            .unwrap();

    assert_eq!(calendar_count, 1);
    assert_eq!(event_count, 2);
}

#[tokio::test]
async fn run_sync_ics_output_has_vcalendar_wrapper() {
    let events = [("uid-wrap", "Wrap", "20250401T120000Z", "20250401T130000Z")];
    let state = std::sync::Arc::new(MockState {
        propfind_body: mock_propfind_response(&["/cal/"]),
        report_body: mock_report_response(&events),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;

    let (_ec, _cc, ics) = run_sync(&format!("http://{}/dav/", addr), "user", "pass")
        .await
        .unwrap();

    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(ics.ends_with("END:VCALENDAR\r\n"));
    assert!(ics.contains("VERSION:2.0"));
    assert!(ics.contains("PRODID:-//CalDAV/ICS Sync//EN"));
    assert!(ics.contains("BEGIN:VEVENT"));
    assert!(ics.contains("END:VEVENT"));
}

#[tokio::test]
async fn run_sync_handles_multiple_calendars() {
    // Each calendar path triggers the same REPORT response, so the mock
    // returns the same events per calendar. Two calendars with 1 event each
    // means 2 total events.
    let events = [("uid-multi", "Multi", "20250501T140000Z", "20250501T150000Z")];
    let state = std::sync::Arc::new(MockState {
        propfind_body: mock_propfind_response(&["/cal/a/", "/cal/b/"]),
        report_body: mock_report_response(&events),
        put_status: StatusCode::CREATED,
    });
    let addr = start_mock_server(state).await;

    let (event_count, calendar_count, ics) =
        run_sync(&format!("http://{}/dav/", addr), "user", "pass")
            .await
            .unwrap();

    assert_eq!(calendar_count, 2);
    assert_eq!(event_count, 2);
    // Both events are uid-multi so the VEVENT block should appear twice
    assert_eq!(ics.matches("UID:uid-multi").count(), 2);
}

// ---------------------------------------------------------------------------
// run_reverse_sync tests
// ---------------------------------------------------------------------------

/// Helper: start a mock that serves an ICS feed on GET and accepts PUTs on
/// a separate address. Returns (ics_server_addr, caldav_server_addr).
async fn start_reverse_sync_mocks(
    ics_events: &[(&str, &str, &str, &str)],
    put_status: StatusCode,
) -> (SocketAddr, SocketAddr) {
    let ics_feed = mock_ics_feed(ics_events);

    // ICS feed server (plain GET)
    let ics_state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: ics_feed,
        put_status: StatusCode::OK,
    });
    let ics_addr = start_mock_server(ics_state).await;

    // CalDAV server (accepts PUT)
    let caldav_state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: String::new(),
        put_status,
    });
    let caldav_addr = start_mock_server(caldav_state).await;

    (ics_addr, caldav_addr)
}

#[tokio::test]
async fn reverse_sync_uploads_events() {
    let events = [
        ("uid-r1", "Rev1", "20250601T080000Z", "20250601T090000Z"),
        ("uid-r2", "Rev2", "20250601T100000Z", "20250601T110000Z"),
    ];
    let (ics_addr, caldav_addr) = start_reverse_sync_mocks(&events, StatusCode::CREATED).await;

    let (uploaded, _skipped, total) = run_reverse_sync(
        &format!("http://{}/feed.ics", ics_addr),
        &format!("http://{}/dav/calendars", caldav_addr),
        "personal",
        "user",
        "pass",
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(uploaded, 2);
    assert_eq!(total, 2);
}

#[tokio::test]
async fn reverse_sync_handles_double_calendar_path() {
    // caldav_url already ends with the calendar name
    let events = [("uid-d1", "Double", "20250701T080000Z", "20250701T090000Z")];
    let (ics_addr, caldav_addr) = start_reverse_sync_mocks(&events, StatusCode::CREATED).await;

    let (uploaded, _skipped, total) = run_reverse_sync(
        &format!("http://{}/feed.ics", ics_addr),
        &format!("http://{}/dav/calendars/personal", caldav_addr),
        "personal",
        "user",
        "pass",
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(uploaded, 1);
    assert_eq!(total, 1);
}

#[tokio::test]
async fn reverse_sync_reports_correct_uploaded_count() {
    // 204 No Content is also a success status
    let events = [
        ("uid-c1", "Count1", "20250801T080000Z", "20250801T090000Z"),
        ("uid-c2", "Count2", "20250801T100000Z", "20250801T110000Z"),
        ("uid-c3", "Count3", "20250801T120000Z", "20250801T130000Z"),
    ];
    let (ics_addr, caldav_addr) = start_reverse_sync_mocks(&events, StatusCode::NO_CONTENT).await;

    let (uploaded, _skipped, total) = run_reverse_sync(
        &format!("http://{}/feed.ics", ics_addr),
        &format!("http://{}/dav/", caldav_addr),
        "work",
        "user",
        "pass",
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(uploaded, 3);
    assert_eq!(total, 3);
}

#[tokio::test]
async fn reverse_sync_returns_error_when_uploads_fail() {
    let events = [("uid-fail", "Fail", "20250901T080000Z", "20250901T090000Z")];
    let (ics_addr, caldav_addr) =
        start_reverse_sync_mocks(&events, StatusCode::INTERNAL_SERVER_ERROR).await;

    let result = run_reverse_sync(
        &format!("http://{}/feed.ics", ics_addr),
        &format!("http://{}/dav/", caldav_addr),
        "cal",
        "user",
        "pass",
        false,
        false,
    )
    .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("failed"),
        "Expected failure message, got: {err_msg}"
    );
}

#[tokio::test]
async fn reverse_sync_skips_unchanged_events() {
    let events = [
        (
            "uid-same",
            "Same Event",
            "20250601T080000Z",
            "20250601T090000Z",
        ),
        (
            "uid-new",
            "New Event",
            "20250601T100000Z",
            "20250601T110000Z",
        ),
    ];
    let ics_feed = mock_ics_feed(&events);

    // ICS feed server
    let ics_state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: ics_feed,
        put_status: StatusCode::OK,
    });
    let ics_addr = start_mock_server(ics_state).await;

    // CalDAV server that already has uid-same (returned via REPORT)
    let existing = [(
        "uid-same",
        "Same Event",
        "20250601T080000Z",
        "20250601T090000Z",
    )];
    let caldav_state = std::sync::Arc::new(MockState {
        propfind_body: String::new(),
        report_body: mock_report_response(&existing),
        put_status: StatusCode::CREATED,
    });
    let caldav_addr = start_mock_server(caldav_state).await;

    let (uploaded, skipped, total) = run_reverse_sync(
        &format!("http://{}/feed.ics", ics_addr),
        &format!("http://{}/dav/", caldav_addr),
        "cal",
        "user",
        "pass",
        false,
        false,
    )
    .await
    .unwrap();

    assert_eq!(total, 2);
    assert_eq!(skipped, 1, "uid-same should be skipped");
    assert_eq!(uploaded, 1, "only uid-new should be uploaded");
}
