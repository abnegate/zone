use std::num::{NonZeroU8, NonZeroU32};
use std::sync::Arc;

use abnegate_vcs::PullRequestError;
use abnegate_vcs::pull_request::{self, InstalledRepository};
use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::OrgMember;
use crate::db::github_installations::{self, InstallationRow, InstallationStatus};
use crate::error::ServerError;
use crate::services::github_app;
use crate::state::AppState;

const DEFAULT_PER_PAGE: u32 = 50;

const MAXIMUM_PER_PAGE: u8 = 100;

const BRANCH_PAGE_SIZE: NonZeroU8 = NonZeroU8::new(100).unwrap();

const INVALID_PAGE: &str = "page must be 1 or more.";

const INVALID_PER_PAGE: &str = "per_page must be between 1 and 100.";

const INSTALLATION_NOT_FOUND: &str = "GitHub installation not found.";

const REPOSITORY_NOT_FOUND: &str =
    "Repository not found, or not reachable through this GitHub connection.";

const GITHUB_UNAVAILABLE: &str = "GitHub could not be reached.";

const INTERNAL_ERROR: &str = "Internal server error";

fn first_page() -> u32 {
    1
}

fn default_per_page() -> u32 {
    DEFAULT_PER_PAGE
}

#[derive(Debug, Deserialize)]
pub struct Search {
    #[serde(default)]
    query: String,
    installation_id: Option<Uuid>,
    #[serde(default = "first_page")]
    page: u32,
    #[serde(default = "default_per_page")]
    per_page: u32,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Deserialize)]
pub struct BranchSearch {
    #[serde(default = "first_page")]
    page: u32,
}

