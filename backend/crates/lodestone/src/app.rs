//! Wiring: configuration in, a running HTTP server out.

use std::sync::Arc;

use anyhow::Context;
use axum::Router;

use crate::auth::web::CookieSpec;
use crate::auth::{self, Auth};
use crate::backups;
use crate::cache::BlobCache;
use crate::config::{Config, ProviderKind, RuntimeKind};
use crate::content::curseforge::{self, ApiKey, CurseForge};
use crate::content::fake::FakeContent;
use crate::content::hangar::{self, Hangar};
use crate::content::modrinth::{self, Modrinth};
use crate::content::{ContentProvider, Providers};
use crate::events::EventBus;
use crate::host::HostSampler;
use crate::jobs::Jobs;
use crate::model::ContentSource;
use crate::paths::Paths;
use crate::runtime::docker::{DockerOptions, DockerRuntime};
use crate::runtime::fake::FakeRuntime;
use crate::runtime::Runtime;
use crate::software::fake::FakeSoftware;
use crate::software::real::{Endpoints, RealSoftware};
use crate::software::SoftwareProvider;
use crate::store::Store;
use crate::supervisor::{self, InstanceManager};

/// Where Settings keeps the CurseForge API key.
pub const CURSEFORGE_KEY: &str = "curseforge-api-key";

/// Everything a request handler or background task needs.
pub struct Services {
    pub config: Config,
    pub auth: Arc<Auth>,
    pub cookie: CookieSpec,
    /// Mod and plugin sources.
    pub content: Providers,
    /// CurseForge's API key (environment or Settings).
    pub curseforge_key: ApiKey,
    /// Serialises changes to profiles' lockfiles.
    pub content_lock: tokio::sync::Mutex<()>,
    /// Serialises Forge/NeoForge installs.
    pub install_lock: tokio::sync::Mutex<()>,
    pub paths: Paths,
    pub store: Store,
    pub events: EventBus,
    pub jobs: Jobs,
    pub cache: BlobCache,
    pub software: Arc<dyn SoftwareProvider>,
    pub runtime: Arc<dyn Runtime>,
    pub host: HostSampler,
    pub instances: InstanceManager,
    /// Servers' address names (SRV records).
    pub dns: crate::dns::Dns,
}

pub type AppState = Arc<Services>;

pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let paths = Paths::new(&config.data_dir);
    paths.ensure().with_context(|| format!("creating data dir {}", config.data_dir.display()))?;
    let store = Store::open(&paths.database()).await.context("opening lodestone.db")?;
    store.fail_interrupted_jobs().await?;

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
        ProviderKind::Real => Arc::new(RealSoftware::new(
            http.clone().expect("real providers have an HTTP client"),
            store.clone(),
            Endpoints::default(),
        )),
    };
    let stored_key = store.get_value(CURSEFORGE_KEY).await?;
    let curseforge_key = ApiKey::new(config.curseforge_api_key.clone().or(stored_key));
    let content: Vec<Arc<dyn ContentProvider>> = match config.providers {
        ProviderKind::Fake => vec![
            Arc::new(FakeContent::new(ContentSource::Modrinth, None)),
            Arc::new(FakeContent::new(ContentSource::Hangar, None)),
            Arc::new(FakeContent::new(ContentSource::Curseforge, Some(curseforge_key.clone()))),
        ],
        ProviderKind::Real => {
            let http = http.clone().expect("real providers have an HTTP client");
            vec![
                Arc::new(Modrinth::new(http.clone(), modrinth::API)),
                Arc::new(Hangar::new(http.clone(), hangar::API)),
                Arc::new(CurseForge::new(http, curseforge::API, curseforge_key.clone())),
            ]
        }
    };
    let auth = Arc::new(Auth::new(&config.auth));
    auth.start();
    if let Auth::Dev { username } = &*auth {
        // Assets from before sign-in existed belong to the dev user.
        let user = auth::dev_user(&store, username).await?;
        store.claim_orphans(&user.id).await?;
    }
    let runtime: Arc<dyn Runtime> = match config.runtime {
        RuntimeKind::Fake => Arc::new(FakeRuntime::new(config.fake_speed, config.fake_speed > 0.0)),
        RuntimeKind::Docker => Arc::new(DockerRuntime::connect(
            &config.data_dir,
            config.host_data_dir.clone(),
            DockerOptions { network: config.docker_network.clone(), dns: config.container_dns.clone() },
        )),
    };

    // Cloudflare is reached even with fake software providers.
    let dns_http = http.clone().unwrap_or_default();
    let state = Arc::new(Services {
        cookie: CookieSpec::for_url(config.public_url.as_ref()),
        auth,
        content: Providers::new(content),
        curseforge_key,
        content_lock: tokio::sync::Mutex::new(()),
        install_lock: tokio::sync::Mutex::new(()),
        jobs: Jobs::new(store.clone(), events.clone()),
        cache: BlobCache::new(paths.blobs(), http),
        host: HostSampler::start(config.data_dir.clone()),
        instances: InstanceManager::default(),
        dns: crate::dns::Dns::new(&config.dns, dns_http),
        config,
        paths,
        store,
        events,
        software,
        runtime,
    });
    crate::dns::spawn(Arc::downgrade(&state));
    Ok(state)
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
