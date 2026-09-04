//! Zone installer library — frontend serving, API proxy, and compose install.

pub mod frontend;
pub mod proxy;
pub mod serve;

mod install;
mod setup;

pub use frontend::{AppMode, FrontendKind};
pub use serve::{ServeKind, bind, router};
