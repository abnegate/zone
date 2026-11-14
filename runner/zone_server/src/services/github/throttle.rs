use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use uuid::Uuid;

use super::client::REQUEST_FAILED;
use super::fault::Fault;
use crate::db::sources::SourceRow;

pub const CREDENTIAL_PERMITS: usize = 8;
pub const DIFF_PERMITS: usize = 4;

pub struct Throttle {
    credentials: DashMap<Uuid, Arc<Semaphore>>,
    diffs: Arc<Semaphore>,
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            credentials: DashMap::new(),
            diffs: Arc::new(Semaphore::new(DIFF_PERMITS)),
        }
    }
}

impl Throttle {
    pub fn credential(&self, source: &SourceRow) -> Arc<Semaphore> {
        let key = source.github_installation_id.unwrap_or(source.id);
        Arc::clone(
            &self
                .credentials
                .entry(key)
                .or_insert_with(|| Arc::new(Semaphore::new(CREDENTIAL_PERMITS))),
        )
    }

    pub fn diffs(&self) -> Arc<Semaphore> {
        Arc::clone(&self.diffs)
    }
}

pub async fn admitted(permits: &Semaphore) -> Result<SemaphorePermit<'_>, Fault> {
    permits
        .acquire()
        .await
        .map_err(|_| Fault::upstream(REQUEST_FAILED))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::services::github::testing::connected_source;

    #[test]
    fn sources_of_one_installation_share_its_permits() {
        let throttle = Throttle::default();
        let installation = Uuid::new_v4();
        let mut first = connected_source(Uuid::new_v4(), "first").source;
        let mut second = connected_source(Uuid::new_v4(), "second").source;
        first.github_installation_id = Some(installation);
        second.github_installation_id = Some(installation);
        let token = connected_source(Uuid::new_v4(), "token").source;

        assert!(Arc::ptr_eq(
            &throttle.credential(&first),
            &throttle.credential(&second)
        ));
        assert!(!Arc::ptr_eq(
            &throttle.credential(&first),
            &throttle.credential(&token)
        ));
        assert_eq!(
            throttle.credential(&token).available_permits(),
            CREDENTIAL_PERMITS
        );
        assert_eq!(throttle.diffs().available_permits(), DIFF_PERMITS);
    }
}
