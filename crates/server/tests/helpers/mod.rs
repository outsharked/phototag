#![allow(dead_code)]

use axum::{routing::post, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

/// Starts a tiny axum server that mimics an OpenAI-compatible
/// `/chat/completions` endpoint, always replying with `content` as the
/// assistant message text. Returns its base URL (no trailing slash).
pub async fn spawn_mock_gateway(content: &str) -> String {
    let content = content.to_string();
    let app = Router::new().route(
        "/chat/completions",
        post(move || {
            let content = content.clone();
            async move {
                Json(json!({
                    "choices": [
                        { "message": { "content": content } }
                    ]
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock gateway");
    });
    format!("http://{addr}")
}

/// Same as `spawn_mock_gateway`, but the endpoint always returns a 500.
pub async fn spawn_failing_gateway() -> String {
    let app = Router::new().route(
        "/chat/completions",
        post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve failing gateway");
    });
    format!("http://{addr}")
}
