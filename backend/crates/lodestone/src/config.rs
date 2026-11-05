use std::net::SocketAddr;
use std::ops::RangeInclusive;
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

/// Who keeps servers' SRV records (address names) in DNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsConfig {
    /// Nobody: the UI shows each record for someone to add by hand.
    Manual,
    /// Cloudflare's API, with a token allowed to edit the zone's DNS.
    Cloudflare { api_token: String, zone_id: String },
    /// An in-memory stand-in. For the fake runtime and tests.
    Fake,
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
    /// When set, game ports are allocated only in this range and explicit
    /// ones outside it are refused. RCON stays outside it, and the query port
    /// is off. For hosts whose game ports are forwarded from the internet.
    pub game_ports: Option<RangeInclusive<u16>>,
    /// Docker network the servers join. Lodestone creates it when missing,
    /// with traffic between containers off; see `docs/deploy.md` for the
    /// host firewall rule that keeps servers away from the LAN.
    pub docker_network: Option<String>,
    /// DNS servers for server containers (Docker's default when empty).
    pub container_dns: Vec<String>,
    /// Servers are reachable from the internet: `online-mode` is forced on.
    pub public_servers: bool,
    pub dns: DnsConfig,
}

/// Parses `25565-25599` (or a single port) into a range.
pub fn parse_port_range(raw: &str) -> Result<RangeInclusive<u16>, String> {
    let raw = raw.trim();
    let (start, end) = raw.split_once('-').unwrap_or((raw, raw));
    let parse =
        |part: &str| part.trim().parse::<u16>().map_err(|_| format!("`{raw}` is not a port range like 25565-25599"));
    let (start, end) = (parse(start)?, parse(end)?);
    if start < 1024 || end < start {
        return Err(format!("`{raw}`: the range must start at 1024 or higher and not run backwards"));
    }
    Ok(start..=end)
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
            game_ports: None,
            docker_network: None,
            container_dns: Vec::new(),
            public_servers: false,
            dns: DnsConfig::Fake,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_port_range;

    #[test]
    fn parses_port_ranges() {
        assert_eq!(parse_port_range("25565-25599"), Ok(25565..=25599));
        assert_eq!(parse_port_range(" 30000 "), Ok(30000..=30000));
        assert!(parse_port_range("25599-25565").is_err());
        assert!(parse_port_range("80-90").is_err());
        assert!(parse_port_range("25565-").is_err());
        assert!(parse_port_range("all").is_err());
    }
}
