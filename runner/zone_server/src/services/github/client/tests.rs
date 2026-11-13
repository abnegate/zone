use super::*;
use crate::services::github::pull_requests::CheckKind;
use crate::services::github::testing::BLOB;
use crate::services::github::testing::COMMIT;
use crate::services::github::testing::RATE_LIMIT_REMAINING_HEADER;
use crate::services::github::testing::RETRY_AFTER_HEADER;
use crate::services::github::testing::assert_sent_without_authorization;
use crate::services::github::testing::check_runs;
use crate::services::github::testing::commit_and_empty_ci;
use crate::services::github::testing::github_with_token;
use crate::services::github::testing::mock;
use crate::services::github::testing::mock_answering;
use crate::services::github::testing::mock_graphql;
use crate::services::github::testing::mock_green_ci;
use crate::services::github::testing::mock_green_ci_answering;
use crate::services::github::testing::mock_pull;
use crate::services::github::testing::mock_review;
use crate::services::github::testing::mock_statuses;
use crate::services::github::testing::mocked;
use crate::services::github::testing::pull_detail;
use crate::services::github::testing::served;
use crate::services::github::testing::summary;
use crate::services::github::testing::thread_counts;
use abnegate_vcs::RateLimit;
use chrono::DateTime;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use std::time::Instant;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;

const JOB_ID: u64 = 4242;
const RUN_ID: u64 = 99;

/// Pull request `number` read and assessed as the readiness tool reads one
/// named by number.
async fn assessed_pull(github: &Github, number: u64) -> Result<readiness::Assessment, String> {
    let reference = github.reference(number)?;
    let detail = github.pull(&reference).await?;
    let assessed = github
        .assess(&listed(reference, &detail), Some(detail), Depth::Full)
        .await?;
    Ok(assessed.assessment)
}

#[test]
fn absent_unknown_and_pending_results_are_not_green() {
    for conclusions in [vec![], vec!["unknown"], vec!["success", "unexpected"]] {
        assert_eq!(assessment(&conclusions), UNKNOWN_ASSESSMENT);
    }
    assert_eq!(assessment(&["success"]), SUCCESS_ASSESSMENT);
    assert_eq!(assessment(&["success", "pending"]), PENDING_ASSESSMENT);
    assert_eq!(assessment(&["pending", "failure"]), FAILURE_ASSESSMENT);
    assert_eq!(assessment(&["cancelled"]), FAILURE_ASSESSMENT);
}

/// A completed run that concluded neutral or skipped has reported, and
/// reporting is the one thing `unknown` says did not happen. Reading it as
/// unknown had a wait sit out the whole grace period and then answer that
/// no checks are configured or reporting on the commit — about a commit
/// whose checks had reported and finished. Each settles under its own name,
/// and none of those names is a pass.
#[test]
fn a_completed_run_that_neither_passed_nor_failed_settles_under_its_own_name() {
    for conclusion in TERMINAL_ASSESSMENTS {
        assert_eq!(assessment(&[conclusion]), conclusion);
        assert_eq!(
            assessment(&["success", conclusion]),
            conclusion,
            "the conclusion that is not a pass is the one the commit is reported as"
        );
        assert_ne!(assessment(&[conclusion]), SUCCESS_ASSESSMENT);
        assert_ne!(
            assessment(&[conclusion]),
            UNKNOWN_ASSESSMENT,
            "{conclusion} reported, which is the opposite of what unknown stands for"
        );
        assert!(
            SETTLED_ASSESSMENTS.contains(&assessment(&[conclusion])),
            "{conclusion} is over, so a wait on it has nothing left to wait for"
        );
    }
    assert_eq!(
        assessment(&["queued", "skipped"]),
        PENDING_ASSESSMENT,
        "a run still queued beside a skipped one has not finished"
    );
    assert_eq!(
        assessment(&["failure", "neutral"]),
        FAILURE_ASSESSMENT,
        "a failure outranks every other conclusion on the commit"
    );
}

#[tokio::test]
async fn provider_errors_never_echo_secret_or_follow_redirects() {
    let server = MockServer::start().await;
    let external = MockServer::start().await;
    let unbranched = |server: &MockServer| {
        let mut github = mocked(server);
        github.configuration.branch = None;
        github
    };
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", external.uri())
                .set_body_string("test-secret"),
        )
        .mount(&server)
        .await;
    let error = unbranched(&server).resolve(None).await.unwrap_err();
    assert_eq!(error, status_error(302));
    assert!(error.contains("302"));
    assert!(!error.contains("test-secret"));
    assert!(external.received_requests().await.unwrap().is_empty());
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_string("test-secret"))
        .mount(&server)
        .await;
    let error = unbranched(&server).resolve(None).await.unwrap_err();
    assert_eq!(error, status_error(FORBIDDEN));
    assert!(!error.contains("test-secret"));
}

#[tokio::test]
async fn an_unclassified_status_names_itself_and_never_github_s_words() {
    for status in [400, 409, 410, UNPROCESSABLE_ENTITY, 500, 503] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/commits/main"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"message": "test-secret"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let error = mocked(&server).resolve(None).await.unwrap_err();
        assert_eq!(error, status_error(status), "HTTP {status}");
        assert!(!error.contains("test-secret"), "HTTP {status}: {error}");
    }
}

