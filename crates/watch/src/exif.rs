use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

/// Returns true if the file already has a non-empty `IPTC:Keywords` value.
pub async fn has_keywords(path: &Path) -> Result<bool> {
    let output = Command::new("exiftool")
        .arg("-charset")
        .arg("iptc=UTF8")
        .arg("-IPTC:Keywords")
        .arg("-s3")
        .arg("--")
        .arg(path)
        .output()
        .await
        .context("running exiftool -IPTC:Keywords")?;

    if !output.status.success() {
        bail!(
            "exiftool exited with {} for {}: {}",
            output.status,
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// Writes `keywords` into the file's `IPTC:Keywords` and `XMP-dc:Subject`
/// fields in place (no `_original` backup file).
pub async fn write_keywords(path: &Path, keywords: &[String]) -> Result<()> {
    if keywords.is_empty() {
        bail!(
            "refusing to write an empty keyword list to {}",
            path.display()
        );
    }

    let mut cmd = Command::new("exiftool");
    cmd.arg("-overwrite_original");
    cmd.arg("-charset").arg("iptc=UTF8");
    for kw in keywords {
        cmd.arg(format!("-IPTC:Keywords={kw}"));
        cmd.arg(format!("-XMP-dc:Subject={kw}"));
    }
    cmd.arg("--");
    cmd.arg(path);

    let output = cmd
        .output()
        .await
        .context("running exiftool to write keywords")?;
    if !output.status.success() {
        bail!(
            "exiftool exited with {} for {}: {}",
            output.status,
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let img = image::RgbImage::new(4, 4);
        img.save(&path).expect("save test jpeg");
        path
    }

    /// Test-only helper: reads a raw tag value via exiftool, independent of
    /// the functions under test, so assertions actually verify what got
    /// written rather than just re-exercising `has_keywords`.
    async fn read_tag(path: &std::path::Path, tag: &str) -> String {
        let output = tokio::process::Command::new("exiftool")
            .arg("-charset")
            .arg("iptc=UTF8")
            .arg(format!("-{tag}"))
            .arg("-s3")
            .arg("--")
            .arg(path)
            .output()
            .await
            .expect("running exiftool to read tag");
        assert!(
            output.status.success(),
            "exiftool read failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[tokio::test]
    async fn fresh_image_has_no_keywords() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "fresh.jpg");

        assert!(!has_keywords(&path).await.unwrap());
    }

    #[tokio::test]
    async fn write_then_read_round_trips_keywords() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "tagged.jpg");

        write_keywords(&path, &["dog".to_string(), "beach".to_string()])
            .await
            .unwrap();

        assert!(has_keywords(&path).await.unwrap());
        assert_eq!(read_tag(&path, "IPTC:Keywords").await, "dog, beach");
        assert_eq!(read_tag(&path, "XMP-dc:Subject").await, "dog, beach");

        // No `_original` backup file should be left behind.
        let backup = dir.path().join("tagged.jpg_original");
        assert!(!backup.exists(), "unexpected backup file: {backup:?}");
    }

    #[tokio::test]
    async fn write_keywords_rejects_empty_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "empty.jpg");

        let result = write_keywords(&path, &[]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn non_ascii_keywords_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "unicode.jpg");

        write_keywords(&path, &["café".to_string(), "日本語".to_string()])
            .await
            .unwrap();

        assert_eq!(read_tag(&path, "IPTC:Keywords").await, "café, 日本語");
        assert_eq!(read_tag(&path, "XMP-dc:Subject").await, "café, 日本語");
    }

    #[tokio::test]
    async fn dash_prefixed_filename_is_handled() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "-weird.jpg");

        assert!(!has_keywords(&path).await.unwrap());

        write_keywords(&path, &["dog".to_string()]).await.unwrap();

        assert!(has_keywords(&path).await.unwrap());
        assert_eq!(read_tag(&path, "IPTC:Keywords").await, "dog");
    }
}
