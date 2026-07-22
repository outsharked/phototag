mod helpers;

use std::time::Duration;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::client::TaggerClient;
use phototag_watch::config::{Config, RootConfig, WatchSettings};
use phototag_watch::exif;
use phototag_watch::watcher::run_watch;

#[tokio::test]
async fn watcher_tags_a_newly_created_file() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root = tempfile::TempDir::new().unwrap();
    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![RootConfig {
            name: "root".into(),
            path: root.path().to_path_buf(),
        }],
        watch: WatchSettings {
            debounce_ms: 100,
            ..WatchSettings::default()
        },
    };

    let watch_handle = tokio::spawn(run_watch(config, client));

    // Give the watcher a moment to register its inotify watch before we
    // create the file, then write the image.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let path = root.path().join("new-photo.jpg");
    image::RgbImage::new(4, 4).save(&path).unwrap();

    // Poll for up to 5 seconds — comfortably longer than the 100ms debounce
    // plus the mock gateway round-trip.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if exif::has_keywords(&path).await.unwrap_or(false) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("file was not tagged within 5 seconds");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    watch_handle.abort();
}
