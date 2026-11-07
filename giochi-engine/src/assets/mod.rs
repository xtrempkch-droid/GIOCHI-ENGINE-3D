//! Asset loading and resource caching.

pub mod loader;
pub mod manager;

pub use loader::{asset_name, asset_size, read_asset, read_asset_vec, MAX_ASSET_SIZE};
pub use manager::ResourceManager;