#[tokio::test]
async fn every_request_names_the_api_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repository"))
        .and(header("X-GitHub-Api-Version", API_VERSION))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repository/commits/main"))
        .and(header("X-GitHub-Api-Version", API_VERSION))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sha": COMMIT})))
        .mount(&server)
        .await;
    let mut github = mocked(&server);
    github.configuration.branch = None;
    let resolved = github.resolve(None).await.unwrap();
    assert_eq!(resolved, ("main".to_string(), COMMIT.to_string()));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests.iter().all(|request| request
            .headers
            .get("x-github-api-version")
            .is_some_and(|version| version == API_VERSION)),
        "every request names the API version"
    );
}

#[test]
fn crate_errors_keep_zones_words() {
    let cases = [
        (
            PullRequestError::quota_exhausted(
                RateLimit::default()
                    .with_limit(5000)
                    .with_remaining(0)
                    .with_resource("core"),
            ),
            RATE_LIMIT_EXHAUSTED.to_string(),
        ),
        (
            PullRequestError::quota_exhausted(RateLimit::default()),
            RATE_LIMIT_EXHAUSTED.to_string(),
        ),
        (
            PullRequestError::AuthenticationFailed,
            status_error(UNAUTHORIZED),
        ),
        (
            PullRequestError::AuthenticationRequired,
            status_error(UNAUTHORIZED),
        ),
        (PullRequestError::Forbidden, status_error(FORBIDDEN)),
        (PullRequestError::NotFound, status_error(NOT_FOUND)),
        (
            PullRequestError::RepositoryNotFound,
            status_error(NOT_FOUND),
        ),
        (
            PullRequestError::PullRequestNotFound,
            status_error(NOT_FOUND),
        ),
        (
            PullRequestError::RateLimited,
            status_error(TOO_MANY_REQUESTS),
        ),
        (
            PullRequestError::PullRequestAlreadyExists(BranchName::parse("feature").unwrap()),
            status_error(UNPROCESSABLE_ENTITY),
        ),
        (
            PullRequestError::JobLog(JobLogError::Undirected),
            LOG_UNDIRECTED.to_string(),
        ),
        (
            PullRequestError::JobLog(JobLogError::UnexpectedHost),
            LOG_UNEXPECTED_HOST.to_string(),
        ),
        (
            PullRequestError::JobLog(JobLogError::Unreachable),
            LOG_UNREACHABLE.to_string(),
        ),
        (
            PullRequestError::JobLog(JobLogError::Refused(503)),
            "The job log download returned HTTP 503.".to_string(),
        ),
        (
            PullRequestError::JobLog(JobLogError::Interrupted),
            LOG_INTERRUPTED.to_string(),
        ),
        (PullRequestError::TooLarge, FILE_TOO_LARGE.to_string()),
        (PullRequestError::InvalidOrigin, INVALID_ORIGIN.to_string()),
        (
            PullRequestError::InvalidRepositoryName,
            INVALID_REPOSITORY.to_string(),
        ),
        (
            PullRequestError::InvalidRepositoryUrl,
            INVALID_REPOSITORY.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::BranchName("test-secret".into())),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::CommitSha("test-secret".into())),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::ObjectId("test-secret".into())),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::RepositoryPath("test-secret".into())),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::RepositoryUrl),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Parse(ParseError::Tag("test-secret".into())),
            INVALID_RESPONSE.to_string(),
        ),
        (
            PullRequestError::GitHubApi(
                "GitHub API returned 500 Internal Server Error: test-secret".into(),
            ),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (
            PullRequestError::GitHubApi("GitHub API returned 302 Found".into()),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (PullRequestError::refused(302, ""), status_error(302)),
        (
            PullRequestError::refused(400, "Problems parsing JSON"),
            status_error(400),
        ),
        (
            PullRequestError::refused(409, "Git Repository is empty."),
            status_error(409),
        ),
        (
            PullRequestError::refused(
                UNPROCESSABLE_ENTITY,
                "Validation Failed; No commits between main and test-secret",
            ),
            status_error(UNPROCESSABLE_ENTITY),
        ),
        (
            PullRequestError::refused(500, "test-secret"),
            status_error(500),
        ),
        (PullRequestError::refused(503, ""), status_error(503)),
        (
            PullRequestError::RepositoryExists("test-secret".into()),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (
            PullRequestError::NotMergeable("test-secret".into()),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (
            PullRequestError::Protected("test-secret".into()),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (PullRequestError::HeadMoved, UNEXPECTED_RESPONSE.to_string()),
        (
            PullRequestError::AutoMergeNotAllowed,
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (
            PullRequestError::MergeMethodNotAllowed("test-secret".into()),
            UNEXPECTED_RESPONSE.to_string(),
        ),
        (
            PullRequestError::AlreadyMergeable,
            UNEXPECTED_RESPONSE.to_string(),
        ),
    ];
    for (error, expected) in cases {
        let shown = format!("{error:?}");
        let said = described(error);
        assert_eq!(said, expected, "{shown}");
        assert!(!said.contains("test-secret"), "{shown} echoed its payload");
    }
}

#[tokio::test]
async fn an_unreachable_github_reads_as_failed_or_timed_out() {
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let github = served(&format!("http://{address}"));
    assert_eq!(github.resolve(None).await.unwrap_err(), REQUEST_FAILED);
}

#[tokio::test]
async fn default_branch_and_commit_answers_keep_their_site_words() {
    let server = MockServer::start().await;
    let mut github = mocked(&server);
    github.configuration.branch = None;
    for (repository, commit, expected) in [
        (
            json!({"default_branch": "bad..name"}),
            json!({"sha": COMMIT}),
            MISSING_DEFAULT_BRANCH,
        ),
        (json!({}), json!({"sha": COMMIT}), UNEXPECTED_RESPONSE),
        (
            json!({"default_branch": "main"}),
            json!({"sha": "abc123"}),
            MISSING_COMMIT,
        ),
        (
            json!({"default_branch": "main"}),
            json!({}),
            UNEXPECTED_RESPONSE,
        ),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository"))
            .respond_with(ResponseTemplate::new(200).set_body_json(repository.clone()))
            .mount(&server)
            .await;
        mock(&server, "commits/main", commit.clone()).await;
        assert_eq!(
            github.resolve(None).await.unwrap_err(),
            expected,
            "{repository} then {commit}"
        );
    }
}

#[tokio::test]
async fn a_revision_expression_is_refused_before_anything_is_sent() {
    let server = MockServer::start().await;
    mock(&server, "commits/main", json!({"sha": COMMIT})).await;
    for expression in ["main~1", "main^", "HEAD@{1}"] {
        assert_eq!(
            mocked(&server).resolve(Some(expression)).await.unwrap_err(),
            INVALID_REFERENCE,
            "{expression}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_source_without_a_credential_reads_without_an_authorization_header() {
    let server = MockServer::start().await;
    mock_answering(&server, "commits/main", json!({"sha": COMMIT}), None).await;
    mock_green_ci_answering(&server, None).await;
    let github = github_with_token(&server, None);
    assert_eq!(
        github.resolve(None).await.unwrap(),
        ("main".to_string(), COMMIT.to_string())
    );
    assert_eq!(
        github.settled(None).await.unwrap(),
        ("main".to_string(), COMMIT.to_string(), SUCCESS_ASSESSMENT)
    );
    assert_sent_without_authorization(&server).await;
}

#[tokio::test]
async fn a_source_with_an_empty_token_reads_anonymously() {
    let server = MockServer::start().await;
    mock_answering(&server, "commits/main", json!({"sha": COMMIT}), None).await;
    mock_green_ci_answering(&server, None).await;
    let (reference, sha, state) = github_with_token(&server, Some(""))
        .settled(None)
        .await
        .unwrap();
    assert_eq!((reference.as_str(), sha.as_str()), ("main", COMMIT));
    assert_eq!(state, SUCCESS_ASSESSMENT);
    assert_sent_without_authorization(&server).await;
}

#[test]
fn tallied_counts_always_add_up_and_split_running_checks() {
    let evidence = tally(
        [
            "success",
            "failure",
            "queued",
            "in_progress",
            "neutral",
            "success",
        ]
        .into_iter(),
    );
    assert_eq!(evidence.total, 6);
    assert_eq!(evidence.passed, 2);
    assert_eq!(evidence.failed, 1);
    assert_eq!(evidence.running, 2);
    assert_eq!(evidence.queued, 1);
    assert_eq!(evidence.in_progress, 1);
    assert_eq!(evidence.unknown, 1);
    let summary = evidence.normalize();
    assert!(summary.complete, "a tally must survive its own normalizer");
    assert_eq!(summary.state, readiness::CheckState::Failure);
    let absent = tally(std::iter::empty()).normalize();
    assert_eq!(absent.state, readiness::CheckState::None);
    assert_eq!(absent.counts.total, 0);
    let green = tally(["success"].into_iter()).normalize();
    assert_eq!(green.state, readiness::CheckState::Success);
    let skipped = tally(["success", "skipped"].into_iter()).normalize();
    assert_eq!(
        skipped.state,
        readiness::CheckState::Unknown,
        "results outside the known set are not green"
    );
}

#[test]
fn a_configured_review_signal_set_is_validated_when_the_source_is_read() {
    let configuration = |signals: Option<Vec<String>>| Configuration {
        owner: "owner".into(),
        repository: "repository".into(),
        branch: None,
        path: None,
        token: None,
        review_signals: signals,
    };
    let config = crate::state::test_config();
    assert_eq!(
        Github::new(configuration(None), &config)
            .unwrap()
            .signals
            .reviewers(),
        ReviewSignals::recognized().reviewers()
    );
    assert_eq!(
        Github::new(configuration(Some(vec!["greptile".into()])), &config)
            .unwrap()
            .signals
            .reviewers(),
        vec!["greptile-apps".to_string()]
    );
    let error = Github::new(configuration(Some(vec!["sonarcloud".into()])), &config)
        .err()
        .expect("an unrecognised review signal is rejected");
    assert!(error.contains("sonarcloud"), "{error}");
}

#[tokio::test]
async fn a_running_check_does_not_settle_until_it_succeeds() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    Mock::given(path(format!(
        "/repos/owner/repository/commits/{COMMIT}/check-runs"
    )))
    .respond_with(
        ResponseTemplate::new(200)
            .set_body_json(json!({"check_runs": [{"head_sha": COMMIT, "status": "in_progress"}]})),
    )
    .up_to_n_times(1)
    .with_priority(1)
    .mount(&server)
    .await;
    check_runs(
        &server,
        json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "success"}]),
    )
    .await;
    let github = mocked(&server);
    let (reference, sha, running) = github.settled(None).await.unwrap();
    assert_eq!(reference, "main");
    assert_eq!(sha, COMMIT);
    assert_eq!(running, "pending");
    assert!(
        !SETTLED_ASSESSMENTS.contains(&running),
        "a check still running must not end a wait"
    );
    let (_, _, finished) = github.settled(None).await.unwrap();
    assert_eq!(finished, "success");
    assert!(
        SETTLED_ASSESSMENTS.contains(&finished),
        "a successful check must end a wait"
    );
}

#[tokio::test]
async fn a_failed_check_settles_the_wait() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    check_runs(
        &server,
        json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "failure"}]),
    )
    .await;
    let (_, _, state) = mocked(&server).settled(None).await.unwrap();
    assert_eq!(state, "failure");
    assert!(
        SETTLED_ASSESSMENTS.contains(&state),
        "a failed check must end a wait"
    );
}

