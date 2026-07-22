mod helpers;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::backfill::run_backfill;
use phototag_watch::client::TaggerClient;
use phototag_watch::config::{Config, RootConfig, WatchSettings};
use phototag_watch::exif;

fn make_test_jpeg(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    image::RgbImage::new(4, 4)
        .save(&path)
        .expect("save test jpeg");
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
            RootConfig {
                name: "a".into(),
                path: root_a.path().to_path_buf(),
            },
            RootConfig {
                name: "b".into(),
                path: root_b.path().to_path_buf(),
            },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, None, false).await.unwrap();

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
            RootConfig {
                name: "a".into(),
                path: root_a.path().to_path_buf(),
            },
            RootConfig {
                name: "b".into(),
                path: root_b.path().to_path_buf(),
            },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, Some("a"), false)
        .await
        .unwrap();

    assert!(exif::has_keywords(&photo_a).await.unwrap());
    assert!(!exif::has_keywords(&photo_b).await.unwrap());
}

#[tokio::test]
async fn backfill_errors_when_only_root_names_a_nonexistent_root() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_a = tempfile::TempDir::new().unwrap();
    make_test_jpeg(root_a.path(), "a.jpg");

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![RootConfig {
            name: "a".into(),
            path: root_a.path().to_path_buf(),
        }],
        watch: WatchSettings::default(),
    };

    let result = run_backfill(&config, &client, Some("does-not-exist"), false).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn backfill_survives_a_root_with_a_nonexistent_path() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_broken = tempfile::TempDir::new().unwrap();
    let broken_path = root_broken.path().join("does-not-exist-subdir");

    let root_good = tempfile::TempDir::new().unwrap();
    let photo_good = make_test_jpeg(root_good.path(), "good.jpg");

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![
            RootConfig {
                name: "broken".into(),
                path: broken_path,
            },
            RootConfig {
                name: "good".into(),
                path: root_good.path().to_path_buf(),
            },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, None, false).await.unwrap();

    assert!(exif::has_keywords(&photo_good).await.unwrap());
}

#[tokio::test]
async fn backfill_reindexes_outdated_files_only_when_requested() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(root.path(), "old.jpg");
    exif::write_keywords(&path, &["phototag:v0.0.1".to_string()])
        .await
        .unwrap();

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![RootConfig {
            name: "root".into(),
            path: root.path().to_path_buf(),
        }],
        watch: WatchSettings::default(),
    };

    // Without the flag, the outdated marker is left alone.
    run_backfill(&config, &client, None, false).await.unwrap();
    let after_default = exif::read_keywords(&path).await.unwrap();
    assert!(after_default.contains(&"phototag:v0.0.1".to_string()));

    // With the flag, it gets reindexed to the current version.
    run_backfill(&config, &client, None, true).await.unwrap();
    let after_reindex = exif::read_keywords(&path).await.unwrap();
    assert!(!after_reindex.contains(&"phototag:v0.0.1".to_string()));
    assert!(after_reindex
        .iter()
        .any(|k| k.starts_with("phototag:v") && k != "phototag:v0.0.1"));
}
