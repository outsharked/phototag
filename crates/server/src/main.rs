use clap::Parser;
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = ServerConfig::parse();
    let listen_addr = config.listen_addr.clone();

    let state = create_app_state(config).await?;
    let app = build_router(state);

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("phototag-server listening on {listen_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
