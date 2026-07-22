mod helpers;

use helpers::{spawn_failing_gateway, spawn_mock_gateway};
use phototag_common::TagResponse;
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};
use tokio::net::TcpListener;

async fn spawn_phototag_server(gateway_url: String) -> String {
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
            .expect("serve phototag-server");
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn tag_endpoint_returns_keywords_on_success() {
    let gateway_url = spawn_mock_gateway("dog, beach, sunset").await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .header("content-type", "image/jpeg")
        .body(vec![0xff, 0xd8, 0xff]) // not a real JPEG — the server never decodes it
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 200);
    let body: TagResponse = response.json().await.expect("parse response");
    assert_eq!(body.keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn tag_endpoint_returns_bad_gateway_when_upstream_fails() {
    let gateway_url = spawn_failing_gateway().await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .header("content-type", "image/jpeg")
        .body(vec![0xff, 0xd8, 0xff])
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 502);
}

#[tokio::test]
async fn tag_endpoint_rejects_empty_body() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .body(Vec::<u8>::new())
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 400);
}
