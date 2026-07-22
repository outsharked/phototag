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
