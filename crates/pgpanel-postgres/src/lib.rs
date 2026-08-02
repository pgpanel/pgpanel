//! PostgreSQL database/role management and read-only browser/SQL console.
#![forbid(unsafe_code)]

mod browser;
mod client;
mod roles;
mod sql_console;

pub use browser::BrowserService;
pub use client::PgClient;
pub use roles::RoleService;
pub use sql_console::{SqlConsole, SqlGuard};