/// The whole point of the finding: at the Checks level, a run GitHub has
/// completed with a skipped conclusion ends a wait, and one it has only
/// queued does not. The first used to be indistinguishable from a
/// repository with no CI at all.
#[tokio::test]
async fn a_completed_skipped_run_settles_the_wait_and_a_queued_one_does_not() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    check_runs(
        &server,
        json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "skipped"}]),
    )
    .await;
    let (_, _, state) = mocked(&server).settled(None).await.unwrap();
    assert_eq!(state, "skipped");
    assert!(
        SETTLED_ASSESSMENTS.contains(&state),
        "a run that completed and said skipped has reported, so the wait is over"
    );

    let pending = MockServer::start().await;
    commit_and_empty_ci(&pending).await;
    check_runs(
        &pending,
        json!([{"head_sha": COMMIT, "status": "queued", "conclusion": null}]),
    )
    .await;
    let (_, _, state) = mocked(&pending).settled(None).await.unwrap();
    assert!(
        !SETTLED_ASSESSMENTS.contains(&state),
        "a queued run has not reported anything yet: {state}"
    );
}

#[tokio::test]
async fn a_commit_with_no_checks_is_unknown_and_never_settles() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    check_runs(&server, json!([])).await;
    let (_, _, state) = mocked(&server).settled(None).await.unwrap();
    assert_eq!(state, "unknown");
    assert!(
        !SETTLED_ASSESSMENTS.contains(&state),
        "a commit GitHub has registered no run for is not a settled wait"
    );
}

