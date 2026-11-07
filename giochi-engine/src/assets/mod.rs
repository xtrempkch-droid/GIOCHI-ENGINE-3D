//! Asset loading and resource caching.

pub mod loader;
pub mod manager;

pub use loader::{asset_size, read_asset, MAX_ASSET_SIZE};
pub use manager::ResourceManager;
