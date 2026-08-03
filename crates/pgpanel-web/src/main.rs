//! PgPanel web server entry point.

use clap::Parser;
use pgpanel_core::config::Config;
use pgpanel_web::{db, routes, state::AppState};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "pgpanel-web", about = "PgPanel web administration interface")]
struct Cli {
    /// Path to configuration file.
    #[arg(short, long, env = "PGPANEL_CONFIG")]
    config: Option<PathBuf>,

    /// Bind address override.
    #[arg(long, env = "PGPANEL_LISTEN")]
    listen: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_path = cli
        .config
        .unwrap_or_else(|| PathBuf::from("./config/pgpanel.dev.toml"));

    let mut config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        tracing::warn!(
            "config not found at {}, using dev defaults",
            config_path.display()
        );
        Config::dev_default()
    };

    if let Some(listen) = cli.listen {
        config.server.listen = listen.parse()?;
    }

    init_tracing(&config);

    tracing::info!(
        version = pgpanel_core::version::VERSION,
        listen = %config.server.listen,
        "starting pgpanel-web"
    );

    let pool = db::init_pool(&config).await?;
    let state = AppState::new(config.clone(), pool)?;
    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
    tracing::info!(addr = %config.server.listen, "listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

fn init_tracing(config: &Config) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    if config.logging.json {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}
