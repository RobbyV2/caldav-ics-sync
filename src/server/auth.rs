use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordVerifier},
};
use axum::{
    Extension,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use subtle::ConstantTimeEq;

use crate::config::AppConfig;

const AUTH_EXEMPT_PATHS: &[&str] = &["/api/health"];

#[derive(Clone)]
pub enum AuthConfig {
    Disabled,
    PlainText {
        username: String,
        password: String,
    },
    Hashed {
        username: String,
        password_hash: String,
    },
}

impl AuthConfig {
    pub fn from_config(cfg: &AppConfig) -> Self {
        let Some(username) = cfg.auth_username.as_deref().filter(|s| !s.is_empty()) else {
            return Self::Disabled;
        };

        if let Some(hash) = cfg.auth_password_hash.as_deref().filter(|s| !s.is_empty()) {
            return Self::Hashed {
                username: username.to_owned(),
                password_hash: hash.to_owned(),
            };
        }

        if let Some(pass) = cfg.auth_password.as_deref().filter(|s| !s.is_empty()) {
            return Self::PlainText {
                username: username.to_owned(),
                password: pass.to_owned(),
            };
        }

        Self::Disabled
    }

    fn username(&self) -> &str {
        match self {
            AuthConfig::PlainText { username, .. } | AuthConfig::Hashed { username, .. } => {
                username
            }
            AuthConfig::Disabled => unreachable!(),
        }
    }
}

fn unauthorized() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"caldav-ics-sync\""),
        )
        .body(axum::body::Body::from("Unauthorized"))
        .unwrap_or_else(|_| StatusCode::UNAUTHORIZED.into_response())
}

pub async fn basic_auth_middleware(
    Extension(config): Extension<AuthConfig>,
    req: Request,
    next: Next,
) -> Response {
    if matches!(config, AuthConfig::Disabled) {
        return next.run(req).await;
    }

    let path = req.uri().path().to_owned();

    if AUTH_EXEMPT_PATHS.iter().any(|p| path == *p) {
        return next.run(req).await;
    }

    if path.starts_with("/ics/public/") {
        return next.run(req).await;
    }

    if let Some(true) = path.strip_prefix("/ics/").map(|ics_path| {
        let Some(state) = req.extensions().get::<crate::api::AppState>() else {
            return false;
        };
        let db = match state.db.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("DB lock poisoned in auth middleware: {}", e);
                return false;
            }
        };
        match crate::db::is_public_standard_ics(&db, ics_path) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("DB error checking public ICS: {}", e);
                false
            }
        }
    }) {
        return next.run(req).await;
    }

    let Some((req_user, req_pass)) = extract_credentials(&req) else {
        return unauthorized();
    };

    if req_user
        .as_bytes()
        .ct_eq(config.username().as_bytes())
        .unwrap_u8()
        != 1
    {
        return unauthorized();
    }

    match &config {
        AuthConfig::PlainText { password, .. } => {
            if req_pass.as_bytes().ct_eq(password.as_bytes()).unwrap_u8() != 1 {
                return unauthorized();
            }
        }
        AuthConfig::Hashed { password_hash, .. } => {
            let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
                tracing::error!("AUTH_PASSWORD_HASH is not a valid PHC-format hash");
                return unauthorized();
            };
            if Argon2::default()
                .verify_password(req_pass.as_bytes(), &parsed_hash)
                .is_err()
            {
                return unauthorized();
            }
        }
        AuthConfig::Disabled => unreachable!(),
    }

    next.run(req).await
}

fn extract_credentials(req: &Request) -> Option<(String, String)> {
    let auth_header = req.headers().get(header::AUTHORIZATION)?;
    let auth_str = auth_header.to_str().ok()?;
    let encoded = auth_str.strip_prefix("Basic ")?;
    let decoded_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded_bytes).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_owned(), pass.to_owned()))
}
