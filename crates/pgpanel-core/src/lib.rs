//! Shared core library for PgPanel.

#![deny(unsafe_code)]
#![warn(clippy::all)]

pub mod audit;
pub mod auth;
pub mod config;
pub mod error;
pub mod permissions;
pub mod pg;
pub mod validation;
pub mod version;

pub use error::{CoreError, CoreResult};
pub use version::{BUILD_TIME, GIT_COMMIT, VERSION};
