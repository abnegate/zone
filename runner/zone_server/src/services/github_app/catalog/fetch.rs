use std::future::Future;
use std::sync::Arc;

use abnegate_vcs::pull_request::InstalledRepository;
use tokio::sync::OnceCell;

use crate::services::github_app::Error;

#[derive(Default)]
pub(super) struct Fetch(OnceCell<Result<Arc<[InstalledRepository]>, Error>>);

impl Fetch {
    pub(super) async fn fetched<F, Fetching>(
        &self,
        fetch: F,
    ) -> Result<Arc<[InstalledRepository]>, Error>
    where
        F: FnOnce() -> Fetching,
        Fetching: Future<Output = Result<Arc<[InstalledRepository]>, Error>>,
    {
        self.0
            .get_or_init(fetch)
            .await
            .as_ref()
            .map(Arc::clone)
            .map_err(Error::duplicate)
    }
}
