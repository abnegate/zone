use super::connection::ConnectedSource;
use crate::db::sources::SourceRow;

#[derive(Default)]
pub(crate) struct Sources {
    pub(crate) connected: Vec<ConnectedSource>,
    pub(crate) invalid: Vec<SourceRow>,
}
