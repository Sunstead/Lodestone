//! `lodestone`: the backend binary.
//!
//! ```text
//! lodestone --oidc-issuer https://auth.example.com/application/o/lodestone/ \
//!           --public-url https://lodestone.example.com      # real Docker + downloads
//! lodestone --dev-user dev --runtime fake --providers fake  # no Docker, no network
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use lodestone::config::{AuthConfig, OidcConfig};
use lodestone::{Config, ProviderKind, RuntimeKind};

#[derive(Copy, Clone, Debug, ValueEnum)]
enum RuntimeArg {
    Docker,
    Fake,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ProvidersArg {
    Real,
    Fake,
}

#[derive(Parser, Debug)]
#[command(name = "lodestone", version, about = "Run Minecraft servers as worlds, configurations and instances")]
struct Args {
    /// Where worlds, profiles, backups, the download cache and the database live.
    #[arg(long, env = "LODESTONE_DATA", default_value = "./.lodestone-dev")]
    data_dir: PathBuf,

    /// Address for the HTTP API (and the UI when --static-dir is set).
    #[arg(long, env = "LODESTONE_BIND", default_value = "127.0.0.1:7878")]
    bind: SocketAddr,

    /// Where servers run.
    #[arg(long, env = "LODESTONE_RUNTIME", value_enum, default_value = "docker")]
    runtime: RuntimeArg,

    /// Where server software and mods come from.
    #[arg(long, env = "LODESTONE_PROVIDERS", value_enum, default_value = "real")]
    providers: ProvidersArg,

    /// Serve a built frontend (frontend/dist) at `/`.
    #[arg(long, env = "LODESTONE_STATIC_DIR")]
    static_dir: Option<PathBuf>,

    /// Pre-accept the Minecraft EULA (https://aka.ms/MinecraftEULA).
    /// Meant for the fake runtime and automated tests.
    #[arg(long, env = "LODESTONE_ACCEPT_EULA")]
    accept_eula: bool,

    /// Seconds a server gets to save and exit before it is killed.
    #[arg(long, default_value_t = 60)]
    stop_timeout: u64,

    /// Fake runtime: multiplier for simulated delays (0 = instant).
    #[arg(long, default_value_t = 1.0)]
    fake_speed: f64,

    /// When Lodestone runs in a container: the data dir's path on the Docker host.
    #[arg(long, env = "LODESTONE_HOST_DATA_DIR")]
    host_data_dir: Option<PathBuf>,

    /// Sign-in: the OpenID Connect issuer (Authentik), e.g.
    /// https://auth.example.com/application/o/lodestone/
    #[arg(long, env = "LODESTONE_OIDC_ISSUER")]
    oidc_issuer: Option<String>,

    /// The OIDC client id (a public client: no secret).
    #[arg(long, env = "LODESTONE_OIDC_CLIENT_ID", default_value = "lodestone")]
    oidc_client_id: String,

    #[arg(long, env = "LODESTONE_OIDC_SCOPES", default_value = "openid profile email")]
    oidc_scopes: String,

    /// Comma-separated groups allowed to sign in (empty: anyone the provider lets in).
    #[arg(long, env = "LODESTONE_OIDC_ALLOWED_GROUPS", value_delimiter = ',')]
    oidc_allowed_groups: Vec<String>,

    /// Comma-separated groups whose members manage Lodestone.
    #[arg(long, env = "LODESTONE_OIDC_ADMIN_GROUPS", value_delimiter = ',')]
    oidc_admin_groups: Vec<String>,

    /// When Lodestone must reach the provider at another address.
    #[arg(long, env = "LODESTONE_OIDC_DISCOVERY_URL")]
    oidc_discovery_url: Option<String>,

    /// Where browsers reach Lodestone (the sign-in redirect comes back here).
    #[arg(long, env = "LODESTONE_PUBLIC_URL")]
    public_url: Option<String>,

    /// CurseForge API key (console.curseforge.com). Admins can also set it in Settings.
    #[arg(long, env = "LODESTONE_CURSEFORGE_API_KEY", hide_env_values = true)]
    curseforge_api_key: Option<String>,

    /// No sign-in: every request is this user, an admin. Development only.
    #[arg(long, env = "LODESTONE_DEV_USER", conflicts_with = "oidc_issuer")]
    dev_user: Option<String>,
}

fn groups(list: Vec<String>) -> Vec<String> {
    list.into_iter().map(|g| g.trim().to_owned()).filter(|g| !g.is_empty()).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        // Plain text under Docker; colours only on a terminal.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lodestone=info,tower_http=warn".into()),
        )
        .init();
    let args = Args::parse();
    let data_dir = std::path::absolute(&args.data_dir)?;
    let auth = match (args.oidc_issuer, args.dev_user) {
        (Some(issuer), _) => {
            url::Url::parse(&issuer).map_err(|err| anyhow::anyhow!("--oidc-issuer {issuer}: {err}"))?;
            AuthConfig::Oidc(OidcConfig {
                issuer,
                client_id: args.oidc_client_id,
                scopes: args.oidc_scopes,
                allowed_groups: groups(args.oidc_allowed_groups),
                admin_groups: groups(args.oidc_admin_groups),
                discovery_url: args.oidc_discovery_url,
            })
        }
        (None, Some(username)) => {
            tracing::warn!(%username, "no sign-in: every request is the dev user (--dev-user is for development)");
            AuthConfig::Dev { username }
        }
        (None, None) => anyhow::bail!(
            "Lodestone needs sign-in. Set --oidc-issuer (LODESTONE_OIDC_ISSUER) to your Authentik application, \
             or --dev-user <name> for local development."
        ),
    };
    let public_url = match args.public_url {
        Some(raw) => {
            let url = url::Url::parse(&raw).map_err(|err| anyhow::anyhow!("--public-url {raw}: {err}"))?;
            anyhow::ensure!(matches!(url.scheme(), "http" | "https"), "--public-url must be http(s)");
            Some(url)
        }
        None => None,
    };
    if matches!(auth, AuthConfig::Oidc(_)) && public_url.is_none() {
        tracing::warn!("no --public-url: browsers can't sign in (bearer tokens still work)");
    }
    lodestone::run(Config {
        auth,
        public_url,
        curseforge_api_key: args.curseforge_api_key,
        data_dir,
        bind: args.bind,
        runtime: match args.runtime {
            RuntimeArg::Docker => RuntimeKind::Docker,
            RuntimeArg::Fake => RuntimeKind::Fake,
        },
        providers: match args.providers {
            ProvidersArg::Real => ProviderKind::Real,
            ProvidersArg::Fake => ProviderKind::Fake,
        },
        static_dir: args.static_dir,
        accept_eula: args.accept_eula,
        stop_timeout: Duration::from_secs(args.stop_timeout),
        fake_speed: args.fake_speed,
        host_data_dir: args.host_data_dir,
    })
    .await
}
