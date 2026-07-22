#![allow(dead_code)]

use axum::{routing::post, Json, Router};
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};
use serde_json::json;
use tokio::net::TcpListener;

/// Starts a tiny axum server mimicking `phototag-server`'s `POST /tag`,
/// always replying with `keywords`.
pub async fn spawn_mock_phototag_server(keywords: &[&str]) -> String {
    let body = json!({ "keywords": keywords });
    let app = Router::new().route(
        "/tag",
        post(move || {
            let body = body.clone();
            async move { Json(body) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock phototag-server");
    });
    format!("http://{addr}")
}

/// Starts a tiny axum server mimicking an OpenAI-compatible
/// `/chat/completions` endpoint, always replying with `content`.
pub async fn spawn_mock_gateway(content: &str) -> String {
    let content = content.to_string();
    let app = Router::new().route(
        "/chat/completions",
        post(move || {
            let content = content.clone();
            async move {
                Json(json!({
                    "choices": [{ "message": { "content": content } }]
                }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock gateway")
    });
    format!("http://{addr}")
}

/// Starts a real `phototag-server` pointed at `gateway_url`. Returns its base URL.
pub async fn spawn_phototag_server(gateway_url: String) -> String {
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".into(),
        gateway_url,
        gateway_model: "test-model".into(),
        gateway_timeout_secs: 5,
        prompt: None,
    };
    let state = create_app_state(config).await.expect("create_app_state");
    let app = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve phototag-server")
    });
    format!("http://{addr}")
}
