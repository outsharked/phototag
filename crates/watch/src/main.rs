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
