use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::client::TaggerClient;
use crate::config::Config;
use crate::pipeline::tag_one_file;

/// Watches all configured roots and tags new/changed files as they settle.
/// Runs until the event channel closes (i.e. forever, in normal operation) —
/// callers that need to stop it early should abort the task it's spawned in.
pub async fn run_watch(config: Config, client: TaggerClient) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();

    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            let is_relevant = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Modify(notify::event::ModifyKind::Data(_))
                    | EventKind::Modify(notify::event::ModifyKind::Any)
            );
            if !is_relevant {
                return;
            }
            for path in event.paths {
                let _ = tx.send(path);
            }
        })
        .context("creating filesystem watcher")?;

    for root in &config.roots {
        watcher
            .watch(&root.path, RecursiveMode::Recursive)
            .with_context(|| format!("watching root '{}' ({})", root.name, root.path.display()))?;
        tracing::info!(root = %root.name, path = %root.path.display(), "watching");
    }

    if config.roots.is_empty() {
        tracing::warn!("no roots configured, watcher will run but tag nothing");
    }

    let debounce = Duration::from_millis(config.watch.debounce_ms);
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        let sleep_for = pending
            .values()
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));

        tokio::select! {
            maybe_path = rx.recv() => {
                let Some(path) = maybe_path else { break };
                if !config.watch.matches_extension(&path) {
                    continue;
                }
                pending.insert(path, Instant::now() + debounce);
            }
            _ = tokio::time::sleep(sleep_for), if !pending.is_empty() => {
                let now = Instant::now();
                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(path, _)| path.clone())
                    .collect();
                for path in ready {
                    pending.remove(&path);
                    if !path.is_file() {
                        continue;
                    }
                    match tag_one_file(&path, &client, false).await {
                        Ok(outcome) => tracing::info!(path = %path.display(), ?outcome, "processed"),
                        Err(e) => tracing::warn!(path = %path.display(), error = %e, "tagging failed, skipping"),
                    }
                }
            }
        }
    }

    Ok(())
}
