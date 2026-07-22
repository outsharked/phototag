# Version Marker & Reindex Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `phototag:v{version}` sentinel keyword that `phototag-watch` writes into every file it tags, so already-tagged photos are skipped on future runs, and photos tagged by an older `phototag-watch` version can be deliberately re-tagged via a new `--reindex-outdated` flag — without ever deleting keywords that weren't written by phototag.

**Architecture:** `exif.rs` gains a `read_keywords` primitive (parses exiftool's JSON output into a real `Vec<String>`, replacing the old boolean-only check) with `has_keywords` kept as a thin convenience wrapper over it. `pipeline.rs` gains the marker/version logic — detecting an existing marker, deciding whether to skip/tag/reindex, and merging keyword lists additively. `backfill.rs` and `main.rs` thread a new `reindex_outdated: bool` through to `pipeline::tag_one_file`; `watcher.rs` always passes `false`, since newly-created files never have a marker yet.

**Tech Stack:** Same as the existing `phototag-watch` crate (Rust, `tokio`, `exiftool` subprocess calls) plus the `semver` crate for correct version comparison.

---

## Before you start

This plan modifies `crates/watch/src/exif.rs`, `pipeline.rs`, `backfill.rs`,
`watcher.rs`, and `main.rs` — all already implemented and tested from the
prior 14-task plan. Read the current content of each file before editing;
this plan shows the exact diffs/final content, but line numbers may have
shifted since this plan was written.

---

## Task 1: Add dependencies

**Files:**
- Modify: `crates/watch/Cargo.toml`

`semver` is needed for correct version comparison (`0.9.0` must sort before
`0.10.0`, which plain string comparison gets wrong). `serde_json` is
currently only a dev-dependency (used by test helpers) — it needs to move
to a real dependency since `exif.rs`'s production code will parse
exiftool's JSON output starting in Task 2.

- [ ] **Step 1: Edit `crates/watch/Cargo.toml`**

Change the `[dependencies]` and `[dev-dependencies]` sections to:

```toml
[dependencies]
phototag-common = { path = "../common" }
anyhow = { workspace = true }
clap = { workspace = true }
notify = "8"
reqwest = { workspace = true }
semver = "1"
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
walkdir = "2"

[dev-dependencies]
phototag-server = { path = "../server" }
axum = "0.8"
image = { version = "0.25", default-features = false, features = ["jpeg"] }
tempfile = { workspace = true }
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

(`serde_json` moved out of `[dev-dependencies]` into `[dependencies]`;
`semver` added to `[dependencies]`.)

- [ ] **Step 2: Verify it builds**

Run: `cargo build -p phototag-watch`
Expected: `Finished` with no errors (nothing uses the new deps yet, so this
just confirms they resolve and fetch correctly).

- [ ] **Step 3: Commit**

```bash
git add crates/watch/Cargo.toml Cargo.lock
git commit -m "Add semver dependency, promote serde_json to a real dependency"
```

---

## Task 2: `exif.rs` — `read_keywords`

**Files:**
- Modify: `crates/watch/src/exif.rs`

Replaces the internal implementation of the keyword-read path with one that
parses exiftool's actual JSON list output, instead of a raw joined string.
**Important gotcha, verified against the real installed exiftool:** with
`-j` (JSON) output, a list-type tag with exactly one value serializes as a
bare JSON string (`"Keywords": "dog"`), not a one-element array — only 2+
values produce a JSON array (`"Keywords": ["dog","beach"]`). An absent tag
omits the key entirely. `read_keywords` must handle all three shapes.
`has_keywords` stays as a public function, but becomes a thin wrapper over
`read_keywords` rather than its own separate exiftool call — this keeps
every existing caller (including test call sites) working unchanged.

- [ ] **Step 1: Write the failing tests**

Add these three tests to the `#[cfg(test)] mod tests` block in
`crates/watch/src/exif.rs` (alongside the five existing tests — don't
remove or modify those, they'll keep passing once `has_keywords` becomes a
wrapper):

```rust
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
    write_keywords(&path, &["dog".to_string(), "beach".to_string(), "sunset".to_string()])
        .await
        .unwrap();

    assert_eq!(
        read_keywords(&path).await.unwrap(),
        vec!["dog".to_string(), "beach".to_string(), "sunset".to_string()]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p phototag-watch --lib read_keywords`
Expected: FAIL to compile — `error[E0425]: cannot find function 'read_keywords' in this scope`.

- [ ] **Step 3: Replace `has_keywords` with `read_keywords` + a thin wrapper**

Replace the existing `has_keywords` function in `crates/watch/src/exif.rs`
(the one using `-s3`) with:

```rust
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
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => Vec::new(),
    })
}

/// Convenience predicate built on `read_keywords`: true if the file has
/// any `IPTC:Keywords` value at all, regardless of content.
pub async fn has_keywords(path: &Path) -> Result<bool> {
    Ok(!read_keywords(path).await?.is_empty())
}
```

Leave `write_keywords` and the rest of the file (including the
`#[cfg(test)] mod tests` block's `make_test_jpeg`/`read_tag` helpers and
the five pre-existing tests) unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p phototag-watch --lib`
Expected: all 11 tests in `exif::tests` pass (the 5 pre-existing ones,
still exercising `has_keywords`/`write_keywords` unchanged, plus the 3 new
`read_keywords` tests).

- [ ] **Step 5: Commit**

```bash
git add crates/watch/src/exif.rs
git commit -m "Add exif::read_keywords, keep has_keywords as a wrapper over it"
```

---

## Task 3: `pipeline.rs` — marker detection, merge logic, reindex-aware `tag_one_file`

**Files:**
- Modify: `crates/watch/src/pipeline.rs`
- Modify: `crates/watch/tests/pipeline.rs`

This is the core of the feature: detecting the `phototag:v...` marker,
deciding skip/tag/reindex, and merging keyword lists so nothing existing
ever gets deleted.

- [ ] **Step 1: Write the failing unit tests for the pure helpers**

Add this `#[cfg(test)] mod tests` block to the bottom of
`crates/watch/src/pipeline.rs` (it doesn't have one yet):

```rust
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p phototag-watch --lib pipeline::tests`
Expected: FAIL to compile — `find_phototag_marker`, `merge_keywords`,
`phototag_marker` don't exist yet.

- [ ] **Step 3: Implement the marker/merge logic and update `tag_one_file`**

Replace the entire content of `crates/watch/src/pipeline.rs` (above the new
test module you just added) with:

```rust
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

/// Scans `keywords` for a `phototag:v...` entry and parses its version. A
/// malformed marker (e.g. hand-edited to invalid semver) is treated the
/// same as no marker at all, so the file gets tagged fresh rather than
/// erroring.
fn find_phototag_marker(keywords: &[String]) -> Option<semver::Version> {
    keywords
        .iter()
        .find_map(|kw| kw.strip_prefix(MARKER_PREFIX))
        .and_then(|v| semver::Version::parse(v).ok())
}

/// Builds the final keyword list to write: `existing` (with any old
/// phototag marker stripped out) plus `new_content`, deduplicated
/// case-insensitively (first-seen casing wins), plus a fresh marker for
/// the current version appended at the end.
fn merge_keywords(existing: &[String], new_content: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for kw in existing {
        if kw.starts_with(MARKER_PREFIX) {
            continue;
        }
        if seen.insert(kw.to_lowercase()) {
            result.push(kw.clone());
        }
    }
    for kw in new_content {
        if seen.insert(kw.to_lowercase()) {
            result.push(kw.clone());
        }
    }
    result.push(phototag_marker());
    result
}
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p phototag-watch --lib pipeline::tests`
Expected: all 5 tests pass.

- [ ] **Step 5: Replace `crates/watch/tests/pipeline.rs`**

The existing `skips_an_already_tagged_image` test asserts the *old*
behavior ("any keyword at all means skip"), which the new marker-based
logic deliberately changes. Replace the entire file with:

```rust
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
```

- [ ] **Step 6: Run the full `phototag-watch` test suite to verify everything passes**

Run: `cargo test -p phototag-watch`
Expected: all tests pass — 11 in `exif::tests` (Task 2), 5 in
`pipeline::tests` (this task's unit tests), 5 in `tests/pipeline.rs` (this
task's integration tests), plus whatever `tests/client.rs` and
`tests/backfill.rs`/`tests/watcher.rs` currently have (those will fail to
compile at this point, since `tag_one_file`'s signature changed and Tasks
4/5 haven't updated their call sites yet — that's expected and fixed in
the next two tasks; if you want a clean run right now, run
`cargo test -p phototag-watch --lib pipeline::tests --test pipeline`
instead to scope to just what this task touched).

- [ ] **Step 7: Commit**

```bash
git add crates/watch/src/pipeline.rs crates/watch/tests/pipeline.rs
git commit -m "Add phototag version marker detection and reindex-aware tagging"
```

---

## Task 4: `backfill.rs` — thread `reindex_outdated` through

**Files:**
- Modify: `crates/watch/src/backfill.rs`
- Modify: `crates/watch/tests/backfill.rs`

- [ ] **Step 1: Update `run_backfill`/`backfill_root` signatures**

Replace the entire content of `crates/watch/src/backfill.rs` with:

```rust
use anyhow::{bail, Result};
use walkdir::WalkDir;

use crate::client::TaggerClient;
use crate::config::{Config, RootConfig};
use crate::pipeline::{tag_one_file, TagOutcome};

/// Walks each configured root once, tagging every file that doesn't yet
/// have a current-version phototag marker. If `only_root` is set, only
/// that named root is walked. If `reindex_outdated` is true, files whose
/// marker is older than the running binary's version are re-tagged too
/// (see `pipeline::tag_one_file`); otherwise they're left alone.
pub async fn run_backfill(
    config: &Config,
    client: &TaggerClient,
    only_root: Option<&str>,
    reindex_outdated: bool,
) -> Result<()> {
    let mut matched_any = false;
    for root in &config.roots {
        if let Some(name) = only_root {
            if root.name != name {
                continue;
            }
            matched_any = true;
        }
        tracing::info!(root = %root.name, path = %root.path.display(), "backfill starting");
        backfill_root(root, &config.watch, client, reindex_outdated).await;
    }
    if let Some(name) = only_root {
        if !matched_any {
            bail!("no configured root named '{name}'");
        }
    }
    Ok(())
}

async fn backfill_root(
    root: &RootConfig,
    watch: &crate::config::WatchSettings,
    client: &TaggerClient,
    reindex_outdated: bool,
) {
    for entry in WalkDir::new(&root.path) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(root = %root.name, error = %e, "error walking directory tree, skipping entry");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !watch.matches_extension(path) {
            continue;
        }
        match tag_one_file(path, client, reindex_outdated).await {
            Ok(TagOutcome::Tagged(keywords)) => {
                tracing::info!(path = %path.display(), ?keywords, "tagged");
            }
            Ok(TagOutcome::AlreadyTagged) => {
                tracing::debug!(path = %path.display(), "already tagged, skipping");
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "tagging failed, skipping");
            }
        }
    }
}
```

- [ ] **Step 2: Update the 4 existing test call sites**

In `crates/watch/tests/backfill.rs`, every call to `run_backfill(...)` needs
a trailing `false` argument added (none of these tests care about reindex
behavior — that's covered by the new test in Step 3). Specifically:

- `backfill_tags_untagged_files_across_multiple_roots`: change
  `run_backfill(&config, &client, None).await.unwrap();` to
  `run_backfill(&config, &client, None, false).await.unwrap();`
- `backfill_can_be_restricted_to_a_single_named_root`: change
  `run_backfill(&config, &client, Some("a")).await.unwrap();` to
  `run_backfill(&config, &client, Some("a"), false).await.unwrap();`
- `backfill_errors_when_only_root_names_a_nonexistent_root`: change
  `run_backfill(&config, &client, Some("does-not-exist")).await;` to
  `run_backfill(&config, &client, Some("does-not-exist"), false).await;`
- `backfill_survives_a_root_with_a_nonexistent_path`: change
  `run_backfill(&config, &client, None).await.unwrap();` to
  `run_backfill(&config, &client, None, false).await.unwrap();`

Leave everything else in those four tests unchanged (including all
`exif::has_keywords` calls — `has_keywords` still exists as a wrapper from
Task 2, so those assertions keep working as-is).

- [ ] **Step 3: Add a new test for `--reindex-outdated` wiring**

Add this test to `crates/watch/tests/backfill.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p phototag-watch --test backfill`
Expected: all 5 tests pass (the 4 pre-existing ones, now compiling again
with the updated call sites, plus the new reindex test).

- [ ] **Step 5: Commit**

```bash
git add crates/watch/src/backfill.rs crates/watch/tests/backfill.rs
git commit -m "Thread reindex_outdated through phototag-watch backfill"
```

---

## Task 5: `watcher.rs` — never reindex from watch mode

**Files:**
- Modify: `crates/watch/src/watcher.rs`

Continuous watch mode only ever reacts to newly-created/modified files,
which by definition have no marker yet — reindexing is a deliberate,
on-demand operation, not something that should ever trigger from ordinary
filesystem events.

- [ ] **Step 1: Update the `tag_one_file` call site**

In `crates/watch/src/watcher.rs`, find this line (inside the debounce-fire
branch of the `tokio::select!` loop):

```rust
match tag_one_file(&path, &client).await {
```

Change it to:

```rust
match tag_one_file(&path, &client, false).await {
```

No other changes to this file are needed.

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p phototag-watch --test watcher`
Expected: both existing tests pass (`watcher_tags_a_newly_created_file`,
`watcher_with_no_roots_runs_without_panicking_or_erroring`) — neither
needed changes, since they call `run_watch`, not `tag_one_file` directly,
and `exif::has_keywords` still exists.

- [ ] **Step 3: Commit**

```bash
git add crates/watch/src/watcher.rs
git commit -m "Never reindex outdated markers from continuous watch mode"
```

---

## Task 6: `main.rs` — `--reindex-outdated` CLI flag

**Files:**
- Modify: `crates/watch/src/main.rs`

Not a TDD task — pure CLI wiring, verified by manual smoke tests, matching
how the original `--backfill`/`--backfill-root` flags were verified.

- [ ] **Step 1: Update `main.rs`**

Replace the entire content of `crates/watch/src/main.rs` with:

```rust
// crates/watch/src/main.rs
use std::path::PathBuf;

use clap::Parser;
use phototag_watch::{backfill, client::TaggerClient, config, watcher};

/// Watches configured photo library roots and tags new images, or
/// (with --backfill) walks them once instead of watching.
#[derive(Debug, Parser)]
#[command(name = "phototag-watch")]
struct Cli {
    /// Path to the phototag-watch TOML config file.
    #[arg(long, default_value = "phototag-watch.toml")]
    config: PathBuf,

    /// Walk the configured roots once instead of watching.
    #[arg(long)]
    backfill: bool,

    /// With --backfill, only process the root with this name.
    #[arg(long)]
    backfill_root: Option<String>,

    /// With --backfill, also re-tag files whose phototag marker is older
    /// than this build's version (adding fresh keywords, never removing
    /// existing ones). Has no effect without --backfill.
    #[arg(long)]
    reindex_outdated: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = config::load_config(&cli.config)?;
    let client = TaggerClient::new(config.server_url.clone());

    if cli.backfill {
        backfill::run_backfill(
            &config,
            &client,
            cli.backfill_root.as_deref(),
            cli.reindex_outdated,
        )
        .await?;
    } else {
        watcher::run_watch(config, client).await?;
    }
    Ok(())
}
```

- [ ] **Step 2: Verify `--help` shows the new flag**

Run: `cargo build -p phototag-watch && ./target/debug/phototag-watch --help`
Expected: usage text lists `--config`, `--backfill`, `--backfill-root`, and
`--reindex-outdated` with the descriptions above.

- [ ] **Step 3: Smoke-test `--backfill --reindex-outdated` against an empty directory**

```bash
mkdir -p /tmp/phototag-reindex-smoketest/pictures
cat > /tmp/phototag-reindex-smoketest/config.toml <<'EOF'
server_url = "http://127.0.0.1:1"

[[roots]]
name = "pictures"
path = "/tmp/phototag-reindex-smoketest/pictures"
EOF
./target/debug/phototag-watch --config /tmp/phototag-reindex-smoketest/config.toml --backfill --reindex-outdated
rm -rf /tmp/phototag-reindex-smoketest
```
Expected: exits `0` (empty directory, nothing to process, the unreachable
`server_url` is never contacted).

- [ ] **Step 4: Commit**

```bash
git add crates/watch/src/main.rs
git commit -m "Add --reindex-outdated CLI flag to phototag-watch"
```

---

## Task 7: README update and final verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the "Running `phototag-watch`" section**

In `README.md`, find this block:

```markdown
## Running `phototag-watch`

Copy `phototag-watch.example.toml`, fill in real paths and your
`phototag-server` URL, then:

```bash
cargo run -p phototag-watch -- --config your-config.toml           # watch mode
cargo run -p phototag-watch -- --config your-config.toml --backfill # one-shot catch-up
```
```

Replace it with:

```markdown
## Running `phototag-watch`

Copy `phototag-watch.example.toml`, fill in real paths and your
`phototag-server` URL, then:

```bash
cargo run -p phototag-watch -- --config your-config.toml           # watch mode
cargo run -p phototag-watch -- --config your-config.toml --backfill # one-shot catch-up
cargo run -p phototag-watch -- --config your-config.toml --backfill --reindex-outdated # also re-tag files from an older phototag-watch version
```

## Version tracking

Every file `phototag-watch` tags gets a `phototag:v{version}` keyword
added alongside the content keywords, recording which `phototag-watch`
version indexed it. On later runs, a file with a marker at or above the
current version is skipped entirely (no LLM call). A file with an older
marker is also skipped by default — pass `--reindex-outdated` (with
`--backfill`) to re-tag those too. Reindexing only ever adds keywords; it
never removes anything already on the file, including keywords added by
hand.
```

- [ ] **Step 2: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: every test across `phototag-common`, `phototag-server`, and
`phototag-watch` passes.

- [ ] **Step 3: Run clippy across the workspace**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings. Fix anything that comes up before proceeding.

- [ ] **Step 4: Run fmt across the workspace**

Run: `cargo fmt --workspace -- --check`
Expected: clean. If not, run `cargo fmt --workspace` and re-verify tests
still pass.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "Document version marker and --reindex-outdated in README"
```

---

## Self-review notes

- **Spec coverage:** marker format (`phototag:v{version}`, mixed into the
  existing keyword list) — Task 3. `read_keywords` replacing the boolean
  check, with `exif.rs` staying scoped to subprocess-wrapping only — Task
  2 (note: `find_phototag_marker`/`merge_keywords` deliberately live in
  `pipeline.rs`, not `exif.rs`, per the spec's explicit resolution of that
  ambiguity). Tag-check/reindex table (all 4 rows) — Task 3's `tag_one_file`
  and its 5 integration tests. `--reindex-outdated` CLI flag, backfill-only
  — Tasks 4 and 6. Watch mode never reindexing — Task 5. Case-insensitive
  dedup matching `phototag-server`'s convention — `merge_keywords` in Task
  3. Out-of-scope on-demand CLI tool — deliberately not built; `tag_one_file`
  kept path-based and reindex-agnostic-by-parameter so that future tool can
  call it directly, as noted in the spec.
- **No placeholders:** every step has complete, real code.
- **Type consistency:** `tag_one_file(path: &Path, client: &TaggerClient, reindex_outdated: bool) -> Result<TagOutcome>` is introduced in Task 3 and used with that exact signature in Task 4 (`backfill.rs`), Task 5 (`watcher.rs`), and transitively via `run_backfill`'s new `reindex_outdated: bool` parameter in Task 6 (`main.rs`). `read_keywords`/`has_keywords` from Task 2 are used unchanged in Tasks 3 and 4's tests. `MARKER_PREFIX`, `phototag_marker()`, `find_phototag_marker()`, `merge_keywords()` are all defined once in Task 3 and not duplicated elsewhere.
