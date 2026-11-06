//! The repositories each installation reached when GitHub was last asked.

mod entry;

use std::collections::HashMap;
use std::fmt;
use std::num::{NonZeroU8, NonZeroU32};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{ApplicationError, ApplicationService, InstalledRepository};
use sqlx::PgPool;
use uuid::Uuid;

use entry::Entry;

use super::error::Error;
use super::provider::Provider;

pub const TTL: Duration = Duration::from_secs(60);

const PAGE_SIZE: NonZeroU8 = NonZeroU8::new(100).unwrap();

#[derive(Default)]
pub struct Catalog(Mutex<HashMap<Uuid, Entry>>);

impl Catalog {
    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(
        &self,
        organization: Uuid,
        installation: Uuid,
        now: Instant,
    ) -> Option<Arc<[InstalledRepository]>> {
        self.lock()
            .get(&installation)
            .filter(|entry| entry.serves(organization, now))
            .map(|entry| Arc::clone(&entry.repositories))
    }

    pub fn insert(
        &self,
        organization: Uuid,
        installation: Uuid,
        repositories: Arc<[InstalledRepository]>,
        now: Instant,
    ) {
        let mut entries = self.lock();
        entries.retain(|_, entry| entry.fresh(now));
        entries.insert(
            installation,
            Entry {
                organization,
                repositories,
                expires_at: now + TTL,
            },
        );
    }

    pub fn forget(&self, installation: Uuid) {
        self.lock().remove(&installation);
    }

    /// Every repository `installation` of `organization_id` reaches: the
    /// cached list while it is fresh and `refresh` is not asked for, otherwise
    /// every page GitHub lists now. A failure is never cached.
    pub async fn repositories(
        &self,
        provider: &Provider,
        db: &PgPool,
        organization_id: Uuid,
        installation: Uuid,
        refresh: bool,
    ) -> Result<Arc<[InstalledRepository]>, Error> {
        if !refresh && let Some(cached) = self.get(organization_id, installation, Instant::now()) {
            return Ok(cached);
        }

        let credential = provider
            .organization_installation(db, organization_id, installation)
            .await?;
        let applications = provider.applications()?;
        let repositories: Arc<[InstalledRepository]> = provider
            .retrying(db, credential, |token| Self::listed(applications, token))
            .await?
            .into();
        self.insert(
            organization_id,
            installation,
            Arc::clone(&repositories),
            Instant::now(),
        );

        Ok(repositories)
    }

    async fn listed(
        applications: &ApplicationService,
        token: SecretValue,
    ) -> Result<Vec<InstalledRepository>, ApplicationError> {
        let mut repositories = Vec::new();
        let mut next = Some(NonZeroU32::MIN);
        while let Some(page) = next {
            let listed = applications
                .list_installation_repositories(&token, page, PAGE_SIZE)
                .await?;
            repositories.extend(listed.items);
            next = listed.next;
        }

        Ok(repositories)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("installations", &self.lock().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use abnegate_vcs::{RemoteBranchName, Repository};
    use serde_json::{Value, json};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate, Times};

    use super::*;
    use crate::db::github_apps::testing::{pool, seed_organization};
    use crate::db::github_installations;
    use crate::services::github_app::testing::{installation_number, installed, issuing, provider};

    const TOKEN: &str = "ghs_catalogtoken";
    const ORGANIZATION: Uuid = Uuid::from_u128(1);

    fn repositories(names: &[&str]) -> Arc<[InstalledRepository]> {
        names
            .iter()
            .zip(1..)
            .map(|(name, id)| {
                InstalledRepository::new(
                    id,
                    Repository::new("acme", name).expect("repository"),
                    RemoteBranchName::parse("main").expect("branch"),
                    format!("https://github.com/acme/{name}"),
                )
            })
            .collect()
    }

    fn names(repositories: &[InstalledRepository]) -> Vec<&str> {
        repositories.iter().map(InstalledRepository::name).collect()
    }

    #[test]
    fn a_fresh_entry_is_served() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(
            ORGANIZATION,
            installation,
            repositories(&["widgets", "gadgets"]),
            now,
        );

        let served = catalog
            .get(
                ORGANIZATION,
                installation,
                now + TTL - Duration::from_millis(1),
            )
            .expect("the entry is still fresh");

        assert_eq!(names(&served), ["widgets", "gadgets"]);
    }

    #[test]
    fn an_entry_expires_after_the_ttl() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(ORGANIZATION, installation, repositories(&["widgets"]), now);

