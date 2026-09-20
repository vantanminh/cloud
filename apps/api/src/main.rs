use std::sync::Arc;

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use knotree_api::{
    app_services, cluster_kubernetes, config::Config, database, html_pages, resources, router,
    state::AppState,
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = Arc::new(Config::from_env()?);
    let db = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .connect(&config.database_url)
        .await?;

    if std::env::args().any(|argument| argument == "migrate") {
        sqlx::migrate!("./migrations").run(&db).await?;
        tracing::info!("database migrations applied");
        return Ok(());
    }

    let bind_addr = config.bind_addr;
    let state = AppState::new(db, config.clone());
    match html_pages::repair_existing_analytics_files(&state).await {
        Ok(repaired_files) if repaired_files > 0 => {
            tracing::info!(
                repaired_files,
                "repaired stored hosted HTML analytics collectors"
            );
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(
            error = %error,
            "could not scan stored hosted HTML analytics collectors for repair"
        ),
    }
    let metrics_sampler = database::spawn_metrics_sampler(state.clone());
    let storage_guard = database::spawn_storage_guard(state.clone());
    let postgres_provisioning_reconciler = resources::spawn_provisioning_reconciler(state.clone());
    let app_resource_reconciler = config
        .uses_kubernetes_workloads()
        .then(|| cluster_kubernetes::spawn_app_resource_reconciler(config.as_ref().clone()));
    let auto_deployer = app_services::spawn_auto_deployer(state.clone());
    let kong_syncer = app_services::spawn_kong_route_syncer(state.clone());
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "knotree api listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    metrics_sampler.abort();
    storage_guard.abort();
    postgres_provisioning_reconciler.abort();
    if let Some(reconciler) = app_resource_reconciler {
        reconciler.abort();
    }
    auto_deployer.abort();
    kong_syncer.abort();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
