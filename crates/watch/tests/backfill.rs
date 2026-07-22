mod helpers;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::backfill::run_backfill;
use phototag_watch::client::TaggerClient;
use phototag_watch::config::{Config, RootConfig, WatchSettings};
use phototag_watch::exif;

fn make_test_jpeg(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    image::RgbImage::new(4, 4).save(&path).expect("save test jpeg");
    path
}

#[tokio::test]
async fn backfill_tags_untagged_files_across_multiple_roots() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_a = tempfile::TempDir::new().unwrap();
    let root_b = tempfile::TempDir::new().unwrap();
    let photo_a = make_test_jpeg(root_a.path(), "a.jpg");
    let photo_b = make_test_jpeg(root_b.path(), "b.jpg");
    let ignored = root_a.path().join("notes.txt");
    std::fs::write(&ignored, b"not an image").unwrap();

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![
            RootConfig { name: "a".into(), path: root_a.path().to_path_buf() },
            RootConfig { name: "b".into(), path: root_b.path().to_path_buf() },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, None).await.unwrap();

    assert!(exif::has_keywords(&photo_a).await.unwrap());
    assert!(exif::has_keywords(&photo_b).await.unwrap());
}

#[tokio::test]
async fn backfill_can_be_restricted_to_a_single_named_root() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_a = tempfile::TempDir::new().unwrap();
    let root_b = tempfile::TempDir::new().unwrap();
    let photo_a = make_test_jpeg(root_a.path(), "a.jpg");
    let photo_b = make_test_jpeg(root_b.path(), "b.jpg");

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![
            RootConfig { name: "a".into(), path: root_a.path().to_path_buf() },
            RootConfig { name: "b".into(), path: root_b.path().to_path_buf() },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, Some("a")).await.unwrap();

    assert!(exif::has_keywords(&photo_a).await.unwrap());
    assert!(!exif::has_keywords(&photo_b).await.unwrap());
}
