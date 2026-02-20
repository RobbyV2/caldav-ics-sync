use axum::http::{HeaderName, Method, header};
use caldav_ics_sync::api::AppState;
use caldav_ics_sync::server::build_router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    std::fs::create_dir_all(&data_dir)?;
    let db_path = format!("{}/caldav-sync.db", data_dir);
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    caldav_ics_sync::db::init_db(&conn)?;
    info!("Database initialized at {}", db_path);

    let app_state = AppState {
        db: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        start_time: std::time::Instant::now(),
    };

    start_auto_sync(app_state.clone());

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::UPGRADE,
            header::CONNECTION,
            HeaderName::from_static("sec-websocket-key"),
            HeaderName::from_static("sec-websocket-version"),
            HeaderName::from_static("sec-websocket-protocol"),
        ])
        .allow_credentials(true);

    let app = build_router(app_state).await.layer(cors);

    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "6765".to_string());
    let addr = format!("{}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Starting server");
    info!("Listening on http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shutdown complete");

    Ok(())
}

fn start_auto_sync(state: AppState) {
    // Auto-sync sources (CalDAV -> ICS)
    let sources = {
        let db = state.db.lock().unwrap();
        caldav_ics_sync::db::list_sources(&db).unwrap_or_default()
    };

    for source in sources {
        if source.sync_interval_secs > 0 {
            let state = state.clone();
            let id = source.id;
            let interval_secs = source.sync_interval_secs as u64;
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    interval.tick().await;
                    let (url, user, pass) = {
                        let db = state.db.lock().unwrap();
                        match caldav_ics_sync::db::get_source(&db, id) {
                            Ok(Some(s)) => (s.caldav_url, s.username, s.password),
                            _ => break,
                        }
                    };
                    match caldav_ics_sync::api::sync::run_sync(&url, &user, &pass).await {
                        Ok((events, calendars, ics_data)) => {
                            let db = state.db.lock().unwrap();
                            let _ = caldav_ics_sync::db::save_ics_data(&db, id, &ics_data);
                            let _ = caldav_ics_sync::db::update_last_synced(&db, id);
                            let _ = caldav_ics_sync::db::update_sync_status(&db, id, "ok", None);
                            info!(
                                "Auto-sync source {}: {} events from {} calendars",
                                id, events, calendars
                            );
                        }
                        Err(e) => {
                            let db = state.db.lock().unwrap();
                            let _ = caldav_ics_sync::db::update_sync_status(
                                &db,
                                id,
                                "error",
                                Some(&e.to_string()),
                            );
                            tracing::error!("Auto-sync failed for source {}: {}", id, e);
                        }
                    }
                }
            });
            info!(
                "Auto-sync enabled for source {} (every {}s)",
                source.name, interval_secs
            );
        }
    }

    // Auto-sync destinations (ICS -> CalDAV)
    let destinations = {
        let db = state.db.lock().unwrap();
        caldav_ics_sync::db::list_destinations(&db).unwrap_or_default()
    };

    for dest in destinations {
        if dest.sync_interval_secs > 0 {
            let state = state.clone();
            let id = dest.id;
            let interval_secs = dest.sync_interval_secs as u64;
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    interval.tick().await;
                    let d = {
                        let db = state.db.lock().unwrap();
                        match caldav_ics_sync::db::get_destination(&db, id) {
                            Ok(Some(d)) => d,
                            _ => break,
                        }
                    };
                    match caldav_ics_sync::api::reverse_sync::run_reverse_sync(
                        &d.ics_url,
                        &d.caldav_url,
                        &d.calendar_name,
                        &d.username,
                        &d.password,
                        d.sync_all,
                        d.keep_local,
                    )
                    .await
                    {
                        Ok((uploaded, total)) => {
                            let db = state.db.lock().unwrap();
                            let _ = caldav_ics_sync::db::update_destination_sync_status(
                                &db, id, "ok", None,
                            );
                            info!(
                                "Auto-sync destination {}: uploaded {} of {} events",
                                id, uploaded, total
                            );
                        }
                        Err(e) => {
                            let db = state.db.lock().unwrap();
                            let _ = caldav_ics_sync::db::update_destination_sync_status(
                                &db,
                                id,
                                "error",
                                Some(&e.to_string()),
                            );
                            tracing::error!("Auto-sync failed for destination {}: {}", id, e);
                        }
                    }
                }
            });
            info!(
                "Auto-sync enabled for destination {} (every {}s)",
                dest.name, interval_secs
            );
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal, initiating graceful shutdown...");
        },
        _ = terminate => {
            info!("Received terminate signal, initiating graceful shutdown...");
        },
    }
}
