//! Build-time version metadata.

/// Crate / release version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit hash, injected at build time when available.
pub const GIT_COMMIT: &str = match option_env!("PGPANEL_GIT_COMMIT") {
    Some(v) => v,
    None => "unknown",
};

/// Build timestamp (RFC3339) when set by CI.
pub const BUILD_TIME: &str = match option_env!("PGPANEL_BUILD_TIME") {
    Some(v) => v,
    None => "unknown",
};