        assert!(catalog.get(ORGANIZATION, installation, now + TTL).is_none());
    }

    #[test]
    fn an_installation_never_listed_misses() {
        let catalog = Catalog::default();
        catalog.insert(
            ORGANIZATION,
            Uuid::new_v4(),
            repositories(&["widgets"]),
            Instant::now(),
        );

        assert!(
            catalog
                .get(ORGANIZATION, Uuid::new_v4(), Instant::now())
                .is_none()
        );
    }

    #[test]
    fn a_forgotten_installation_misses_and_spares_the_others() {
        let catalog = Catalog::default();
        let forgotten = Uuid::new_v4();
        let kept = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(ORGANIZATION, forgotten, repositories(&["widgets"]), now);
        catalog.insert(ORGANIZATION, kept, repositories(&["gadgets"]), now);

        catalog.forget(forgotten);
        catalog.forget(Uuid::new_v4());

        assert!(catalog.get(ORGANIZATION, forgotten, now).is_none());
        assert_eq!(
            names(&catalog.get(ORGANIZATION, kept, now).expect("kept")),
            ["gadgets"]
        );
    }

    #[test]
    fn inserting_prunes_expired_entries() {
        let catalog = Catalog::default();
        let stale = Uuid::new_v4();
        let live = Uuid::new_v4();
        let fresh = Uuid::new_v4();
        let start = Instant::now();
        catalog.insert(ORGANIZATION, stale, repositories(&["old"]), start);
        catalog.insert(
            ORGANIZATION,
            live,
            repositories(&["recent"]),
            start + TTL / 2,
        );

        let later = start + TTL;
        catalog.insert(ORGANIZATION, fresh, repositories(&["new"]), later);

        assert_eq!(catalog.len(), 2);
        assert!(catalog.get(ORGANIZATION, stale, later).is_none());
        assert!(catalog.get(ORGANIZATION, live, later).is_some());
        assert!(catalog.get(ORGANIZATION, fresh, later).is_some());
    }

    #[test]
    fn reinserting_replaces_and_renews_an_entry() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let start = Instant::now();
        catalog.insert(
            ORGANIZATION,
            installation,
            repositories(&["widgets"]),
            start,
        );

        let later = start + TTL / 2;
        catalog.insert(
            ORGANIZATION,
            installation,
            repositories(&["gadgets"]),
            later,
        );

        let served = catalog
            .get(ORGANIZATION, installation, start + TTL)
            .expect("the renewed entry outlives the first");
        assert_eq!(names(&served), ["gadgets"]);
    }

    #[test]
    fn another_organization_misses_a_cached_listing() {
        let catalog = Catalog::default();
        let installation = Uuid::new_v4();
        let now = Instant::now();
        catalog.insert(ORGANIZATION, installation, repositories(&["widgets"]), now);

        assert!(catalog.get(Uuid::new_v4(), installation, now).is_none());
        assert!(catalog.get(ORGANIZATION, installation, now).is_some());
    }

    #[test]
    fn debug_counts_installations_without_listing_repositories() {
        let catalog = Catalog::default();
        catalog.insert(
            ORGANIZATION,
            Uuid::new_v4(),
            repositories(&["widgets"]),
            Instant::now(),
        );

        let rendered = format!("{catalog:?}");

        assert!(rendered.contains("installations: 1"), "{rendered}");
        assert!(!rendered.contains("widgets"), "{rendered}");
    }

    fn key() -> [u8; 32] {
        static KEY: LazyLock<[u8; 32]> = LazyLock::new(rand::random);
        *KEY
    }

    fn listed(id: u64, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "full_name": format!("acme/{name}"),
            "owner": { "login": "acme" },
            "private": false,
            "archived": false,
            "default_branch": "main",
            "html_url": format!("https://github.com/acme/{name}"),
        })
    }

    fn page_of(rows: &[Value]) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "total_count": rows.len(),
            "repositories": rows,
        }))
    }

    async fn listing(
        server: &MockServer,
        page: u32,
        answer: ResponseTemplate,
        times: impl Into<Times>,
    ) {
        Mock::given(method("GET"))
            .and(path("/installation/repositories"))
            .and(query_param("page", page.to_string()))
            .and(query_param("per_page", PAGE_SIZE.to_string()))
            .and(header("authorization", format!("Bearer {TOKEN}")))
            .respond_with(answer)
            .expect(times)
            .mount(server)
            .await;
    }

    struct Fetching {
        server: MockServer,
        pool: PgPool,
        provider: Provider,
        organization: Uuid,
        installation: Uuid,
    }

    impl Fetching {
        async fn new(mints: u64) -> Self {
            let server = MockServer::start().await;
            let pool = pool().await;
            let provider = provider(&server, key());
            let organization = seed_organization(&pool).await;
            let number = installation_number();
            let installation = installed(&pool, &provider, organization, number).await;
            issuing(&server, number, TOKEN, mints).await;
            Self {
                server,
                pool,
                provider,
                organization,
                installation,
            }
        }

        async fn fetch(&self, refresh: bool) -> Result<Arc<[InstalledRepository]>, Error> {
            self.provider
                .catalog()
                .repositories(
                    &self.provider,
                    &self.pool,
                    self.organization,
                    self.installation,
                    refresh,
                )
                .await
        }
    }

    #[tokio::test]
    async fn a_fresh_listing_is_served_without_asking_github_again() {
        let fetching = Fetching::new(1).await;
        listing(&fetching.server, 1, page_of(&[listed(1, "widgets")]), 1).await;

        let fetched = fetching.fetch(false).await.expect("listed");
        let cached = fetching.fetch(false).await.expect("cached");

        assert_eq!(names(&fetched), ["widgets"]);
        assert!(
            Arc::ptr_eq(&fetched, &cached),
            "the second answer is the cached one"
        );
    }

    #[tokio::test]
    async fn a_refresh_lists_again_and_replaces_the_cached_listing() {
        let fetching = Fetching::new(1).await;
        listing(&fetching.server, 1, page_of(&[listed(1, "widgets")]), 2).await;

        let fetched = fetching.fetch(false).await.expect("listed");
        let refreshed = fetching.fetch(true).await.expect("listed again");

        assert!(!Arc::ptr_eq(&fetched, &refreshed));
        let cached = fetching
            .provider
            .catalog()
            .get(fetching.organization, fetching.installation, Instant::now())
            .expect("the refreshed listing is cached");
        assert!(Arc::ptr_eq(&refreshed, &cached));
    }

    #[tokio::test]
    async fn every_page_is_followed() {
        let fetching = Fetching::new(1).await;
        let full: Vec<Value> = (1..=100)
            .map(|id| listed(id, &format!("repository-{id}")))
            .collect();
        listing(&fetching.server, 1, page_of(&full), 1).await;
        listing(&fetching.server, 2, page_of(&[listed(101, "last")]), 1).await;

        let fetched = fetching.fetch(false).await.expect("listed");

        assert_eq!(fetched.len(), 101);
        assert_eq!(fetched.last().map(InstalledRepository::name), Some("last"));
    }

    #[tokio::test]
    async fn a_failed_listing_is_not_cached() {
        let fetching = Fetching::new(1).await;
        Mock::given(method("GET"))
            .and(path("/installation/repositories"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&fetching.server)
            .await;
        listing(&fetching.server, 1, page_of(&[listed(1, "widgets")]), 1).await;

        let failed = fetching.fetch(false).await.expect_err("GitHub failed");
        assert!(
            matches!(failed, Error::Application(ApplicationError::Status(500))),
            "{failed:?}"
        );
        assert!(
            fetching
                .provider
                .catalog()
                .get(fetching.organization, fetching.installation, Instant::now())
                .is_none()
        );

        let fetched = fetching
            .fetch(false)
            .await
            .expect("listed after the outage");
        assert_eq!(names(&fetched), ["widgets"]);
    }

    #[tokio::test]
    async fn a_revoked_installation_is_refused_without_asking_github() {
        let fetching = Fetching::new(0).await;
        listing(&fetching.server, 1, page_of(&[]), 0).await;
        github_installations::mark_revoked(&fetching.pool, fetching.installation)
            .await
            .expect("revoke");

        let refused = fetching.fetch(true).await.expect_err("revoked");

        assert!(matches!(refused, Error::Revoked), "{refused:?}");
    }

    #[tokio::test]
    async fn another_organization_is_refused_even_a_cached_listing() {
        let fetching = Fetching::new(1).await;
        listing(&fetching.server, 1, page_of(&[listed(1, "widgets")]), 1).await;
        fetching.fetch(false).await.expect("the owner lists");
        let stranger = seed_organization(&fetching.pool).await;

        let refused = fetching
            .provider
            .catalog()
            .repositories(
                &fetching.provider,
                &fetching.pool,
                stranger,
                fetching.installation,
                false,
            )
            .await
            .expect_err("another organization's installation");

        assert!(matches!(refused, Error::NotConnected), "{refused:?}");
    }
}
