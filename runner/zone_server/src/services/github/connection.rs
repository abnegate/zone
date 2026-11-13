use serde::Deserialize;
use uuid::Uuid;

use super::configuration::Configuration;
use super::sources::Sources;
use super::unavailable::Unavailable;
use super::unconnected::Unconnected;
use crate::db::sources;
use crate::db::sources::SourceRow;
use crate::state::AppState;

pub(crate) struct ConnectedSource {
    pub(crate) source: SourceRow,
    pub(crate) configuration: Configuration,
}

pub(crate) async fn active_source(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
) -> Result<SourceRow, Unavailable> {
    let source = sources::get_source(state.db(), source_id, workspace_id)
        .await
        .map_err(|_| Unavailable::Unreadable)?
        .filter(|source| source.is_active.unwrap_or(true))
        .ok_or(Unavailable::Missing)?;
    if source.source_type != sources::GITHUB {
        return Err(Unavailable::NotGithub);
    }
    Ok(source)
}

pub(crate) async fn connect(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
) -> Result<ConnectedSource, Unconnected> {
    let source = active_source(state, workspace_id, source_id).await?;
    let configuration = Configuration::deserialize(&source.config)
        .map_err(|_| Unconnected::InvalidConfiguration)?;
    Ok(ConnectedSource {
        source,
        configuration,
    })
}

pub(crate) async fn connected_sources(
    state: &AppState,
    workspace_id: Uuid,
) -> Result<Sources, Unavailable> {
    let rows = sources::list_sources(
        state.db(),
        workspace_id,
        Some(sources::GITHUB),
        None,
        i64::MAX,
        0,
    )
    .await
    .map_err(|_| Unavailable::Unreadable)?;
    let mut listed = Sources::default();
    for source in rows
        .into_iter()
        .filter(|source| source.is_active.unwrap_or(true))
    {
        match Configuration::deserialize(&source.config) {
            Ok(configuration) => listed.connected.push(ConnectedSource {
                source,
                configuration,
            }),
            Err(_) => listed.invalid.push(source),
        }
    }
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github::testing::OWNER;
    use crate::services::github::testing::REPOSITORY;
    use crate::services::github::testing::connected::Connected;
    use serde_json::json;

    #[tokio::test]
    async fn an_active_github_source_connects_with_its_configuration() {
        let connected = Connected::new().await;
        let source = connected.source(None, None).await;

        let found = active_source(&connected.state, connected.workspace, source.id)
            .await
            .expect("an active GitHub source is found");
        let connection = connect(&connected.state, connected.workspace, source.id)
            .await
            .expect("an active GitHub source connects");

        assert_eq!(found.id, source.id);
        assert_eq!(connection.source.id, source.id);
        assert_eq!(connection.configuration.owner, OWNER);
        assert_eq!(connection.configuration.repository, REPOSITORY);
        assert_eq!(connection.configuration.branch.as_deref(), Some("main"));
        assert!(connection.configuration.token.is_none());
    }

    #[tokio::test]
    async fn a_source_that_is_absent_or_in_another_workspace_is_missing() {
        let connected = Connected::new().await;
        let source = connected.source(None, None).await;

        for (workspace, id) in [
            (connected.workspace, Uuid::new_v4()),
            (Uuid::new_v4(), source.id),
        ] {
            assert_eq!(
                active_source(&connected.state, workspace, id).await.err(),
                Some(Unavailable::Missing)
            );
            assert_eq!(
                connect(&connected.state, workspace, id).await.err(),
                Some(Unconnected::Unavailable(Unavailable::Missing))
            );
        }
    }

    #[tokio::test]
    async fn an_inactive_source_is_missing() {
        let connected = Connected::new().await;
        let source = connected.source(None, None).await;
        connected.deactivate(&source).await;

        assert_eq!(
            active_source(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unavailable::Missing)
        );
        assert_eq!(
            connect(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unconnected::Unavailable(Unavailable::Missing))
        );
    }

    #[tokio::test]
    async fn a_source_of_another_kind_is_not_github() {
        let connected = Connected::new().await;
        let source = connected
            .source_of(sources::WEB, json!({"url": "https://example.com"}))
            .await;

        assert_eq!(
            active_source(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unavailable::NotGithub)
        );
        assert_eq!(
            connect(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unconnected::Unavailable(Unavailable::NotGithub))
        );
    }

    #[tokio::test]
    async fn a_github_source_without_a_usable_configuration_is_found_but_never_connected() {
        let connected = Connected::new().await;
        let source = connected.source_of(sources::GITHUB, json!({})).await;

        assert_eq!(
            active_source(&connected.state, connected.workspace, source.id)
                .await
                .map(|found| found.id),
            Ok(source.id)
        );
        assert_eq!(
            connect(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unconnected::InvalidConfiguration)
        );
    }

    #[tokio::test]
    async fn every_active_github_source_in_the_workspace_is_listed_once() {
        let connected = Connected::new().await;
        let first = connected.source(None, None).await;
        let second = connected
            .source_of(sources::GITHUB, json!({"owner": OWNER, "repo": "other"}))
            .await;
        let invalid = connected.source_of(sources::GITHUB, json!({})).await;
        let inactive = connected.source(None, None).await;
        connected.deactivate(&inactive).await;
        connected
            .source_of(sources::WEB, json!({"url": "https://example.com"}))
            .await;
        let elsewhere = Connected::new().await;
        elsewhere.source(None, None).await;

        let listed = connected_sources(&connected.state, connected.workspace)
            .await
            .expect("the workspace's sources are listed");

        let mut found: Vec<Uuid> = listed
            .connected
            .iter()
            .map(|connection| connection.source.id)
            .collect();
        found.sort();
        let mut expected = vec![first.id, second.id];
        expected.sort();
        assert_eq!(found, expected);
        let named: Vec<String> = listed
            .connected
            .iter()
            .map(|connection| connection.configuration.full_name())
            .collect();
        assert!(
            named.contains(&format!("{OWNER}/{REPOSITORY}")),
            "{named:?}"
        );
        assert!(named.contains(&format!("{OWNER}/other")), "{named:?}");
        let unusable: Vec<Uuid> = listed.invalid.iter().map(|source| source.id).collect();
        assert_eq!(unusable, [invalid.id]);
    }

    #[tokio::test]
    async fn a_workspace_whose_sources_cannot_be_read_is_unreadable() {
        let connected = Connected::new().await;
        connected.pool.close().await;

        assert_eq!(
            connected_sources(&connected.state, connected.workspace)
                .await
                .err(),
            Some(Unavailable::Unreadable)
        );
    }

    #[tokio::test]
    async fn a_source_that_cannot_be_read_is_unreadable() {
        let connected = Connected::new().await;
        let source = connected.source(None, None).await;
        connected.pool.close().await;

        assert_eq!(
            active_source(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unavailable::Unreadable)
        );
        assert_eq!(
            connect(&connected.state, connected.workspace, source.id)
                .await
                .err(),
            Some(Unconnected::Unavailable(Unavailable::Unreadable))
        );
    }
}