#[tokio::test]
async fn settled_refuses_checks_reported_for_another_commit() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    check_runs(
        &server,
        json!([{"head_sha": BLOB, "status": "completed", "conclusion": "success"}]),
    )
    .await;
    assert!(
        mocked(&server)
            .settled(None)
            .await
            .unwrap_err()
            .contains("different")
    );
}

async fn rate_limited(server: &MockServer, response: ResponseTemplate) {
    Mock::given(path("/repos/owner/repository/commits/main"))
        .respond_with(response)
        .up_to_n_times(1)
        .with_priority(1)
        .mount(server)
        .await;
    mock(server, "commits/main", json!({"sha": COMMIT})).await;
}

#[tokio::test]
async fn a_retry_after_in_seconds_is_honoured_once() {
    let server = MockServer::start().await;
    rate_limited(
        &server,
        ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, "3"),
    )
    .await;
    let started = Instant::now();
    let (_, sha) = mocked(&server).resolve(None).await.unwrap();
    let waited = started.elapsed();
    assert_eq!(sha, COMMIT);
    assert!(
        waited >= Duration::from_secs(3),
        "Retry-After was not honoured, waited {waited:?}"
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "a rate-limited request is retried exactly once, never in a loop"
    );
}

#[tokio::test]
async fn a_retry_after_http_date_is_honoured_once() {
    const DATE_FORMAT: &str = "%a, %d %b %Y %H:%M:%S GMT";
    const DISTANCE_SECONDS: i64 = 4;

    let server = MockServer::start().await;
    let at = (Utc::now() + chrono::Duration::seconds(DISTANCE_SECONDS))
        .format(DATE_FORMAT)
        .to_string();
    // A date names an instant, so the retry is held to that instant and not
    // to an elapsed floor a slow run exhausts before the wait even begins.
    let date = DateTime::parse_from_rfc2822(&at)
        .unwrap()
        .with_timezone(&Utc);
    rate_limited(
        &server,
        ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, at.as_str()),
    )
    .await;
    let (_, sha) = mocked(&server).resolve(None).await.unwrap();
    let retried = Utc::now();
    assert_eq!(sha, COMMIT);
    assert!(
        retried >= date,
        "an HTTP-date Retry-After was not honoured, retried at {retried} before {date}"
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "a rate-limited request is retried exactly once, never in a loop"
    );
}

