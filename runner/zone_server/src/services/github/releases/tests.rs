use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::services::github::connection::ConnectedSource;
use crate::services::github::release::PipelineState;
use crate::services::github::release::ReleaseIdentity;
use crate::services::github::release::ReleasePipeline;
use crate::services::github::release::RunEvidence;
use crate::services::github::release::run_url;
use crate::services::github::testing::connected_source;

const SOURCE: &str = "6f2c1f8e-0d55-4a3c-9a43-2f1d4c1e7b10";
const ORIGIN: &str = "https://github.example.com";
const REPOSITORY: &str = "owner/repository";
const TAG: &str = "v1.4.0";
const PUBLISHED_AT: &str = "2026-11-14T00:00:00Z";
const CHECKED_AT: &str = "2026-11-14T00:05:00Z";
const BUILT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RELEASE_URL: &str = "https://github.example.com/owner/repository/releases/tag/v1.4.0";

fn source() -> ConnectedSource {
    connected_source(SOURCE.parse().unwrap(), "Zone")
}

fn identity() -> ReleaseIdentity {
    ReleaseIdentity {
        repository: REPOSITORY.into(),
        id: 90,
        tag: TAG.into(),
        published_at: PUBLISHED_AT.into(),
        url: RELEASE_URL.into(),
    }
}

fn succeeded() -> ReleasePipeline {
    let run = RunEvidence {
        id: 501,
        workflow_id: 3,
        attempt: 1,
        name: "Publish".into(),
        path: ".github/workflows/publish.yml".into(),
        event: "release".into(),
        status: "completed".into(),
        conclusion: Some("success".into()),
        head_branch: TAG.into(),
        head_sha: BUILT.into(),
        created_at: "2026-11-14T00:01:00Z".into(),
        started_at: None,
        updated_at: "2026-11-14T00:04:00Z".into(),
        url: run_url(ORIGIN, REPOSITORY, 501),
        repository: REPOSITORY.into(),
    };
    ReleasePipeline::observe(identity(), &[run], CHECKED_AT, ORIGIN)
}

fn published() -> abnegate_vcs::Release {
    abnegate_vcs::Release::new(90, TAG, RELEASE_URL)
        .with_name("Zone 1.4.0")
        .with_published_at(PUBLISHED_AT)
        .with_prerelease(true)
}

fn pulls() -> Vec<IncludedPull> {
    vec![IncludedPull {
        number: 7,
        url: "https://github.example.com/owner/repository/pull/7".into(),
        title: "Add the thing".into(),
    }]
}

#[test]
fn an_observed_release_carries_its_source_and_its_pipelines_verdict() {
    let release = Release::observed(
        &source(),
        ORIGIN,
        &published(),
        pulls(),
        ReleasePipeline::observe(identity(), &[], CHECKED_AT, ORIGIN),
    );

    assert_eq!(
        serde_json::to_value(&release).unwrap(),
        json!({
            "source_id": SOURCE,
            "repository": REPOSITORY,
            "repository_url": "https://github.example.com/owner/repository",
            "id": 90,
            "tag": TAG,
            "name": "Zone 1.4.0",
            "url": RELEASE_URL,
            "published_at": PUBLISHED_AT,
            "prerelease": true,
            "pulls": [{
                "number": 7,
                "url": "https://github.example.com/owner/repository/pull/7",
                "title": "Add the thing",
            }],
            "pipeline": {
                "release": {
                    "repository": REPOSITORY,
                    "id": 90,
                    "tag": TAG,
                    "published_at": PUBLISHED_AT,
                    "url": RELEASE_URL,
                },
                "lookup": "pending",
                "runs": [],
                "checked_at": CHECKED_AT,
            },
            "state": "unknown",
            "succeeded": false,
            "complete": false,
            "blockers": serde_json::to_value(release.pipeline.reports()).unwrap(),
        })
    );
    let codes: Vec<_> = release
        .blockers
        .iter()
        .map(|blocker| serde_json::to_value(blocker.code).unwrap())
        .collect();
    assert_eq!(codes, [json!("lookup_pending"), json!("runs_absent")]);
}

