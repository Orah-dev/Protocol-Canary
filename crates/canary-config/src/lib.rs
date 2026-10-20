//! Configuration loading and validation for Stellar Protocol Canary.

pub mod loader;
pub mod lockfile;
pub mod schema;

pub use loader::{load, load_from_root, ConfigError, CONFIG_FILE_NAME};
pub use lockfile::{Lockfile, LockfileError, LOCK_FILE_NAME, LOCK_VERSION};
pub use schema::{
    ConfigFile, PolicySection, ProjectSection, ProjectTypeSetting, TestsSection, DEFAULT_PROTOCOL,
    SUPPORTED_CONFIG_VERSION,
};
