use axum::Router;

pub mod auth;
pub mod route_builder;

pub async fn build_router(
    state: crate::api::AppState,
    proxy_url: &str,
    rate_limit: Option<crate::config::RateLimit>,
) -> Router {
    route_builder::register_routes(state, proxy_url, rate_limit).await
}
