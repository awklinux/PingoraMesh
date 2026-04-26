use pingorahub_dns_provider::{ProviderConfig, build_provider};
use pingorahub_dns_worker::{
    PostgresCertificateOrderStore, build_acme_client, process_pending_certificate_orders,
    process_presented_certificate_orders,
};
use serde_json::json;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    if let Ok(postgres_url) = std::env::var("PINGORAHUB_POSTGRES_URL") {
        let max_connections = std::env::var("PINGORAHUB_POSTGRES_MAX_CONNECTIONS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(5);
        let acme_mode =
            std::env::var("PINGORAHUB_ACME_MODE").unwrap_or_else(|_| "mock".to_string());
        let batch_size = std::env::var("PINGORAHUB_DNS_WORKER_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(10);
        let interval_seconds = std::env::var("PINGORAHUB_DNS_WORKER_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15);
        let renew_before_days = std::env::var("PINGORAHUB_CERT_AUTO_RENEW_BEFORE_DAYS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(30);
        let renew_acme_provider = std::env::var("PINGORAHUB_CERT_AUTO_RENEW_ACME_PROVIDER")
            .unwrap_or_else(|_| acme_mode.clone());
        let run_once = std::env::var("PINGORAHUB_DNS_WORKER_ONCE")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        let store = PostgresCertificateOrderStore::connect(&postgres_url, max_connections).await?;
        let acme_client = build_acme_client(&acme_mode)?;
        info!(
            batch_size,
            interval_seconds, run_once, acme_mode, "dns-worker started in certificate order mode"
        );

        loop {
            let renewal_orders = store
                .enqueue_due_renewal_orders(
                    chrono::Utc::now(),
                    chrono::Duration::days(renew_before_days.max(1)),
                    batch_size,
                    &renew_acme_provider,
                )
                .await?;
            let dns_presented =
                process_pending_certificate_orders(&store, acme_client.as_ref(), batch_size)
                    .await?;
            let issued =
                process_presented_certificate_orders(&store, acme_client.as_ref(), batch_size)
                    .await?;
            info!(
                dns_presented,
                issued, renewal_orders, "dns-worker processed certificate dns orders"
            );

            if run_once {
                break;
            }

            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(interval_seconds)) => {}
            }
        }

        info!("dns-worker stopped");
        return Ok(());
    }

    let provider_type =
        std::env::var("PINGORAHUB_DNS_PROVIDER_TYPE").unwrap_or_else(|_| "noop".to_string());
    let api_endpoint = std::env::var("PINGORAHUB_DNS_API_ENDPOINT").ok();
    let api_token = std::env::var("PINGORAHUB_DNS_API_TOKEN").ok();
    let zone_name = std::env::var("PINGORAHUB_DNS_ZONE_NAME").ok();

    let provider = build_provider(&ProviderConfig {
        provider_type,
        api_endpoint,
        credentials: json!({
            "api_token": api_token.unwrap_or_default()
        }),
    })?;
    let metadata = provider.metadata();
    info!(
        provider = %metadata.provider_type,
        dns01 = metadata.supports_dns01,
        "dns-worker started"
    );

    if let Some(zone_name) = zone_name {
        let zones = provider.list_zones(&[zone_name]).await?;
        info!(count = zones.len(), "dns-worker listed zones");
    }

    tokio::signal::ctrl_c().await?;
    info!("dns-worker stopped");
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pingorahub_dns_worker=info,info".into()),
        )
        .try_init();
}
