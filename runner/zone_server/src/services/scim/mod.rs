//! SCIM 2.0 provisioning: the bearer secrets identity providers present, and the services the SCIM
//! and console routes share.

pub mod client;
pub mod entitlement;
pub mod failure;
pub mod groups;
pub mod limit;
pub mod password;
pub mod record;
pub mod resource;
pub mod secret;
pub mod slot;
pub mod users;
pub mod writer;

pub use client::ScimClient;
pub use entitlement::Entitlement;
pub use failure::Failure;
pub use slot::Slot;
pub use writer::ScimWriter;
