//! Wiring: configuration in, a running HTTP server out.

use std::sync::Arc;

use anyhow::Context;
use axum::Router;

use crate::backups;
use crate::cache::BlobCache;
use crate::config::{Config, ProviderKind, RuntimeKind};
use crate::events::EventBus;
use crate::host::HostSampler;
use crate::jobs::Jobs;
use crate::paths::Paths;
use crate::runtime::fake::FakeRuntime;
use crate::runtime::Runtime;
use crate::software::fake::FakeSoftware;
use crate::software::SoftwareProvider;
use crate::store::Store;
use crate::supervisor::{self, InstanceManager};

/// Everything a request handler or background task needs.
pub struct Services {
    pub config: Config,
    pub paths: Paths,
    pub store: Store,
    pub events: EventBus,
    pub jobs: Jobs,
    pub cache: BlobCache,
    pub software: Arc<dyn SoftwareProvider>,
    pub runtime: Arc<dyn Runtime>,
    pub host: HostSampler,
    pub instances: InstanceManager,
}

pub type AppState = Arc<Services>;

pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let paths = Paths::new(&config.data_dir);
    paths.ensure().with_context(|| format!("creating data dir {}", config.data_dir.display()))?;
    let store = Store::open(&paths.database()).await.context("opening lodestone.db")?;
    store.fail_interrupted_jobs().await?;
    if config.accept_eula {
        let mut settings = store.settings().await?;
        if !settings.eula_accepted {
            settings.eula_accepted = true;
            store.put_settings(&settings).await?;
        }
    }

    let events = EventBus::new();
    let http = match config.providers {
        ProviderKind::Real => Some(
            reqwest::Client::builder()
                .user_agent(concat!(
                    "lodestone/",
                    env!("CARGO_PKG_VERSION"),
                    " (+https://github.com/Sunstead/Lodestone)"
                ))
                .build()?,
        ),
        ProviderKind::Fake => None,
    };
    let software: Arc<dyn SoftwareProvider> = match config.providers {
        ProviderKind::Fake => Arc::new(FakeSoftware),
        ProviderKind::Real => {
            anyhow::bail!("real software providers are not implemented yet; run with --providers fake")
        }
    };
    let runtime: Arc<dyn Runtime> = match config.runtime {
        RuntimeKind::Fake => Arc::new(FakeRuntime::new(config.fake_speed, config.fake_speed > 0.0)),
        RuntimeKind::Docker => anyhow::bail!("the Docker runtime is not implemented yet; run with --runtime fake"),
    };

    Ok(Arc::new(Services {
        jobs: Jobs::new(store.clone(), events.clone()),
        cache: BlobCache::new(paths.blobs(), http),
        host: HostSampler::start(config.data_dir.clone()),
        instances: InstanceManager::default(),
        config,
        paths,
        store,
        events,
        software,
        runtime,
    }))
}

pub fn router(state: AppState) -> Router {
    crate::api::router(state)
}

/// Builds everything, re-attaches to running servers and serves HTTP until Ctrl-C.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let state = build_state(config).await?;
    supervisor::reconcile(&state).await?;
    backups::spawn_scheduler(state.clone());

    let listener = tokio::net::TcpListener::bind(state.config.bind)
        .await
        .with_context(|| format!("binding {}", state.config.bind))?;
    tracing::info!(
        address = %listener.local_addr()?,
        data_dir = %state.config.data_dir.display(),
        runtime = state.runtime.kind(),
        providers = state.software.kind(),
        "Lodestone is listening"
    );
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    // Servers keep running in their containers; Lodestone re-attaches on boot.
    tracing::info!("shutting down");
}
