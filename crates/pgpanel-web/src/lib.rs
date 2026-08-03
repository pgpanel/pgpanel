//! PgPanel web administration interface.

#![deny(unsafe_code)]
#![warn(clippy::all)]

pub mod crypto;
pub mod db;
pub mod error;
pub mod helper_client;
pub mod middleware;
pub mod routes;
pub mod services;
pub mod state;
pub mod templates_data;

pub use error::{AppError, AppResult};
pub use state::AppState;
