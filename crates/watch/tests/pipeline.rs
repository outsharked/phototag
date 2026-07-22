mod helpers;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::client::TaggerClient;
use phototag_watch::exif;
use phototag_watch::pipeline::{tag_one_file, TagOutcome};

fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    image::RgbImage::new(4, 4)
        .save(&path)
        .expect("save test jpeg");
    path
}

#[tokio::test]
async fn tags_a_fresh_image_end_to_end() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");

    let outcome = tag_one_file(&path, &client).await.unwrap();

    match outcome {
        TagOutcome::Tagged(keywords) => assert_eq!(keywords, vec!["dog", "beach"]),
        TagOutcome::AlreadyTagged => panic!("expected Tagged, got AlreadyTagged"),
    }
    assert!(exif::has_keywords(&path).await.unwrap());
}

#[tokio::test]
async fn skips_an_already_tagged_image() {
    let gateway_url = spawn_mock_gateway("should-not-be-called").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");
    exif::write_keywords(&path, &["existing".to_string()])
        .await
        .unwrap();

    let outcome = tag_one_file(&path, &client).await.unwrap();

    assert!(matches!(outcome, TagOutcome::AlreadyTagged));
}
