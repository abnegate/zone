//! Everything a section needs to decide what it renders.

use super::{Environment, Surface};
use crate::agent::ChatTools;

/// The inputs every section reads, assembled once per prompt.
///
/// Rendering depends only on these four, which is what makes each section
/// testable without a database, a workspace or a live tool registry.
pub(crate) struct Context<'a> {
    pub surface: Surface,
    pub auto_approve: bool,
    pub environment: &'a Environment,
    pub tools: &'a ChatTools,
}
