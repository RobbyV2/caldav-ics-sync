use axum::http::{HeaderName, Method, header};
use axum::middleware;
use caldav_ics_sync::api::AppState;
use caldav_ics_sync::auto_sync;
use caldav_ics_sync::config::AppConfig;
use caldav_ics_sync::server::auth::{AuthConfig, basic_auth_middleware};
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

    let cfg = AppConfig::load()?;

    std::fs::create_dir_all(&cfg.data_dir)?;
    let db_path = format!("{}/caldav-sync.db", cfg.data_dir);
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    caldav_ics_sync::db::init_db(&conn)?;
    info!("Database initialized at {}", db_path);

    let proxy_url = cfg.proxy_url();

    let sync_tasks = auto_sync::new_registry();
    let app_state = AppState {
        db: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        start_time: std::time::Instant::now(),
        sync_tasks: sync_tasks.clone(),
    };

    auto_sync::register_all(&sync_tasks, &app_state);

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

    let auth_config = AuthConfig::from_config(&cfg);
    match &auth_config {
        AuthConfig::Disabled => {
            info!("HTTP Basic Auth disabled (AUTH_USERNAME not set or no password configured)");
        }
        AuthConfig::PlainText { username, .. } => {
            info!(
                "HTTP Basic Auth enabled for user '{}' (plain text)",
                username
            );
        }
        AuthConfig::Hashed { username, .. } => {
            info!(
                "HTTP Basic Auth enabled for user '{}' (argon2 hash)",
                username
            );
        }
    }

    let app = build_router(app_state.clone(), &proxy_url)
        .await
        .layer(middleware::from_fn(basic_auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(axum::Extension(app_state))
        .layer(cors);

    let addr = format!("{}:{}", cfg.server_host, cfg.server_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Starting server");
    info!("Listening on http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shutdown complete");

    Ok(())
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
