//! PgPanel updater CLI.

use clap::{Parser, Subcommand};
use pgpanel_updater::{TargetArch, Updater, UpdaterResult};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// Blue-green updater for PgPanel.
#[derive(Debug, Parser)]
#[command(name = "pgpanel-updater", version, about)]
struct Cli {
    /// Path to pgpanel.toml configuration.
    #[arg(
        long,
        env = "PGPANEL_CONFIG",
        default_value = "/etc/pgpanel/pgpanel.toml"
    )]
    config: PathBuf,

    /// Target CPU architecture override.
    #[arg(long, value_parser = ["amd64", "arm64", "x86_64", "aarch64"])]
    arch: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check for available updates on GitHub.
    Check,
    /// Download, verify, and deploy an update.
    Update {
        /// Specific version to install (default: latest).
        #[arg(long)]
        version: Option<String>,
        /// Allow installing older versions.
        #[arg(long)]
        allow_downgrade: bool,
    },
    /// Roll back to the previous release.
    Rollback,
    /// Show current deployment status.
    Status,
    /// Run the privileged Unix-socket updater daemon.
    Serve {
        /// Override the configured updater socket path.
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Explicit UID allowed to connect to the socket.
        #[arg(long)]
        allowed_uid: Option<u32>,
        /// Development mode: allow the current UID.
        #[arg(long)]
        dev: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let cli = Cli::parse();
    let arch = match cli.arch.as_deref() {
        Some(a) => TargetArch::parse(a).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        None => TargetArch::detect(),
    };

    let updater = Updater::from_config_path(&cli.config)?;

    match cli.command {
        Commands::Check => run_check(&updater, arch).await?,
        Commands::Update {
            version,
            allow_downgrade,
        } => run_update(&updater, arch, version.as_deref(), allow_downgrade).await?,
        Commands::Rollback => run_rollback(&updater).await?,
        Commands::Status => run_status(&updater)?,
        Commands::Serve {
            socket,
            allowed_uid,
            dev,
        } => run_serve(updater, arch, socket, allowed_uid, dev).await?,
    }

    Ok(())
}

#[cfg(unix)]
async fn run_serve(
    updater: Updater,
    arch: TargetArch,
    socket: Option<PathBuf>,
    allowed_uid: Option<u32>,
    dev: bool,
) -> UpdaterResult<()> {
    if !dev && !nix::unistd::geteuid().is_root() {
        return Err(pgpanel_updater::UpdaterError::Config(
            "serve must run as root unless --dev is used".into(),
        ));
    }
    let socket = socket.unwrap_or_else(|| updater.config().paths.updater_socket.clone());
    let allowed_uid = pgpanel_updater::peer::resolve_allowed_uid(allowed_uid, dev)
        .map_err(pgpanel_updater::UpdaterError::Config)?;
    let daemon = pgpanel_updater::UpdaterDaemon::new(updater, arch)?;
    daemon.serve(&socket, allowed_uid, dev).await
}

#[cfg(not(unix))]
async fn run_serve(
    _updater: Updater,
    _arch: TargetArch,
    _socket: Option<PathBuf>,
    _allowed_uid: Option<u32>,
    _dev: bool,
) -> UpdaterResult<()> {
    Err(pgpanel_updater::UpdaterError::Config(
        "the updater daemon requires Unix sockets".into(),
    ))
}

async fn run_check(updater: &Updater, arch: TargetArch) -> UpdaterResult<()> {
    match updater.check(arch).await? {
        Some(info) => {
            println!("update available: {} (tag {})", info.version, info.tag);
            println!("artifact: {}", info.arch.artifact_name());
        }
        None => println!("no update available"),
    }
    Ok(())
}

async fn run_update(
    updater: &Updater,
    arch: TargetArch,
    version: Option<&str>,
    allow_downgrade: bool,
) -> UpdaterResult<()> {
    let installed = updater.update(arch, version, allow_downgrade).await?;
    println!("update complete: {installed}");
    Ok(())
}

async fn run_rollback(updater: &Updater) -> UpdaterResult<()> {
    let restored = updater.rollback().await?;
    println!("rollback complete: {restored}");
    Ok(())
}

fn run_status(updater: &Updater) -> UpdaterResult<()> {
    let status = updater.status()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&status)
            .map_err(|e| { pgpanel_updater::UpdaterError::Config(e.to_string()) })?
    );
    Ok(())
}
