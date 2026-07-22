#![allow(dead_code)]

use axum::{routing::post, Json, Router};
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
