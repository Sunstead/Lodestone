//! Lodestone backend library. `lodestone-server` is a thin binary over [`run`].

pub mod api;
pub mod app;
pub mod auth;
pub mod backups;
pub mod cache;
pub mod config;
pub mod content;
pub mod error;
pub mod events;
pub mod host;
pub mod jobs;
pub mod level;
pub mod logparse;
pub mod model;
pub mod paths;
pub mod ports;
pub mod properties;
pub mod runtime;
pub mod software;
pub mod store;
pub mod supervisor;
pub mod util;
pub mod worldfiles;

pub use app::{build_state, router, run, AppState};
pub use config::{Config, ProviderKind, RuntimeKind};

/// Crate version, reported on `/health` and `/system`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
