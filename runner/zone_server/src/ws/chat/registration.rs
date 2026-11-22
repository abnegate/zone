use tokio::sync::broadcast;
use uuid::Uuid;

/// A turn's line for being cancelled, and who it runs as and where, so a
/// member who leaves can have every turn they started stopped.
pub struct Registration {
    pub sender: broadcast::Sender<()>,
    pub author: Uuid,
    pub workspace_id: Uuid,
}
