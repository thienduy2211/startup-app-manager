//! Config cua app: kieu du lieu, doc/ghi TOML, gom env vars.

pub mod env;
pub mod model;
pub mod store;

pub use model::{AppConfig, HealthCheck, ManagedApp, RestartPolicy, Settings};
