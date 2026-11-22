//! SCIM 2.0 provisioning: the bearer secrets identity providers present, and the services the SCIM
//! and console routes share.

pub mod console;
pub mod issued;
pub mod remapped;
pub mod remapping;
pub mod secret;

pub use issued::Issued;
pub use remapped::Remapped;
pub use remapping::Remapping;