#[test]
fn a_new_pipeline_replaces_the_verdict_with_its_own() {
    let pending = Release::observed(
        &source(),
        ORIGIN,
        &published(),
        pulls(),
        ReleasePipeline::unavailable(identity(), CHECKED_AT),
    );
    assert!(!pending.complete);
    assert!(!pending.blockers.is_empty());

    let release = pending.with_pipeline(succeeded());

    assert_eq!(release.state, PipelineState::Succeeded);
    assert!(release.succeeded);
    assert!(release.complete);
    assert!(release.blockers.is_empty());
    assert_eq!(release.pipeline.runs.len(), 1);
    assert_eq!(release.pulls, pulls());
    assert_eq!(release.source_id, SOURCE.parse::<Uuid>().unwrap());
    let encoded = serde_json::to_value(&release).unwrap();
    assert_eq!(encoded["pipeline"]["runs"][0]["state"], json!("succeeded"));
    assert!(
        encoded["pipeline"]["runs"][0].get("started_at").is_none(),
        "a run that never said when it started leaves the field out"
    );
}

#[test]
fn source_options_serialise_as_the_console_reads_them() {
    let options = SourceOptions {
        source_id: SOURCE.parse().unwrap(),
        repository: REPOSITORY.into(),
        repository_url: "https://github.example.com/owner/repository".into(),
        default_branch: "main".into(),
        latest_tag: None,
        next_tag: "v0.1.0".into(),
        previous_tags: Vec::new(),
        lines: vec![TagLine {
            prefix: Some("pkg".into()),
            latest_tag: Some("pkg/v1.0.0".into()),
            next_tag: "pkg/v1.0.1".into(),
            previous_tags: vec!["pkg/v1.0.0".into()],
        }],
    };

    assert_eq!(
        serde_json::to_value(options).unwrap(),
        json!({
            "source_id": SOURCE,
            "repository": REPOSITORY,
            "repository_url": "https://github.example.com/owner/repository",
            "default_branch": "main",
            "latest_tag": null,
            "next_tag": "v0.1.0",
            "previous_tags": [],
            "lines": [{
                "prefix": "pkg",
                "latest_tag": "pkg/v1.0.0",
                "next_tag": "pkg/v1.0.1",
                "previous_tags": ["pkg/v1.0.0"],
            }],
        })
    );
}

#[test]
fn a_listing_narrows_to_the_releases_published_since_a_moment() {
    let at = |published_at: &str| Release {
        published_at: published_at.into(),
        ..Release::observed(
            &source(),
            ORIGIN,
            &published(),
            pulls(),
            ReleasePipeline::observe(identity(), &[], CHECKED_AT, ORIGIN),
        )
    };
    let listing = Listing {
        releases: vec![
            at("2026-11-14T00:00:00Z"),
            at("2026-11-01T00:00:00Z"),
            at("unknown"),
        ],
        complete: true,
        warnings: vec!["kept".into()],
    };

    let narrowed = listing.since("2026-11-07T00:00:00Z".parse().unwrap());

    let kept: Vec<&str> = narrowed
        .releases
        .iter()
        .map(|release| release.published_at.as_str())
        .collect();
    assert_eq!(kept, ["2026-11-14T00:00:00Z"]);
    assert!(narrowed.complete);
    assert_eq!(narrowed.warnings, ["kept"]);
}

#[test]
fn a_preview_serialises_as_the_console_reads_it() {
    let preview = Preview {
        source_id: SOURCE.parse().unwrap(),
        repository: REPOSITORY.into(),
        tag: TAG.into(),
        name: TAG.into(),
        body: "## What's Changed".into(),
        base_tag: Some("v1.3.9".into()),
        target: BUILT.into(),
        prerelease: false,
        pulls: pulls(),
        digest: "d1".into(),
    };

    assert_eq!(
        serde_json::to_value(preview).unwrap(),
        json!({
            "source_id": SOURCE,
            "repository": REPOSITORY,
            "tag": TAG,
            "name": TAG,
            "body": "## What's Changed",
            "base_tag": "v1.3.9",
            "target": BUILT,
            "prerelease": false,
            "pulls": [{
                "number": 7,
                "url": "https://github.example.com/owner/repository/pull/7",
                "title": "Add the thing",
            }],
            "digest": "d1",
        })
    );
}

#[test]
fn a_created_release_serialises_as_the_console_reads_it() {
    let created = Created {
        source_id: SOURCE.parse().unwrap(),
        id: 90,
        tag: TAG.into(),
        name: None,
        url: RELEASE_URL.into(),
        published_at: PUBLISHED_AT.into(),
    };

    assert_eq!(
        serde_json::to_value(created).unwrap(),
        json!({
            "source_id": SOURCE,
            "id": 90,
            "tag": TAG,
            "name": null,
            "url": RELEASE_URL,
            "published_at": PUBLISHED_AT,
        })
    );
}
