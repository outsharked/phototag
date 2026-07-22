mod helpers;

use helpers::spawn_mock_phototag_server;
use phototag_watch::client::TaggerClient;

fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let img = image::RgbImage::new(4, 4);
    img.save(&path).expect("save test jpeg");
    path
}

#[tokio::test]
async fn tag_image_returns_keywords_from_server() {
    let server_url = spawn_mock_phototag_server(&["dog", "beach"]).await;
    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");

    let client = TaggerClient::new(server_url);
    let keywords = client.tag_image(&path).await.unwrap();

    assert_eq!(keywords, vec!["dog".to_string(), "beach".to_string()]);
}
