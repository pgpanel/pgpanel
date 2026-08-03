//! PgPanel privileged helper daemon.

mod audit;
mod cluster;
mod config_edit;
mod ops;
mod peer;
mod server;

use clap::Parser;
use pgpanel_core::config::Config;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::audit::AuditWriter;
use crate::ops::HelperState;
use crate::peer::resolve_allowed_uid;

/// Privileged helper for PostgreSQL cluster management.
#[derive(Debug, Parser)]
#[command(name = "pgpanel-helper", about = "PgPanel privileged helper daemon")]
struct Cli {
    /// Path to pgpanel.toml configuration file.
    #[arg(long, default_value = "/etc/pgpanel/pgpanel.toml")]
    config: PathBuf,

    /// Override helper Unix socket path.
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Use built-in development defaults instead of loading config from disk.
    #[arg(long)]
    dev: bool,

    /// UID allowed to connect (defaults to pgpanel user or current UID in --dev).
    #[arg(long)]
    allowed_uid: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let config = if cli.dev {
        Config::dev_default()
    } else {
        Config::load(&cli.config).map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?
    };

    init_tracing(&config);

    let allowed_uid = resolve_allowed_uid(cli.allowed_uid, cli.dev)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;

    let socket_path = cli
        .socket
        .unwrap_or_else(|| config.paths.helper_socket.clone());

    let audit = AuditWriter::from_config(&config);
    let state = Arc::new(HelperState::new(config, allowed_uid, cli.dev, audit));

    info!(
        version = pgpanel_core::VERSION,
        git = pgpanel_core::GIT_COMMIT,
        allowed_uid,
        dev = cli.dev,
        socket = %socket_path.display(),
        "starting pgpanel-helper"
    );

    if let Err(e) = server::run_server(state, &socket_path).await {
        tracing::error!(error = %e, "helper server exited with error");
        return Err(Box::new(e) as Box<dyn std::error::Error>);
    }

    Ok(())
}

fn init_tracing(config: &Config) {
    let filter = EnvFilter::try_new(&config.logging.level)
        .unwrap_or_else(|_| EnvFilter::new("info,pgpanel_helper=debug"));

    if config.logging.json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_target(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .init();
    }
}
