use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

/// Returns true if the file already has a non-empty `IPTC:Keywords` value.
pub async fn has_keywords(path: &Path) -> Result<bool> {
    let output = Command::new("exiftool")
        .arg("-IPTC:Keywords")
        .arg("-s3")
        .arg(path)
        .output()
        .await
        .context("running exiftool -IPTC:Keywords")?;

    if !output.status.success() {
        bail!(
            "exiftool exited with {}: {}",
            output.status,
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
    for kw in keywords {
        cmd.arg(format!("-IPTC:Keywords={kw}"));
        cmd.arg(format!("-XMP-dc:Subject={kw}"));
    }
    cmd.arg(path);

    let output = cmd
        .output()
        .await
        .context("running exiftool to write keywords")?;
    if !output.status.success() {
        bail!(
            "exiftool exited with {}: {}",
            output.status,
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
    }

    #[tokio::test]
    async fn write_keywords_rejects_empty_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "empty.jpg");

        let result = write_keywords(&path, &[]).await;

        assert!(result.is_err());
    }
}