#[derive(Debug, Deserialize)]
pub struct BranchPath {
    id: Uuid,
    owner: String,
    repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Repository {
    installation_id: Uuid,
    id: u64,
    owner: String,
    name: String,
    full_name: String,
    private: bool,
    archived: bool,
    default_branch: String,
    html_url: String,
}

impl Repository {
    fn of(installation_id: Uuid, repository: &InstalledRepository) -> Self {
        Self {
            installation_id,
            id: repository.id(),
            owner: repository.owner().to_string(),
            name: repository.name().to_string(),
            full_name: repository.full_name().to_string(),
            private: repository.private(),
            archived: repository.archived(),
            default_branch: repository.default_branch().as_str().to_string(),
            html_url: repository.html_url().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unlisted {
    Revoked,
    Suspended,
    Unavailable,
}

impl Unlisted {
    fn of(installation: Uuid, error: &github_app::Error) -> Self {
        match error {
            github_app::Error::Revoked => Self::Revoked,
            github_app::Error::Suspended => Self::Suspended,
            error => {
                tracing::warn!(
                    installation = %installation,
                    error = %error,
                    "The repositories of a GitHub installation could not be listed"
                );
                Self::Unavailable
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListedInstallation {
    id: Uuid,
    account_login: String,
    html_url: String,
    error: Option<Unlisted>,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    repositories: Vec<Repository>,
    installations: Vec<ListedInstallation>,
    total: usize,
    next_page: Option<NonZeroU32>,
}

#[derive(Debug, Serialize)]
pub struct Branch {
    name: String,
    protected: bool,
}

#[derive(Debug, Serialize)]
pub struct Branches {
    branches: Vec<Branch>,
    next_page: Option<NonZeroU32>,
}

fn page_of(page: u32) -> Result<NonZeroU32, ServerError> {
    NonZeroU32::new(page).ok_or_else(|| ServerError::BadRequest(INVALID_PAGE.to_string()))
}

fn per_page_of(per_page: u32) -> Result<NonZeroU8, ServerError> {
    u8::try_from(per_page)
        .ok()
        .filter(|per_page| *per_page <= MAXIMUM_PER_PAGE)
        .and_then(NonZeroU8::new)
        .ok_or_else(|| ServerError::BadRequest(INVALID_PER_PAGE.to_string()))
}

fn installation_not_found() -> ServerError {
    ServerError::NotFound(INSTALLATION_NOT_FOUND.to_string())
}

fn repository_not_found() -> ServerError {
    ServerError::NotFound(REPOSITORY_NOT_FOUND.to_string())
}

fn unavailable() -> ServerError {
    ServerError::BadGateway(GITHUB_UNAVAILABLE.to_string())
}

fn internal() -> ServerError {
    ServerError::Internal(INTERNAL_ERROR.to_string())
}

/// The rows matching `query`, a case-insensitive substring of the full name,
/// sorted case-insensitively by owner then name; then page `page` of them,
/// how many matched, and the page after it when there is one.
pub fn select(
    rows: Vec<Repository>,
    query: &str,
    page: NonZeroU32,
    per_page: NonZeroU8,
) -> (Vec<Repository>, usize, Option<NonZeroU32>) {
    let needle = query.trim().to_lowercase();
    let mut matched: Vec<Repository> = rows
        .into_iter()
        .filter(|row| row.full_name.to_lowercase().contains(&needle))
        .collect();
    matched.sort_by_cached_key(|row| {
        (
            row.owner.to_lowercase(),
            row.name.to_lowercase(),
            row.full_name.clone(),
            row.installation_id,
        )
    });

    let total = matched.len();
    let per_page = usize::from(per_page.get());
    let start = usize::try_from(page.get() - 1)
        .ok()
        .and_then(|skipped| skipped.checked_mul(per_page))
        .filter(|start| *start < total);
    let Some(start) = start else {
        return (Vec::new(), total, None);
    };
    let end = start.saturating_add(per_page).min(total);
    let next_page = (end < total).then(|| page.checked_add(1)).flatten();
    let selected = matched.drain(start..end).collect();

    (selected, total, next_page)
}

pub async fn list(
    member: OrgMember,
    State(state): State<AppState>,
    search: Result<Query<Search>, QueryRejection>,
) -> Result<Json<Listing>, ServerError> {
    let Query(search) =
        search.map_err(|rejection| ServerError::BadRequest(rejection.body_text()))?;
    let page = page_of(search.page)?;
    let per_page = per_page_of(search.per_page)?;

    let mut installations = github_installations::list(state.db(), member.org_id).await?;
    if let Some(chosen) = search.installation_id {
        installations.retain(|installation| installation.id == chosen);
        if installations.is_empty() {
            return Err(installation_not_found());
        }
    }

    let outcomes = join_all(
        installations
            .iter()
            .map(|installation| listed(&state, member.org_id, installation, search.refresh)),
    )
    .await;

    let mut rows = Vec::new();
    let mut listed_installations = Vec::with_capacity(installations.len());
    for (installation, outcome) in installations.into_iter().zip(outcomes) {
        let error = match outcome {
            Ok(repositories) => {
                rows.extend(
                    repositories
                        .iter()
                        .map(|repository| Repository::of(installation.id, repository)),
                );
                None
            }
            Err(unlisted) => Some(unlisted),
        };
        listed_installations.push(ListedInstallation {
            id: installation.id,
            account_login: installation.account_login,
            html_url: installation.html_url,
            error,
        });
    }

    let (repositories, total, next_page) = select(rows, &search.query, page, per_page);
    Ok(Json(Listing {
        repositories,
        installations: listed_installations,
        total,
        next_page,
    }))
}

async fn listed(
    state: &AppState,
    organization_id: Uuid,
    installation: &InstallationRow,
    refresh: bool,
) -> Result<Arc<[InstalledRepository]>, Unlisted> {
    match installation.status() {
        InstallationStatus::Active => {}
        InstallationStatus::Revoked => return Err(Unlisted::Revoked),
        InstallationStatus::Suspended => return Err(Unlisted::Suspended),
    }

    state
        .github()
        .repositories(state.db(), organization_id, installation.id, refresh)
        .await
        .map_err(|error| Unlisted::of(installation.id, &error))
}

pub async fn branches(
    member: OrgMember,
    State(state): State<AppState>,
    path: Result<Path<BranchPath>, PathRejection>,
    search: Result<Query<BranchSearch>, QueryRejection>,
) -> Result<Json<Branches>, ServerError> {
    let Path(path) = path.map_err(|_| installation_not_found())?;
    let Query(search) =
        search.map_err(|rejection| ServerError::BadRequest(rejection.body_text()))?;
    let page = page_of(search.page)?;
    let repository = pull_request::Repository::new(&path.owner, &path.repository)
        .map_err(|_| repository_not_found())?;
    let service = state.config().pull_requests().map_err(|error| {
        tracing::error!(error = %error, "The GitHub API origin is not usable");
        internal()
    })?;

    let provider = state.github();
    let credential = provider
        .organization_installation(state.db(), member.org_id, path.id)
        .await
        .map_err(|error| credential_refusal(path.id, error))?;

    let service = &service;
    let repository = &repository;
    let listed = provider
        .retrying(state.db(), credential, |token| async move {
            service
                .list_branches(repository, &token, page, BRANCH_PAGE_SIZE)
                .await
        })
        .await
        .map_err(|error| branch_refusal(path.id, error))?;

    Ok(Json(Branches {
        branches: listed
            .items
            .into_iter()
            .map(|branch| Branch {
                name: branch.name.as_str().to_string(),
                protected: branch.protected,
            })
            .collect(),
        next_page: listed.next,
    }))
}

fn credential_refusal(installation: Uuid, error: github_app::Error) -> ServerError {
    match error {
        github_app::Error::Revoked
        | github_app::Error::Suspended
        | github_app::Error::NotConnected => installation_not_found(),
        github_app::Error::Application(_) => {
            tracing::warn!(installation = %installation, error = %error, "GitHub could not mint an installation token");
            unavailable()
        }
        error => {
            tracing::error!(installation = %installation, error = %error, "An installation token could not be minted");
            internal()
        }
    }
}

fn branch_refusal(installation: Uuid, error: PullRequestError) -> ServerError {
    match error {
        PullRequestError::NotFound
        | PullRequestError::Forbidden
        | PullRequestError::InvalidRepositoryName => repository_not_found(),
        PullRequestError::Http(error) => {
            tracing::warn!(installation = %installation, error = %error.without_url(), "GitHub could not be reached to list branches");
            unavailable()
        }
        error => {
            tracing::warn!(installation = %installation, error = %error, "GitHub could not list branches");
            unavailable()
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::RemoteBranchName;

    use super::*;

    const INSTALLATION: Uuid = Uuid::from_u128(7);

    fn row(owner: &str, name: &str) -> Repository {
        Repository {
            installation_id: INSTALLATION,
            id: 1,
            owner: owner.to_string(),
            name: name.to_string(),
            full_name: format!("{owner}/{name}"),
            private: false,
            archived: false,
            default_branch: "main".to_string(),
            html_url: format!("https://github.com/{owner}/{name}"),
        }
    }

    fn page(number: u32) -> NonZeroU32 {
        NonZeroU32::new(number).expect("a page number")
    }

    fn per_page(number: u8) -> NonZeroU8 {
        NonZeroU8::new(number).expect("a page size")
    }

    fn full_names(rows: &[Repository]) -> Vec<&str> {
        rows.iter().map(|row| row.full_name.as_str()).collect()
    }

    fn catalog() -> Vec<Repository> {
        vec![
            row("acme", "widgets"),
            row("other", "tools"),
            row("acme", "Gadgets"),
            row("Beta", "zeta"),
            row("beta", "Alpha"),
        ]
    }

    fn installed() -> InstalledRepository {
        InstalledRepository::new(
            42,
            pull_request::Repository::new("acme", "widgets").expect("repository"),
            RemoteBranchName::parse("trunk").expect("branch"),
            "https://github.com/acme/widgets",
        )
        .with_private(true)
    }

    #[test]
    fn rows_sort_case_insensitively_by_owner_then_name() {
        let (selected, total, next_page) = select(catalog(), "", page(1), per_page(50));

        assert_eq!(
            full_names(&selected),
            [
                "acme/Gadgets",
                "acme/widgets",
                "beta/Alpha",
                "Beta/zeta",
                "other/tools"
            ]
        );
        assert_eq!(total, 5);
        assert_eq!(next_page, None);
    }

    #[test]
    fn the_query_is_a_trimmed_case_insensitive_substring_of_the_full_name() {
        let (selected, total, _) = select(catalog(), "  ACME/wid ", page(1), per_page(50));
        assert_eq!(full_names(&selected), ["acme/widgets"]);
        assert_eq!(total, 1);

        let (selected, _, _) = select(catalog(), "ET", page(1), per_page(50));
        assert_eq!(
            full_names(&selected),
            ["acme/Gadgets", "acme/widgets", "beta/Alpha", "Beta/zeta"]
        );

        let (selected, total, next_page) = select(catalog(), "nothing", page(1), per_page(50));
        assert!(selected.is_empty());
        assert_eq!(total, 0);
        assert_eq!(next_page, None);
    }

    #[test]
    fn a_blank_query_matches_everything() {
        let (_, total, _) = select(catalog(), "   ", page(1), per_page(50));

        assert_eq!(total, 5);
    }

    #[test]
    fn pages_follow_until_the_last() {
        let (first, total, next_page) = select(catalog(), "", page(1), per_page(2));
        assert_eq!(full_names(&first), ["acme/Gadgets", "acme/widgets"]);
        assert_eq!(total, 5);
        assert_eq!(next_page, Some(page(2)));

        let (last, total, next_page) = select(catalog(), "", page(3), per_page(2));
        assert_eq!(full_names(&last), ["other/tools"]);
        assert_eq!(total, 5);
        assert_eq!(next_page, None);

        let (exact, _, next_page) = select(catalog(), "", page(1), per_page(5));
        assert_eq!(exact.len(), 5);
        assert_eq!(next_page, None);
    }

    #[test]
    fn the_total_counts_what_the_query_kept() {
        let (selected, total, next_page) = select(catalog(), "a", page(1), per_page(1));

        assert_eq!(full_names(&selected), ["acme/Gadgets"]);
        assert_eq!(total, 4);
        assert_eq!(next_page, Some(page(2)));
    }

    #[test]
    fn a_page_beyond_the_last_is_empty_with_no_next_page() {
        for number in [4, 9, u32::MAX] {
            let (selected, total, next_page) = select(catalog(), "", page(number), per_page(2));

            assert!(selected.is_empty(), "page {number}");
            assert_eq!(total, 5);
            assert_eq!(next_page, None);
        }
    }

    #[test]
    fn paging_is_validated() {
        assert!(page_of(0).is_err());
        assert_eq!(page_of(1).ok(), Some(page(1)));
        assert!(per_page_of(0).is_err());
        assert!(per_page_of(101).is_err());
        assert!(per_page_of(256).is_err());
        assert_eq!(per_page_of(100).ok(), Some(per_page(100)));
        assert_eq!(per_page_of(1).ok(), Some(per_page(1)));
    }

    #[test]
    fn a_repository_carries_every_field() {
        let repository = Repository::of(INSTALLATION, &installed());

        assert_eq!(
            repository,
            Repository {
                installation_id: INSTALLATION,
                id: 42,
                owner: "acme".to_string(),
                name: "widgets".to_string(),
                full_name: "acme/widgets".to_string(),
                private: true,
                archived: false,
                default_branch: "trunk".to_string(),
                html_url: "https://github.com/acme/widgets".to_string(),
            }
        );
    }

    #[test]
    fn unlisted_installations_serialize_lowercase() {
        assert_eq!(
            serde_json::to_value([
                Unlisted::Revoked,
                Unlisted::Suspended,
                Unlisted::Unavailable
            ])
            .expect("serialize"),
            serde_json::json!(["revoked", "suspended", "unavailable"])
        );
    }

    #[test]
    fn revoked_and_suspended_keep_their_names_and_the_rest_are_unavailable() {
        assert_eq!(
            Unlisted::of(INSTALLATION, &github_app::Error::Revoked),
            Unlisted::Revoked
        );
        assert_eq!(
            Unlisted::of(INSTALLATION, &github_app::Error::Suspended),
            Unlisted::Suspended
        );
        assert_eq!(
            Unlisted::of(INSTALLATION, &github_app::Error::NotConnected),
            Unlisted::Unavailable
        );
    }
}
