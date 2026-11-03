//! `lodestone`: the backend binary.
//!
//! ```text
//! lodestone                                  # real Docker + real downloads
//! lodestone --runtime fake --providers fake  # no Docker, no network
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, ValueEnum};
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lodestone=info,tower_http=warn".into()),
        )
        .init();
    let args = Args::parse();
    let data_dir = std::path::absolute(&args.data_dir)?;
    lodestone::run(Config {
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
