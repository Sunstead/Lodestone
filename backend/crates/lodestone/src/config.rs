use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Which container runtime runs servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    /// The local Docker daemon (`DOCKER_HOST` is honoured).
    Docker,
    /// In-process simulation: no Docker, no Java. For dev and tests.
    Fake,
}

/// Where server software and mods come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Mojang, PaperMC, Fabric, Forge, NeoForge, Modrinth.
    Real,
    /// Small built-in stand-ins. No network.
    Fake,
}

/// Sign-in through an OpenID Connect provider (Authentik). The client is
/// public: Lodestone holds no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcConfig {
    /// e.g. `https://auth.example.com/application/o/lodestone/`.
    pub issuer: String,
    pub client_id: String,
    pub scopes: String,
    /// When set, only members of these groups may sign in.
    pub allowed_groups: Vec<String>,
    /// Members of these groups manage Lodestone (settings, everyone's assets).
    pub admin_groups: Vec<String>,
    /// When Lodestone must reach the provider at a different address.
    pub discovery_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthConfig {
    Oidc(OidcConfig),
    /// Every request is this user, an admin. Local development and tests.
    Dev {
        username: String,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub auth: AuthConfig,
    /// Where browsers reach Lodestone, e.g. `https://lodestone.example.com`.
    /// Needed for web sign-in (the redirect URI) and to check `Origin`.
    pub public_url: Option<url::Url>,
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    /// CurseForge API key from the environment; Settings can set one too.
    pub curseforge_api_key: Option<String>,
    pub runtime: RuntimeKind,
    pub providers: ProviderKind,
    /// Serve a built frontend from this directory at `/`.
    pub static_dir: Option<PathBuf>,
    /// Pre-accept the Minecraft EULA on boot. Only for fake/dev setups.
    pub accept_eula: bool,
    /// How long a graceful stop may take before the container is killed.
    pub stop_timeout: Duration,
    /// Multiplier for the fake runtime's simulated delays (0 = instant).
    pub fake_speed: f64,
    /// When set, data paths handed to Docker are rewritten from `data_dir`
    /// to this host path (Lodestone itself running in a container).
    pub host_data_dir: Option<PathBuf>,
}

impl Config {
    /// A config for tests and `--runtime fake`: everything in `data_dir`, no network.
    pub fn fake(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            auth: AuthConfig::Dev { username: "dev".into() },
            public_url: None,
            data_dir: data_dir.into(),
            bind: "127.0.0.1:0".parse().expect("valid addr"),
            curseforge_api_key: None,
            runtime: RuntimeKind::Fake,
            providers: ProviderKind::Fake,
            static_dir: None,
            accept_eula: true,
            stop_timeout: Duration::from_secs(5),
            fake_speed: 0.0,
            host_data_dir: None,
        }
    }
}
