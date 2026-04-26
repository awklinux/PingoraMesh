use chrono::Utc;
use pingorahub_application::ReleasePlanner;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let preview = ReleasePlanner::build_release_version("bootstrap", Utc::now(), 1);
    info!(release_preview = %preview, "release-worker skeleton started");

    tokio::signal::ctrl_c().await?;
    info!("release-worker stopped");
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pingorahub_release_worker=info,info".into()),
        )
        .try_init();
}
