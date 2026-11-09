use super::UriNamespace;

/// The index records a source keeps after a fetch; every other record of the
/// source is dropped
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retention {
    /// Exactly these URIs
    Exact(Vec<String>),
    /// Every URI in the namespace, since the listing was cut short and a file
    /// it left out cannot be told from one deleted
    Namespace(UriNamespace),
}