#[tokio::test]
async fn an_exhausted_rate_limit_is_reported_and_never_retried() {
    let server = MockServer::start().await;
    for status in [403, 429] {
        server.reset().await;
        rate_limited(
            &server,
            ResponseTemplate::new(status)
                .insert_header(RATE_LIMIT_REMAINING_HEADER, "0")
                .insert_header(RETRY_AFTER_HEADER, "1"),
        )
        .await;
        let error = mocked(&server).resolve(None).await.unwrap_err();
        assert_eq!(error, RATE_LIMIT_EXHAUSTED, "HTTP {status}");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "an exhausted quota must not be retried"
        );
    }
}

#[tokio::test]
async fn a_forbidden_response_without_an_exhausted_quota_keeps_its_status() {
    let server = MockServer::start().await;
    mock(&server, "commits/main", json!({"sha": COMMIT})).await;
    Mock::given(path("/repos/owner/repository/commits/main"))
        .respond_with(ResponseTemplate::new(403))
        .with_priority(1)
        .mount(&server)
        .await;
    let error = mocked(&server).resolve(None).await.unwrap_err();
    assert!(error.contains("HTTP 403"), "{error}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_forbidden_response_asking_for_too_long_a_wait_reads_as_rate_limited() {
    let server = MockServer::start().await;
    rate_limited(
        &server,
        ResponseTemplate::new(FORBIDDEN).insert_header(RETRY_AFTER_HEADER, "120"),
    )
    .await;
    let error = mocked(&server).resolve(None).await.unwrap_err();
    assert_eq!(error, status_error(TOO_MANY_REQUESTS));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

async fn retried(retry_after: &str) -> (Result<(String, String), String>, usize) {
    let server = MockServer::start().await;
    rate_limited(
        &server,
        ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, retry_after),
    )
    .await;
    let resolved = mocked(&server).resolve(None).await;
    (resolved, server.received_requests().await.unwrap().len())
}

fn soon(format: &str) -> String {
    (Utc::now() + chrono::Duration::seconds(2))
        .format(format)
        .to_string()
}

#[tokio::test]
async fn retry_after_values_the_crate_will_not_wait_on_fail_at_once() {
    const IMF_FIXDATE: &str = "%a, %d %b %Y %H:%M:%S GMT";
    let distant = (Utc::now() + chrono::Duration::seconds(120))
        .format(IMF_FIXDATE)
        .to_string();
    for value in [
        soon("%a, %d %b %Y %H:%M:%S +0000"),
        "61".to_string(),
        "18446744073709551616".to_string(),
        "Thu, 01 Jan 1969 00:00:00 GMT".to_string(),
        soon(IMF_FIXDATE).to_lowercase().replace("gmt", "GMT"),
        soon("%d %b %Y %H:%M:%S GMT"),
        String::new(),
        "soon".to_string(),
        distant,
    ] {
        let (resolved, requests) = retried(&value).await;
        assert_eq!(
            resolved.unwrap_err(),
            status_error(TOO_MANY_REQUESTS),
            "{value:?}"
        );
        assert_eq!(requests, 1, "{value:?} was waited on");
    }
}

#[tokio::test]
async fn retry_after_dates_in_every_rfc_9110_form_are_waited_on_once() {
    for value in [
        "Sun, 06 Nov 1994 08:48:00 GMT".to_string(),
        " 1 ".to_string(),
        soon("%A, %d-%b-%y %H:%M:%S GMT"),
        soon("%a %b %e %H:%M:%S %Y"),
    ] {
        let (resolved, requests) = retried(&value).await;
        assert_eq!(resolved.unwrap().1, COMMIT, "{value:?}");
        assert_eq!(requests, 2, "{value:?} was not retried exactly once");
    }
}

#[tokio::test]
async fn a_source_token_is_kept_only_as_a_secret() {
    let server = MockServer::start().await;
    let github = mocked(&server);
    assert_eq!(github.configuration.token, None);
    assert_eq!(github.token.expose(), "test-secret");
}

#[test]
fn ci_tokens_come_from_the_record_that_carries_them() {
    let commit = Revision::parse(COMMIT).unwrap();
    let run = WorkflowRun::new(
        NonZeroU64::new(11).unwrap(),
        NonZeroU64::new(1).unwrap(),
        ".github/workflows/ci.yml",
        "push",
        commit.clone(),
        "2026-09-05T00:00:00Z",
        "2026-09-05T00:05:00Z",
        "https://github.com/owner/repository/actions/runs/11",
        "owner/repository",
    );
    assert_eq!(
        run_token(
            &run.clone()
                .with_status("completed")
                .with_conclusion("failure")
        ),
        "failure"
    );
    assert_eq!(run_token(&run.clone().with_status("completed")), "unknown");
    assert_eq!(
        run_token(&run),
        "unknown",
        "a run without a status has not said anything"
    );
    assert_eq!(check_token(&CheckRun::new("queued")), "queued");
    assert_eq!(check_token(&CheckRun::new("in_progress")), "in_progress");
    assert_eq!(
        check_token(&CheckRun::new("completed").with_conclusion("success")),
        "success"
    );
    let statuses = [CommitStatus::new("lint", "pending")];
    assert_eq!(conclusions(&[run], &[], &statuses), ["unknown", "pending"]);
}

#[tokio::test]
async fn job_identity_must_match_the_repository_and_the_requested_job() {
    let server = MockServer::start().await;
    let at = |run: u64, job: u64| -> String {
        format!("https://github.com/owner/repository/actions/runs/{run}/job/{job}")
    };
    let fetched = |id: u64, head_sha: &str, url: String| {
        WorkflowJob::new(
            NonZeroU64::new(id).unwrap(),
            NonZeroU64::new(RUN_ID).unwrap(),
            "build",
            "completed",
            Revision::parse(head_sha).unwrap(),
        )
        .with_conclusion("failure")
        .with_url(url)
    };
    let foreign_repository = fetched(
        JOB_ID,
        COMMIT,
        format!("https://github.com/attacker/private/actions/runs/{RUN_ID}/job/{JOB_ID}"),
    );
    let renumbered = fetched(JOB_ID + 1, COMMIT, at(RUN_ID, JOB_ID));
    let elsewhere = fetched(JOB_ID, BLOB, at(RUN_ID, JOB_ID));
    let unlinked = WorkflowJob::new(
        NonZeroU64::new(JOB_ID).unwrap(),
        NonZeroU64::new(RUN_ID).unwrap(),
        "build",
        "completed",
        Revision::parse(COMMIT).unwrap(),
    );
    for metadata in [foreign_repository, renumbered, elsewhere, unlinked] {
        assert_eq!(
            mocked(&server).verify_job(&metadata, JOB_ID, COMMIT),
            Err(FOREIGN_JOB.to_string())
        );
    }
    assert_eq!(
        mocked(&server).verify_job(&fetched(JOB_ID, COMMIT, at(RUN_ID, JOB_ID)), JOB_ID, COMMIT),
        Ok(RUN_ID)
    );
    assert!(
        mocked(&server)
            .verify_job(
                &fetched(JOB_ID, COMMIT, at(RUN_ID, JOB_ID).to_uppercase()),
                JOB_ID,
                COMMIT
            )
            .is_ok()
    );
    assert!(!is_job_url(
        &format!("https://github.com/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}?x=1"),
        "owner",
        "repository",
        RUN_ID,
        JOB_ID
    ));
    assert!(!is_job_url(
        &format!(
            "https://github.com.evil.test/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}"
        ),
        "owner",
        "repository",
        RUN_ID,
        JOB_ID
    ));
}

#[tokio::test]
async fn check_rows_that_shift_between_pages_never_settle_as_success() {
    let server = MockServer::start().await;
    commit_and_empty_ci(&server).await;
    let first: Vec<Value> = (1..=100)
        .map(|id| {
            json!({"id": id, "head_sha": COMMIT, "status": "completed", "conclusion": "success"})
        })
        .collect();
    Mock::given(path(format!(
        "/repos/owner/repository/commits/{COMMIT}/check-runs"
    )))
    .and(query_param("page", "1"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"check_runs": first})))
    .mount(&server)
    .await;
    Mock::given(path(format!(
        "/repos/owner/repository/commits/{COMMIT}/check-runs"
    )))
    .and(query_param("page", "2"))
    .respond_with(
        ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
            {"id": 100, "head_sha": COMMIT, "status": "completed", "conclusion": "success"}
        ]})),
    )
    .mount(&server)
    .await;
    let github = mocked(&server);

    let (_, _, state) = github.settled(None).await.unwrap();
    assert_eq!(state, PENDING_ASSESSMENT);
    assert!(
        !SETTLED_ASSESSMENTS.contains(&state),
        "a check read twice may have pushed another past the pages read"
    );

    let (evidence, _) = github.check_evidence(COMMIT).await.unwrap();
    assert!(!evidence.complete);
    assert_ne!(evidence.normalize().state, readiness::CheckState::Success);
}

