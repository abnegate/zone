//! Where a sweep's work belongs when several servers share one database.

/// Whether a sweep runs once for the whole cluster or once on every replica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Shared state in Postgres or outside zone: only the elected leader runs it.
    Cluster,
    /// State this process alone holds, such as its own connections, gauges or disk.
    Replica,
}
