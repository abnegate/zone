//! Zone client HTTP shell — frontend serving and API proxy.

pub mod client;
pub mod frontend;
pub mod proxy;
pub mod serve;

mod setup;

pub use client::{ClientPlatform, ManagerDirInputs, config_path, resolve_manager_dir};
pub use frontend::{AppMode, FrontendKind};
pub use serve::{ServeKind, bind, router};
