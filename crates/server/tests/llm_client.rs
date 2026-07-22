mod helpers;

use helpers::{spawn_failing_gateway, spawn_mock_gateway};
use phototag_server::config::ServerConfig;
use phototag_server::llm_client::GatewayClient;

fn test_config(gateway_url: String) -> ServerConfig {
    ServerConfig {
        listen_addr: "127.0.0.1:0".into(),
        gateway_url,
        gateway_model: "test-model".into(),
        gateway_timeout_secs: 5,
        prompt: None,
    }
}

#[tokio::test]
async fn extracts_keywords_from_comma_separated_response() {
    let base_url = spawn_mock_gateway("dog, beach, sunset").await;
    let client = GatewayClient::new(&test_config(base_url));

    let keywords = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await
        .expect("extract_keywords");

    assert_eq!(keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn extracts_keywords_from_json_array_response() {
    let base_url = spawn_mock_gateway(r#"["dog", "beach", "sunset"]"#).await;
    let client = GatewayClient::new(&test_config(base_url));

    let keywords = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await
        .expect("extract_keywords");

    assert_eq!(keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn errors_when_gateway_request_fails() {
    let base_url = spawn_failing_gateway().await;
    let client = GatewayClient::new(&test_config(base_url));

    let result = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn errors_on_apology_refusal_response() {
    let base_url =
        spawn_mock_gateway("I'm sorry, but I cannot help with that request as it violates policy.")
            .await;
    let client = GatewayClient::new(&test_config(base_url));

    let result = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await;

    assert!(
        result.is_err(),
        "a refusal response should bail rather than return a single bogus keyword"
    );
}

#[tokio::test]
async fn errors_on_unfortunately_refusal_response() {
    let base_url = spawn_mock_gateway(
        "Unfortunately, I am not able to process this image due to content policy restrictions.",
    )
    .await;
    let client = GatewayClient::new(&test_config(base_url));

    let result = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await;

    assert!(
        result.is_err(),
        "a refusal response should bail rather than return a single bogus keyword"
    );
}
