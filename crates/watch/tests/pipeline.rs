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

    let outcome = tag_one_file(&path, &client, false).await.unwrap();

    match outcome {
        TagOutcome::Tagged(keywords) => assert_eq!(keywords, vec!["dog", "beach"]),
        TagOutcome::AlreadyTagged => panic!("expected Tagged, got AlreadyTagged"),
    }
    assert!(exif::has_keywords(&path).await.unwrap());
}

#[tokio::test]
async fn tags_an_image_that_already_has_non_phototag_keywords() {
    // A photo with keywords from some other source (camera, manual
    // tagging) but no phototag marker should still get tagged — and the
    // pre-existing keyword should survive alongside the new ones.
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");
    exif::write_keywords(&path, &["vacation".to_string()])
        .await
        .unwrap();

    let outcome = tag_one_file(&path, &client, false).await.unwrap();

    match outcome {
        TagOutcome::Tagged(keywords) => assert_eq!(keywords, vec!["dog", "beach"]),
        TagOutcome::AlreadyTagged => panic!("expected Tagged, got AlreadyTagged"),
    }
    let final_keywords = exif::read_keywords(&path).await.unwrap();
    assert!(final_keywords.contains(&"vacation".to_string()));
    assert!(final_keywords.contains(&"dog".to_string()));
    assert!(final_keywords.contains(&"beach".to_string()));
}

#[tokio::test]
async fn skips_an_image_already_tagged_at_the_current_version() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");

    let first = tag_one_file(&path, &client, false).await.unwrap();
    assert!(matches!(first, TagOutcome::Tagged(_)));

    let second = tag_one_file(&path, &client, false).await.unwrap();
    assert!(matches!(second, TagOutcome::AlreadyTagged));
}

#[tokio::test]
async fn skips_an_outdated_marker_by_default() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");
    exif::write_keywords(&path, &["phototag:v0.0.1".to_string()])
        .await
        .unwrap();

    let outcome = tag_one_file(&path, &client, false).await.unwrap();

    assert!(matches!(outcome, TagOutcome::AlreadyTagged));
}

#[tokio::test]
async fn reindexes_an_outdated_marker_when_requested() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");
    exif::write_keywords(
        &path,
        &["vacation".to_string(), "phototag:v0.0.1".to_string()],
    )
    .await
    .unwrap();

    let outcome = tag_one_file(&path, &client, true).await.unwrap();

    assert!(matches!(outcome, TagOutcome::Tagged(_)));
    let final_keywords = exif::read_keywords(&path).await.unwrap();
    // The pre-existing manual keyword survives the reindex.
    assert!(final_keywords.contains(&"vacation".to_string()));
    // The new content keywords are present.
    assert!(final_keywords.contains(&"dog".to_string()));
    assert!(final_keywords.contains(&"beach".to_string()));
    // The old marker is gone, replaced by exactly one current-version marker.
    let markers: Vec<&String> = final_keywords
        .iter()
        .filter(|k| k.starts_with("phototag:v"))
        .collect();
    assert_eq!(markers.len(), 1);
    assert_ne!(markers[0], "phototag:v0.0.1");
}