#[tokio::test]
async fn a_release_without_a_usable_identity_fails_closed() {
    let server = MockServer::start().await;
    mock(
        &server,
        "releases",
        json!([{"id": 90, "tag_name": "", "published_at": "2026-09-05T00:00:00Z",
                "html_url": "https://github.com/owner/repository/releases/tag/",
                "draft": false, "prerelease": false}]),
    )
    .await;
    let github = mocked(&server);
    let listed = github
        .service
        .list_releases(&github.repository, &github.token, NonZeroU32::MIN, PER_PAGE)
        .await
        .unwrap();

    assert_eq!(
        github.release_identity(&listed.items[0]).unwrap_err(),
        UNUSABLE_RELEASE
    );
}

#[tokio::test]
async fn review_query_failures_and_mismatched_pull_requests_fail_closed() {
    let server = MockServer::start().await;
    mock_pull(&server, COMMIT).await;
    mock_graphql(
        &server,
        json!({"data": Value::Null, "errors": [{"message": "test-secret"}]}),
    )
    .await;
    let error = assessed_pull(&mocked(&server), 7).await.unwrap_err();
    assert_eq!(error, REVIEW_QUERY_REJECTED);
    assert!(!error.contains("test-secret"));
    server.reset().await;
    mock_pull(&server, COMMIT).await;
    mock_graphql(&server, json!({"data": {"repository": Value::Null}})).await;
    assert_eq!(
        assessed_pull(&mocked(&server), 7).await.unwrap_err(),
        REVIEW_QUERY_REJECTED,
        "a repository GitHub's review query cannot see reads as a rejected query"
    );
    server.reset().await;
    let mut other = pull_detail(COMMIT);
    other["number"] = json!(8);
    mock(&server, "pulls/7", other).await;
    mock_review(&server, true, &summary(5, COMMIT), false).await;
    assert_eq!(
        assessed_pull(&mocked(&server), 7).await.unwrap_err(),
        MISMATCHED_PULL
    );
}

