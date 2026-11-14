use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use super::OPTIONS_TTL;
use super::PIPELINE_TTL;
use super::REFRESH_FLOOR;
use super::Snapshots;
use crate::config::GithubConfig;
use crate::services::github::freshness::Freshness;
use crate::services::github::pull_requests;
use crate::services::github::releases;
use crate::services::github::releases::SourceOptions;

fn options(source_id: Uuid) -> SourceOptions {
    SourceOptions {
        source_id,
        repository: "owner/repository".into(),
        repository_url: "https://github.com/owner/repository".into(),
        default_branch: "main".into(),
        latest_tag: None,
        next_tag: "v0.1.0".into(),
        previous_tags: Vec::new(),
        lines: Vec::new(),
    }
}

async fn freshness(snapshots: &Snapshots, source_id: Uuid) -> [Freshness; 3] {
    let now = Utc::now();
    let pull_requests = snapshots
        .pull_requests
        .read(source_id, false, now, |_| async {
            Ok(pull_requests::Listing::default())
        })
        .await
        .expect("read the pull requests")
        .1;
    let releases = snapshots
        .releases
        .read(source_id, false, now, |_| async {
            Ok(releases::Listing::default())
        })
        .await
        .expect("read the releases")
        .1;
    let options = snapshots
        .options
        .read(
            source_id,
            false,
            now,
            |_| async move { Ok(options(source_id)) },
        )
        .await
        .expect("read the options")
        .1;
    [pull_requests, releases, options]
}

#[test]
fn each_cache_lives_as_long_as_its_setting_says() {
    let config = GithubConfig {
        pull_request_ttl: Duration::from_secs(60),
        release_ttl: Duration::from_secs(600),
        ..GithubConfig::default()
    };

    let snapshots = Snapshots::new(&config);

    assert_eq!(snapshots.pull_requests.ttl(), Duration::from_secs(60));
    assert_eq!(snapshots.releases.ttl(), Duration::from_secs(600));
    assert_eq!(snapshots.options.ttl(), OPTIONS_TTL);
    assert_eq!(snapshots.pipelines.ttl(), PIPELINE_TTL);
    for floor in [
        snapshots.pull_requests.floor(),
        snapshots.releases.floor(),
        snapshots.options.floor(),
        snapshots.pipelines.floor(),
    ] {
        assert_eq!(floor, REFRESH_FLOOR);
    }
    assert_eq!(PIPELINE_TTL, Duration::from_secs(15));
    assert_eq!(OPTIONS_TTL, Duration::from_secs(300));
    assert_eq!(REFRESH_FLOOR, Duration::from_secs(10));
}

#[tokio::test]
async fn invalidating_a_source_expires_every_read_of_it_and_nothing_else() {
    let snapshots = Snapshots::new(&GithubConfig::default());
    let written = Uuid::new_v4();
    let untouched = Uuid::new_v4();
    freshness(&snapshots, written).await;
    freshness(&snapshots, untouched).await;

    snapshots.invalidate(written);

    assert_eq!(
        freshness(&snapshots, written).await,
        [Freshness::Refreshed; 3]
    );
    assert_eq!(
        freshness(&snapshots, untouched).await,
        [Freshness::Cached; 3]
    );
}

#[test]
fn one_release_runs_per_source_at_a_time() {
    let snapshots = Snapshots::new(&GithubConfig::default());
    let source = Uuid::new_v4();
    let other = Uuid::new_v4();

    let guard = snapshots
        .try_release(source)
        .expect("the first release claims its source");
    assert_eq!(guard.source_id(), source);
    assert!(
        snapshots.try_release(source).is_none(),
        "a second release of the source waits"
    );
    let elsewhere = snapshots
        .try_release(other)
        .expect("another source releases at the same time");

    drop(guard);
    assert!(
        snapshots.try_release(source).is_some(),
        "the source is free once its release ends"
    );
    drop(elsewhere);
}
