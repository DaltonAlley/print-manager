use leptos::config::get_configuration;
use tracing_subscriber::EnvFilter;

use crate::{
    config::AppConfig,
    database::Database,
    email::ResendClient,
    repository::Repository,
    storage::Storage,
    web::{WebState, router},
};

/// Builds and serves the application until the process receives a shutdown signal.
///
/// # Errors
///
/// Returns an error when configuration, storage, database, or server startup fails.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let app_config = AppConfig::from_env()?;
    let storage = Storage::new(
        app_config.data_root.clone(),
        app_config.attachment_max_bytes,
    );
    storage.prepare().await?;
    let leptos_options = get_configuration(None)?.leptos_options;
    let resend = match (app_config.resend_api_key, app_config.resend_from) {
        (Some(key), Some(from)) => Some(ResendClient::new(key, from)?),
        _ => None,
    };
    let database = Database::connect(&app_config.database_url).await?;
    database.migrate().await?;
    let repository =
        Repository::with_shop_time_zone(database.pool().clone(), app_config.shop_time_zone);
    repository.ensure_settings().await?;
    let integrity = storage
        .audit(&repository.all_attachment_integrity().await?)
        .await?;
    if !integrity.is_clean() {
        tracing::warn!(missing = ?integrity.missing, corrupt = ?integrity.corrupt, orphaned = ?integrity.orphaned, staging = ?integrity.staging, "Attachment integrity audit found artifacts");
    }
    let app = router(WebState::new(repository, resend, leptos_options, storage));

    let listener = tokio::net::TcpListener::bind(app_config.bind_addr).await?;
    tracing::info!(address = %app_config.bind_addr, "Print Manager listening");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "Failed to install Ctrl-C handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "Failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("Shutdown signal received");
}
