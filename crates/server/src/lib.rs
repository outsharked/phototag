pub mod config;
pub mod llm_client;
pub mod tag;

use anyhow::Result;
use axum::{extract::DefaultBodyLimit, routing::post, Router};

use config::ServerConfig;
use llm_client::GatewayClient;

#[derive(Clone)]
pub struct AppState {
    pub gateway: GatewayClient,
}

pub async fn create_app_state(config: ServerConfig) -> Result<AppState> {
    Ok(AppState {
        gateway: GatewayClient::new(&config),
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/tag", post(tag::tag_handler))
        // Default axum body limit (2MB) is too small for real photos.
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state)
}
