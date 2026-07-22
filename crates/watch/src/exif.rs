use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

/// Returns the file's current `IPTC:Keywords` list (empty if none set).
pub async fn read_keywords(path: &Path) -> Result<Vec<String>> {
    let output = Command::new("exiftool")
        .arg("-charset")
        .arg("iptc=UTF8")
        .arg("-j")
        .arg("-IPTC:Keywords")
        .arg("--")
        .arg(path)
        .output()
        .await
        .context("running exiftool -j -IPTC:Keywords")?;

    if !output.status.success() {
        bail!(
            "exiftool exited with {} for {}: {}",
            output.status,
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&stdout)
        .with_context(|| format!("parsing exiftool JSON output for {}", path.display()))?;
    let obj = parsed
        .into_iter()
        .next()
        .with_context(|| format!("exiftool returned no JSON object for {}", path.display()))?;

    Ok(match obj.get("Keywords") {
        None => Vec::new(),
        Some(serde_json::Value::Array(arr)) => {
            let mut keywords = Vec::with_capacity(arr.len());
            for v in arr {
                match json_value_to_keyword(v) {
                    Some(kw) => keywords.push(kw),
                    None => bail!(
                        "unexpected element in Keywords array for {}: {:?}",
                        path.display(),
                        v
                    ),
                }
            }
            keywords
        }
        Some(v) => match json_value_to_keyword(v) {
            Some(kw) => vec![kw],
            None => bail!(
                "unexpected JSON shape for Keywords tag on {}: {:?}",
                path.display(),
                v
            ),
        },
    })
}

/// Converts a single exiftool JSON scalar to its keyword text. exiftool's
/// `-j` output emits list values that look like numbers or booleans
/// (e.g. a keyword of `"2024"` or `"true"`) as native JSON numbers/bools
/// rather than strings, so all three scalar shapes must be accepted or a
/// legitimate keyword would silently vanish.
///
/// Integer-looking keyword text round-trips exactly; a keyword that looks
/// like a decimal number (e.g. `"1.50"`) may be reformatted (e.g. to
/// `"1.5"`) due to JSON number parsing -- not worth chasing further with
/// `arbitrary_precision` for this narrow a case.
fn json_value_to_keyword(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else if let Some(u) = n.as_u64() {
                Some(u.to_string())
            } else {
                Some(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Convenience predicate built on `read_keywords`: true if the file has
/// any `IPTC:Keywords` value at all, regardless of content.
pub async fn has_keywords(path: &Path) -> Result<bool> {
    Ok(!read_keywords(path).await?.is_empty())
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

    #[tokio::test]
    async fn read_keywords_returns_empty_vec_for_fresh_image() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "fresh.jpg");

        assert_eq!(read_keywords(&path).await.unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn read_keywords_returns_single_element_vec_for_one_keyword() {
        // Regression test for the exiftool JSON quirk: a single list value
        // serializes as a bare string, not a one-element array.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "one.jpg");
        write_keywords(&path, &["dog".to_string()]).await.unwrap();

        assert_eq!(read_keywords(&path).await.unwrap(), vec!["dog".to_string()]);
    }

    #[tokio::test]
    async fn read_keywords_returns_multi_element_vec_for_multiple_keywords() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "many.jpg");
        write_keywords(
            &path,
            &["dog".to_string(), "beach".to_string(), "sunset".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["dog".to_string(), "beach".to_string(), "sunset".to_string()]
        );
    }

    #[tokio::test]
    async fn read_keywords_handles_numeric_looking_single_keyword() {
        // Regression test: exiftool's -j output emits a single keyword that
        // looks like a number (e.g. a year) as a bare JSON number, not a
        // string. It must still round-trip as text, not vanish.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "numeric.jpg");
        write_keywords(&path, &["2024".to_string()]).await.unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["2024".to_string()]
        );
    }

    #[tokio::test]
    async fn read_keywords_handles_boolean_looking_single_keyword() {
        // Regression test: same quirk, but for a keyword that looks like a
        // JSON boolean.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "boolish.jpg");
        write_keywords(&path, &["true".to_string()]).await.unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["true".to_string()]
        );
    }

    #[tokio::test]
    async fn read_keywords_handles_numeric_and_boolean_keywords_in_a_mixed_list() {
        // Regression test: in a multi-value list, exiftool keeps
        // number/boolean-looking entries as native JSON types while other
        // entries stay strings. All of them must survive round-trip as text.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "mixed.jpg");
        write_keywords(
            &path,
            &["2024".to_string(), "dog".to_string(), "true".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["2024".to_string(), "dog".to_string(), "true".to_string()]
        );
    }

    #[tokio::test]
    async fn read_keywords_round_trips_integer_looking_keywords_exactly() {
        // Regression test: unlike decimal-looking keywords, integer-looking
        // keyword text (including large integers) must round-trip exactly,
        // not get reformatted via lossy f64 conversion.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "integers.jpg");
        write_keywords(&path, &["2024".to_string(), "1234567890".to_string()])
            .await
            .unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["2024".to_string(), "1234567890".to_string()]
        );
    }

    #[tokio::test]
    async fn read_keywords_still_returns_multi_element_vec_after_array_bail_refactor() {
        // Regression test: confirms the array branch's restructuring (to
        // bail on inconvertible elements) didn't change behavior for a
        // normal multi-value string list.
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "array_refactor.jpg");
        write_keywords(
            &path,
            &["dog".to_string(), "beach".to_string(), "sunset".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(
            read_keywords(&path).await.unwrap(),
            vec!["dog".to_string(), "beach".to_string(), "sunset".to_string()]
        );
    }
}