#[tokio::test]
async fn a_pull_request_the_review_query_cannot_find_says_so() {
    for answer in [
        json!({"data": {"repository": {"pullRequest": Value::Null}}}),
        json!({"data": {"repository": {"pullRequest": Value::Null}}, "errors": [{
            "type": "NOT_FOUND",
            "path": ["repository", "pullRequest"],
            "message": "Could not resolve to a PullRequest with the number of 7."
        }]}),
    ] {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(&server, answer.clone()).await;

        assert_eq!(
            assessed_pull(&mocked(&server), 7).await.unwrap_err(),
            MISSING_PULL,
            "{answer}"
        );
    }
}

#[tokio::test]
async fn review_threads_counted_for_another_pull_request_fail_closed() {
    for number in [json!(8), Value::Null] {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        let mut counts = thread_counts(true, false);
        counts["data"]["repository"]["pullRequest"]["number"] = number.clone();
        mock_graphql(&server, counts).await;

        assert_eq!(
            assessed_pull(&mocked(&server), 7).await.unwrap_err(),
            MISMATCHED_PULL,
            "threads numbered {number} are not pull request 7's"
        );
    }
}

async fn review_query_failure(response: ResponseTemplate, readiness: bool) -> (String, usize) {
    let server = MockServer::start().await;
    mock_pull(&server, COMMIT).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(response)
        .mount(&server)
        .await;
    let github = mocked(&server);
    let failed = if readiness {
        assessed_pull(&github, 7).await.map(drop)
    } else {
        github.threads(7).await.map(drop)
    };
    let queries = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/graphql")
        .count();
    (failed.unwrap_err(), queries)
}

#[tokio::test]
async fn a_review_query_github_refuses_over_http_names_the_status() {
    let cases = [
        (
            ResponseTemplate::new(FORBIDDEN).set_body_json(
                json!({"message": "Resource not accessible by integration test-secret"}),
            ),
            status_error(FORBIDDEN),
            1,
        ),
        (
            ResponseTemplate::new(NOT_FOUND).set_body_json(json!({"message": "Not Found"})),
            status_error(NOT_FOUND),
            1,
        ),
        (
            ResponseTemplate::new(TOO_MANY_REQUESTS),
            status_error(TOO_MANY_REQUESTS),
            1,
        ),
        (
            ResponseTemplate::new(TOO_MANY_REQUESTS).insert_header(RETRY_AFTER_HEADER, "1"),
            status_error(TOO_MANY_REQUESTS),
            2,
        ),
        (
            ResponseTemplate::new(TOO_MANY_REQUESTS)
                .insert_header(RATE_LIMIT_REMAINING_HEADER, "0"),
            RATE_LIMIT_EXHAUSTED.to_string(),
            1,
        ),
    ];
    for (response, expected, sent) in cases {
        for readiness in [false, true] {
            let (error, queries) = review_query_failure(response.clone(), readiness).await;
            assert_eq!(error, expected, "readiness={readiness}");
            assert!(!error.contains("test-secret"), "{error}");
            assert_eq!(queries, sent, "readiness={readiness}: {expected}");
        }
    }
}

#[tokio::test]
async fn review_query_errors_github_names_the_kind_of_read_as_that_status() {
    for (kind, status) in [
        ("FORBIDDEN", FORBIDDEN),
        ("NOT_FOUND", NOT_FOUND),
        ("RATE_LIMITED", TOO_MANY_REQUESTS),
    ] {
        let response = ResponseTemplate::new(200).set_body_json(json!({
            "data": Value::Null,
            "errors": [{"type": kind, "path": ["node"], "message": "test-secret"}]
        }));
        for readiness in [false, true] {
            let (error, queries) = review_query_failure(response.clone(), readiness).await;
            assert_eq!(error, status_error(status), "{kind} readiness={readiness}");
            assert_eq!(queries, 1, "{kind} readiness={readiness}");
        }
    }
}

