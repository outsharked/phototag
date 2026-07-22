use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::client::TaggerClient;
use crate::exif;

/// Prefix for the sentinel keyword phototag writes to mark a file as
/// indexed, e.g. `phototag:v0.1.0`.
const MARKER_PREFIX: &str = "phototag:v";

#[derive(Debug)]
pub enum TagOutcome {
    Tagged(Vec<String>),
    AlreadyTagged,
}

/// Tags a single file. If the file has no `phototag:v...` marker, it's
/// tagged for the first time — existing keywords (from a camera, manual
/// tagging, anything) are kept, new content keywords are added, and a
/// marker for the current version is appended. If it has a marker at or
/// above the current `phototag-watch` version, it's skipped. If it has an
/// older marker, it's skipped unless `reindex_outdated` is true, in which
/// case it's re-tagged the same way first-time tagging works: fresh
/// keywords are added, nothing existing is removed, and the marker is
/// replaced with the current version.
pub async fn tag_one_file(
    path: &Path,
    client: &TaggerClient,
    reindex_outdated: bool,
) -> Result<TagOutcome> {
    let existing = exif::read_keywords(path).await?;

    if let Some(marker_version) = find_phototag_marker(&existing) {
        let needs_reindex = reindex_outdated && marker_version < current_version();
        if !needs_reindex {
            return Ok(TagOutcome::AlreadyTagged);
        }
    }

    let new_content = client.tag_image(path).await?;
    let final_keywords = merge_keywords(&existing, &new_content);
    exif::write_keywords(path, &final_keywords).await?;
    Ok(TagOutcome::Tagged(new_content))
}

/// The running binary's own version, per its `Cargo.toml`.
fn current_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is valid semver")
}

/// The marker keyword for the running binary's version, e.g. `phototag:v0.1.0`.
fn phototag_marker() -> String {
    format!("{MARKER_PREFIX}{}", current_version())
}

/// If `kw` is a genuine `phototag:v{semver}` marker — i.e. it starts with
/// `MARKER_PREFIX` AND the remainder parses as valid semver — returns its
/// parsed version. A string that merely starts with `phototag:v` (e.g. a
/// human-written `phototag:vacation` keyword) returns `None`. This is the
/// single source of truth for what counts as a marker, shared by detection
/// (`find_phototag_marker`) and stripping (`merge_keywords`) so they can
/// never drift apart.
fn parse_marker(kw: &str) -> Option<semver::Version> {
    kw.strip_prefix(MARKER_PREFIX)
        .and_then(|v| semver::Version::parse(v).ok())
}

/// True if `kw` is a genuine `phototag:v{semver}` marker. See `parse_marker`.
fn is_phototag_marker(kw: &str) -> bool {
    parse_marker(kw).is_some()
}

/// Scans `keywords` for a `phototag:v...` entry and parses its version. A
/// malformed marker (e.g. hand-edited to invalid semver) is treated the
/// same as no marker at all, so the file gets tagged fresh rather than
/// erroring.
fn find_phototag_marker(keywords: &[String]) -> Option<semver::Version> {
    keywords.iter().filter_map(|kw| parse_marker(kw)).next()
}

/// Builds the final keyword list to write: `existing` (with any old
/// phototag marker stripped out) plus `new_content`, deduplicated
/// case-insensitively (first-seen casing wins), plus a fresh marker for
/// the current version appended at the end.
fn merge_keywords(existing: &[String], new_content: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for kw in existing {
        if is_phototag_marker(kw) {
            continue;
        }
        if seen.insert(kw.to_lowercase()) {
            result.push(kw.clone());
        }
    }
    for kw in new_content {
        if is_phototag_marker(kw) {
            continue;
        }
        if seen.insert(kw.to_lowercase()) {
            result.push(kw.clone());
        }
    }
    result.push(phototag_marker());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_phototag_marker_finds_a_valid_marker() {
        let keywords = vec!["dog".to_string(), "phototag:v0.1.0".to_string()];
        assert_eq!(
            find_phototag_marker(&keywords),
            Some(semver::Version::parse("0.1.0").unwrap())
        );
    }

    #[test]
    fn find_phototag_marker_returns_none_when_absent() {
        let keywords = vec!["dog".to_string(), "beach".to_string()];
        assert_eq!(find_phototag_marker(&keywords), None);
    }

    #[test]
    fn find_phototag_marker_treats_malformed_version_as_absent() {
        let keywords = vec!["phototag:vnotaversion".to_string()];
        assert_eq!(find_phototag_marker(&keywords), None);
    }

    #[test]
    fn merge_keywords_dedupes_case_insensitively_and_appends_marker() {
        let existing = vec!["Dog".to_string()];
        let new_content = vec!["dog".to_string(), "beach".to_string()];

        let merged = merge_keywords(&existing, &new_content);

        assert_eq!(merged[..2], ["Dog".to_string(), "beach".to_string()]);
        assert_eq!(merged[2], phototag_marker());
    }

    #[test]
    fn merge_keywords_strips_old_marker_before_adding_new_one() {
        let existing = vec!["dog".to_string(), "phototag:v0.0.1".to_string()];
        let new_content = vec!["beach".to_string()];

        let merged = merge_keywords(&existing, &new_content);

        assert_eq!(
            merged,
            vec!["dog".to_string(), "beach".to_string(), phototag_marker()]
        );
    }

    #[test]
    fn merge_keywords_strips_marker_shaped_strings_from_new_content_too() {
        let existing = vec!["dog".to_string()];
        let new_content = vec!["beach".to_string(), "phototag:v9.9.9".to_string()];

        let merged = merge_keywords(&existing, &new_content);

        // Only one phototag:v-prefixed entry should survive: the real marker
        // appended at the end, not the rogue one from new_content.
        let markers: Vec<&String> = merged
            .iter()
            .filter(|k| k.starts_with("phototag:v"))
            .collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], &phototag_marker());
    }

    #[test]
    fn merge_keywords_only_strips_a_genuine_marker_not_a_prefix_lookalike() {
        let existing = vec!["phototag:vacation".to_string()];
        let new_content = vec!["beach".to_string()];

        let merged = merge_keywords(&existing, &new_content);

        // "phototag:vacation" is not a valid marker (not real semver after the
        // prefix) and must survive — only a genuine phototag:v{semver} marker
        // should ever be stripped.
        assert!(merged.contains(&"phototag:vacation".to_string()));
        assert!(merged.contains(&"beach".to_string()));
        assert_eq!(
            merged
                .iter()
                .filter(|k| k.starts_with("phototag:v"))
                .count(),
            2 // the surviving lookalike, plus the one real marker appended
        );
    }

    #[test]
    fn find_phototag_marker_skips_past_a_malformed_entry_to_find_a_valid_one() {
        let keywords = vec![
            "phototag:vBADVERSION".to_string(),
            "phototag:v0.2.0".to_string(),
        ];
        assert_eq!(
            find_phototag_marker(&keywords),
            Some(semver::Version::parse("0.2.0").unwrap())
        );
    }
}