#[tokio::test]
async fn a_workflow_run_listed_on_two_pages_never_settles() {
    let server = MockServer::start().await;
    mock(&server, "commits/main", json!({"sha": COMMIT})).await;
    mock_statuses(&server, json!([])).await;
    check_runs(&server, json!([])).await;
    let page_size = u64::from(PER_PAGE.get());
    let run = |id: u64| {
        json!({"id": id, "workflow_id": id, "name": "CI",
               "path": ".github/workflows/ci.yml", "event": "push",
               "head_sha": COMMIT, "status": "completed", "conclusion": "success",
               "created_at": "2026-09-05T00:00:00Z", "updated_at": "2026-09-05T00:05:00Z",
               "html_url": format!("https://github.com/owner/repository/actions/runs/{id}"),
               "repository": {"full_name": "owner/repository"}})
    };
    let first: Vec<Value> = (1..=page_size).map(run).collect();
    for (page, rows) in [("1", first), ("2", vec![run(page_size)])] {
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/actions/runs"))
            .and(query_param("page", page))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"workflow_runs": rows, "total_count": page_size + 1})),
            )
            .mount(&server)
            .await;
    }

    assert_eq!(
        mocked(&server).settled(None).await.unwrap_err(),
        PARTIAL_WORKFLOW_RUNS,
        "a run GitHub listed twice may have pushed another past the pages read"
    );
}

async fn files_read(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/repos/owner/repository/pulls/7/files")
        .count()
}

#[tokio::test]
async fn a_listing_assessment_never_walks_the_changed_files_and_a_full_one_does() {
    let server = MockServer::start().await;
    mock_pull(&server, COMMIT).await;
    mock_review(&server, true, &summary(5, COMMIT), false).await;
    mock_green_ci(&server).await;
    mock(
        &server,
        "pulls/7/files",
        json!([{"filename": "migrations/0001_init.sql", "status": "modified", "additions": 1, "deletions": 0}]),
    )
    .await;
    let github = mocked(&server);
    let reference = github.reference(7).unwrap();
    let detail = github.pull(&reference).await.unwrap();
    let row = listed(reference, &detail);

    let listing = github
        .assess(&row, Some(detail.clone()), Depth::Listing)
        .await
        .unwrap();
    assert_eq!(files_read(&server).await, 0);
    assert!(listing.risk.presumed(), "{}", listing.risk);

    let full = github
        .assess(&row, Some(detail), Depth::Full)
        .await
        .unwrap();
    assert_eq!(files_read(&server).await, 1);
    assert_eq!(full.risk.touched, 1, "{}", full.risk);
    assert_eq!(listing.assessment, full.assessment);
    assert!(
        listing.assessment.ready,
        "{:?}",
        listing.assessment.blockers
    );
}

#[tokio::test]
async fn an_assessment_names_each_check_it_weighed() {
    let server = MockServer::start().await;
    mock_pull(&server, COMMIT).await;
    mock_review(&server, true, &summary(5, COMMIT), false).await;
    mock_answering(
        &server,
        "actions/runs",
        json!({"workflow_runs": [{"id": 11, "workflow_id": 1, "name": "CI",
               "path": ".github/workflows/ci.yml", "event": "pull_request",
               "head_sha": COMMIT, "status": "completed", "conclusion": "failure",
               "created_at": "2026-09-05T00:00:00Z", "updated_at": "2026-09-05T00:05:00Z",
               "html_url": "https://github.com/owner/repository/actions/runs/11",
               "repository": {"full_name": "owner/repository"}}], "total_count": 1}),
        Some("Bearer test-secret"),
    )
    .await;
    check_runs(
        &server,
        json!([{"id": 21, "name": "test", "head_sha": COMMIT, "status": "in_progress",
                "html_url": "https://github.com/owner/repository/actions/runs/11/job/21",
                "started_at": "2026-09-05T00:01:00Z"}]),
    )
    .await;
    mock_statuses(
        &server,
        json!([{"context": "deploy", "state": "success", "target_url": "javascript:alert(1)",
                "created_at": "2026-09-05T00:02:00Z"}]),
    )
    .await;
    let github = mocked(&server);
    let reference = github.reference(7).unwrap();
    let detail = github.pull(&reference).await.unwrap();

    let assessed = github
        .assess(&listed(reference, &detail), Some(detail), Depth::Listing)
        .await
        .unwrap();

    let named: Vec<(CheckKind, u64, &str, readiness::CheckState, Option<&str>)> = assessed
        .checks
        .iter()
        .map(|check| {
            (
                check.kind,
                check.id,
                check.name.as_str(),
                check.state,
                check.url.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            (
                CheckKind::Workflow,
                11,
                "CI",
                readiness::CheckState::Failure,
                Some("https://github.com/owner/repository/actions/runs/11"),
            ),
            (
                CheckKind::CheckRun,
                21,
                "test",
                readiness::CheckState::Pending,
                Some("https://github.com/owner/repository/actions/runs/11/job/21"),
            ),
            (
                CheckKind::Status,
                1,
                "deploy",
                readiness::CheckState::Success,
                None
            ),
        ]
    );
    assert_eq!(
        assessed.checks[1].updated_at.as_deref(),
        Some("2026-09-05T00:01:00Z")
    );
}
