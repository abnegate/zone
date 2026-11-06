use sqlx::PgPool;

use super::*;
use crate::db::chats::create_chat;

async fn pool() -> PgPool {
    PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
        .await
        .expect("the test database accepts connections")
}

async fn chat(pool: &PgPool) -> Uuid {
    create_chat(pool, None, "Subagents", "sonnet", false, true)
        .await
        .expect("a chat")
        .id
}

fn named(chat_id: Uuid, external_id: &str) -> SubagentChange<'_> {
    SubagentChange {
        chat_id,
        external_id,
        ..SubagentChange::default()
    }
}

fn step(id: &str, status: StepStatus) -> SubagentStep {
    SubagentStep {
        id: id.to_string(),
        name: "Bash".to_string(),
        subject: Some("cargo test".to_string()),
        status,
    }
}

async fn running(pool: &PgPool, chat_id: Uuid, external_id: &str, turn: Option<Uuid>) -> Uuid {
    record(
        pool,
        &SubagentChange {
            message_id: turn,
            ..named(chat_id, external_id)
        },
    )
    .await
    .expect("a running subagent")
    .id
}

async fn statuses(pool: &PgPool, chat_id: Uuid) -> Vec<(String, String)> {
    list(pool, chat_id)
        .await
        .expect("the list")
        .into_iter()
        .map(|row| (row.external_id, row.status))
        .collect()
}

async fn stored(pool: &PgPool, chat_id: Uuid, subagent_id: Uuid) -> Vec<SubagentStep> {
    steps(pool, chat_id, subagent_id)
        .await
        .expect("the steps")
        .expect("the subagent")
}

#[tokio::test]
async fn a_new_subagent_takes_the_columns_defaults() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;

    let row = record(&pool, &named(chat_id, "toolu_01Agent"))
        .await
        .expect("a subagent");

    assert_eq!(row.chat_id, chat_id);
    assert_eq!(row.message_id, None);
    assert_eq!(row.external_id, "toolu_01Agent");
    assert_eq!(row.description, "");
    assert!(!row.background);
    assert_eq!(row.status, SubagentStatus::Running.as_str());
    assert_eq!((row.step_count, row.revision), (0, 0));
    assert_eq!(row.finished_at, None);
    assert_eq!(
        (
            row.kind,
            row.model,
            row.tool_uses,
            row.duration_ms,
            row.tokens
        ),
        (None, None, None, None, None)
    );
    assert_eq!((row.last_tool, row.summary, row.error), (None, None, None));
}

#[tokio::test]
async fn an_update_changes_only_what_it_names_and_bumps_the_revision() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let turn = Uuid::new_v4();
    let first = record(
        &pool,
        &SubagentChange {
            message_id: Some(turn),
            description: Some("Ask for a review"),
            kind: Some("general-purpose"),
            model: Some("sonnet"),
            background: Some(true),
            tool_uses: Some(3),
            ..named(chat_id, "toolu_01Agent")
        },
    )
    .await
    .expect("the first update");

    let second = record(
        &pool,
        &SubagentChange {
            tokens: Some(18_000),
            last_tool: Some("Bash"),
            ..named(chat_id, "toolu_01Agent")
        },
    )
    .await
    .expect("the second update");

    assert_eq!(second.id, first.id);
    assert_eq!(second.message_id, Some(turn));
    assert_eq!(second.description, "Ask for a review");
    assert_eq!(second.kind.as_deref(), Some("general-purpose"));
    assert_eq!(second.model.as_deref(), Some("sonnet"));
    assert!(second.background);
    assert_eq!(second.tool_uses, Some(3));
    assert_eq!(second.tokens, Some(18_000));
    assert_eq!(second.last_tool.as_deref(), Some("Bash"));
    assert_eq!(second.status, SubagentStatus::Running.as_str());
    assert_eq!(second.revision, first.revision + 1);
    assert!(second.updated_at > first.updated_at);
    assert_eq!(list(&pool, chat_id).await.expect("the list"), [second]);
}

#[tokio::test]
async fn concurrent_writers_end_on_the_last_commit_with_a_rising_revision() {
    const WRITES: i64 = 25;
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    running(&pool, chat_id, "toolu_01Agent", None).await;

    let writer = |offset: i64| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut written = Vec::new();
            for write in 0..WRITES {
                let row = record(
                    &pool,
                    &SubagentChange {
                        tool_uses: Some(offset + write),
                        ..named(chat_id, "toolu_01Agent")
                    },
                )
                .await
                .expect("a write");
                written.push((row.revision, row.tool_uses, row.updated_at));
            }
            written
        })
    };
    let (one, other) = tokio::join!(writer(0), writer(1_000));
    let one = one.expect("the first writer");
    let other = other.expect("the second writer");

    for written in [&one, &other] {
        assert!(
            written.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "each writer sees its own writes in order: {written:?}"
        );
    }
    let mut revisions: Vec<i64> = one.iter().chain(&other).map(|write| write.0).collect();
    revisions.sort_unstable();
    assert_eq!(
        revisions,
        (1..=2 * WRITES).collect::<Vec<_>>(),
        "every write has a revision of its own"
    );
    let last = one
        .iter()
        .chain(&other)
        .max_by_key(|write| write.0)
        .expect("a last write");
    let row = list(&pool, chat_id).await.expect("the list").remove(0);
    assert_eq!(row.revision, 2 * WRITES);
    assert_eq!(row.tool_uses, last.1, "the row holds the last commit");
    assert_eq!(row.updated_at, last.2);
    let mut by_revision: Vec<_> = one.iter().chain(&other).collect();
    by_revision.sort_by_key(|write| write.0);
    assert!(
        by_revision.windows(2).all(|pair| pair[0].2 <= pair[1].2),
        "a later revision is never stamped earlier"
    );
}

#[tokio::test]
async fn the_turn_that_first_named_a_subagent_keeps_it() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let first = Uuid::new_v4();
    running(&pool, chat_id, "thread-1", Some(first)).await;

    let row = record(
        &pool,
        &SubagentChange {
            message_id: Some(Uuid::new_v4()),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("a later update");

    assert_eq!(row.message_id, Some(first));
}

#[tokio::test]
async fn a_turn_names_a_subagent_first_heard_of_without_one() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let turn = Uuid::new_v4();
    running(&pool, chat_id, "thread-1", None).await;

    let row = record(
        &pool,
        &SubagentChange {
            message_id: Some(turn),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("a later update");

    assert_eq!(row.message_id, Some(turn));
}

#[tokio::test]
async fn a_finished_subagent_never_runs_again_and_finishes_once() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    running(&pool, chat_id, "toolu_01Agent", None).await;
    let completed = record(
        &pool,
        &SubagentChange {
            status: Some(SubagentStatus::Completed),
            summary: Some("Looks good"),
            ..named(chat_id, "toolu_01Agent")
        },
    )
    .await
    .expect("the completion");
    let finished = completed.finished_at.expect("a finish time");

    for status in [SubagentStatus::Running, SubagentStatus::Stopped] {
        let row = record(
            &pool,
            &SubagentChange {
                status: Some(status),
                ..named(chat_id, "toolu_01Agent")
            },
        )
        .await
        .expect("a late update");
        assert_eq!(row.status, SubagentStatus::Completed.as_str(), "{status:?}");
        assert_eq!(row.finished_at, Some(finished), "{status:?}");
    }
    assert_eq!(completed.summary.as_deref(), Some("Looks good"));
}

#[tokio::test]
async fn a_reported_outcome_replaces_a_stop_but_keeps_its_finish_time() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let stopped = record(
        &pool,
        &SubagentChange {
            status: Some(SubagentStatus::Stopped),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("a stopped subagent");

    let failed = record(
        &pool,
        &SubagentChange {
            status: Some(SubagentStatus::Failed),
            error: Some("exit 1"),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("its failure");

    assert_eq!(failed.status, SubagentStatus::Failed.as_str());
    assert_eq!(failed.error.as_deref(), Some("exit 1"));
    assert!(stopped.finished_at.is_some());
    assert_eq!(failed.finished_at, stopped.finished_at);
}

#[tokio::test]
async fn a_subagent_first_heard_of_finished_is_finished_now() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;

    let row = record(
        &pool,
        &SubagentChange {
            status: Some(SubagentStatus::Completed),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("a finished subagent");

    assert_eq!(row.status, SubagentStatus::Completed.as_str());
    assert!(row.finished_at.is_some());
}

#[tokio::test]
async fn the_same_name_in_another_chat_is_another_subagent() {
    let pool = pool().await;
    let first = chat(&pool).await;
    let second = chat(&pool).await;

    let one = record(&pool, &named(first, "toolu_01Agent"))
        .await
        .expect("one");
    let other = record(&pool, &named(second, "toolu_01Agent"))
        .await
        .expect("the other");

    assert_ne!(one.id, other.id);
    assert_eq!(list(&pool, first).await.expect("the list"), [one]);
}

#[tokio::test]
async fn a_chat_lists_its_subagents_oldest_first_with_their_step_counts() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let newer = running(&pool, chat_id, "newer", None).await;
    running(&pool, chat_id, "older", None).await;
    sqlx::query(
        "UPDATE chat_subagents SET started_at = started_at - INTERVAL '1 minute'
         WHERE chat_id = $1 AND external_id = 'older'",
    )
    .bind(chat_id)
    .execute(&pool)
    .await
    .expect("an older subagent");
    record_steps(
        &pool,
        newer,
        &[
            step("one", StepStatus::Running),
            step("two", StepStatus::Running),
        ],
    )
    .await
    .expect("the newer subagent's steps");

    let listed: Vec<(String, i64)> = list(&pool, chat_id)
        .await
        .expect("the list")
        .into_iter()
        .map(|row| (row.external_id, row.step_count))
        .collect();

    assert_eq!(listed, [("older".to_string(), 0), ("newer".to_string(), 2)]);
}

#[tokio::test]
async fn new_steps_go_last_in_the_order_given_and_return_as_added() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let subagent = running(&pool, chat_id, "toolu_01Agent", None).await;
    let first = [
        step("one", StepStatus::Running),
        step("two", StepStatus::Running),
    ];
    let opening = record_steps(&pool, subagent, &first)
        .await
        .expect("the first steps");

    let next = record_steps(
        &pool,
        subagent,
        &[
            step("three", StepStatus::Running),
            step("one", StepStatus::Succeeded),
            step("four", StepStatus::Running),
        ],
    )
    .await
    .expect("the next steps");

    assert_eq!(
        opening,
        RecordedSteps {
            changed: first.to_vec(),
            added: 2,
        }
    );
    assert_eq!(
        next,
        RecordedSteps {
            changed: vec![
                step("one", StepStatus::Succeeded),
                step("three", StepStatus::Running),
                step("four", StepStatus::Running),
            ],
            added: 2,
        },
        "a known step keeps its place"
    );
    let positions: Vec<(String, i64)> = sqlx::query_as(
        "SELECT step_id, position FROM chat_subagent_steps WHERE subagent_id = $1 ORDER BY position",
    )
    .bind(subagent)
    .fetch_all(&pool)
    .await
    .expect("the positions");
    assert_eq!(
        positions,
        [("one", 0), ("two", 1), ("three", 2), ("four", 3)]
            .map(|(id, position)| (id.to_string(), position))
    );
}

#[tokio::test]
async fn a_settled_step_never_runs_again_and_an_unchanged_one_is_not_returned() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let subagent = running(&pool, chat_id, "toolu_01Agent", None).await;
    record_steps(
        &pool,
        subagent,
        &[
            step("done", StepStatus::Succeeded),
            step("open", StepStatus::Running),
        ],
    )
    .await
    .expect("the steps");

    let again = record_steps(
        &pool,
        subagent,
        &[
            step("done", StepStatus::Running),
            step("open", StepStatus::Running),
        ],
    )
    .await
    .expect("a repeat");

    assert_eq!(again, RecordedSteps::default());
    assert_eq!(
        stored(&pool, chat_id, subagent).await,
        [
            step("done", StepStatus::Succeeded),
            step("open", StepStatus::Running),
        ]
    );
}

#[tokio::test]
async fn a_step_named_twice_in_one_write_keeps_the_later() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let subagent = running(&pool, chat_id, "toolu_01Agent", None).await;

    let written = record_steps(
        &pool,
        subagent,
        &[
            step("one", StepStatus::Running),
            step("one", StepStatus::Failed),
        ],
    )
    .await
    .expect("the steps");

    assert_eq!(
        written,
        RecordedSteps {
            changed: vec![step("one", StepStatus::Failed)],
            added: 1,
        }
    );
}

#[tokio::test]
async fn a_subagents_steps_are_read_only_through_its_own_chat() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let other = chat(&pool).await;
    let subagent = running(&pool, chat_id, "toolu_01Agent", None).await;
    let empty = running(&pool, chat_id, "toolu_02Agent", None).await;
    record_steps(&pool, subagent, &[step("one", StepStatus::Running)])
        .await
        .expect("a step");

    assert_eq!(
        stored(&pool, chat_id, subagent).await,
        [step("one", StepStatus::Running)]
    );
    assert_eq!(stored(&pool, chat_id, empty).await, []);
    assert_eq!(steps(&pool, other, subagent).await.expect("a read"), None);
    assert_eq!(
        steps(&pool, chat_id, Uuid::new_v4()).await.expect("a read"),
        None
    );
}

#[tokio::test]
async fn stopping_a_turn_stops_only_its_running_subagents_and_fails_their_running_steps() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let other = chat(&pool).await;
    let turn = Uuid::new_v4();
    let earlier = Uuid::new_v4();
    let mine = running(&pool, chat_id, "mine", Some(turn)).await;
    running(&pool, chat_id, "earlier", Some(earlier)).await;
    running(&pool, chat_id, "unnamed", None).await;
    running(&pool, other, "elsewhere", Some(turn)).await;
    record(
        &pool,
        &SubagentChange {
            message_id: Some(turn),
            status: Some(SubagentStatus::Completed),
            ..named(chat_id, "done")
        },
    )
    .await
    .expect("a completed subagent");
    record_steps(
        &pool,
        mine,
        &[
            step("finished", StepStatus::Succeeded),
            step("open", StepStatus::Running),
        ],
    )
    .await
    .expect("its steps");

    let stopped = stop(&pool, chat_id, turn).await.expect("the stop");

    assert_eq!(stopped.len(), 1, "{stopped:?}");
    let Stopped { subagent, failed } = &stopped[0];
    assert_eq!(subagent.external_id, "mine");
    assert_eq!(subagent.status, SubagentStatus::Stopped.as_str());
    assert_eq!((subagent.step_count, subagent.revision), (2, 1));
    assert!(subagent.finished_at.is_some());
    assert_eq!(failed.0, [step("open", StepStatus::Failed)]);
    assert_eq!(
        stored(&pool, chat_id, mine).await,
        [
            step("finished", StepStatus::Succeeded),
            step("open", StepStatus::Failed),
        ]
    );
    let mut seen = statuses(&pool, chat_id).await;
    seen.sort();
    assert_eq!(
        seen,
        [
            ("done".to_string(), "completed".to_string()),
            ("earlier".to_string(), "running".to_string()),
            ("mine".to_string(), "stopped".to_string()),
            ("unnamed".to_string(), "running".to_string()),
        ]
    );
    assert_eq!(
        statuses(&pool, other).await,
        [("elsewhere".to_string(), "running".to_string())]
    );
}

#[tokio::test]
async fn starting_a_turn_stops_the_running_subagents_of_every_other_turn() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let other = chat(&pool).await;
    let turn = Uuid::new_v4();
    let earlier = Uuid::new_v4();
    let mine = running(&pool, chat_id, "mine", Some(turn)).await;
    let left = running(&pool, chat_id, "earlier", Some(earlier)).await;
    running(&pool, chat_id, "unnamed", None).await;
    running(&pool, other, "elsewhere", Some(earlier)).await;
    record(
        &pool,
        &SubagentChange {
            message_id: Some(earlier),
            status: Some(SubagentStatus::Failed),
            ..named(chat_id, "failed")
        },
    )
    .await
    .expect("a failed subagent");
    for subagent in [mine, left] {
        record_steps(&pool, subagent, &[step("open", StepStatus::Running)])
            .await
            .expect("a running step");
    }

    let stopped = stop_others(&pool, chat_id, turn).await.expect("the sweep");

    let mut names: Vec<&str> = stopped
        .iter()
        .map(|stopped| stopped.subagent.external_id.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["earlier", "unnamed"]);
    assert!(stopped.iter().all(|stopped| stopped.subagent.status
        == SubagentStatus::Stopped.as_str()
        && stopped.subagent.finished_at.is_some()));
    assert_eq!(
        stored(&pool, chat_id, left).await,
        [step("open", StepStatus::Failed)]
    );
    assert_eq!(
        stored(&pool, chat_id, mine).await,
        [step("open", StepStatus::Running)],
        "the starting turn's own steps run on"
    );
    let mut seen = statuses(&pool, chat_id).await;
    seen.sort();
    assert_eq!(
        seen,
        [
            ("earlier".to_string(), "stopped".to_string()),
            ("failed".to_string(), "failed".to_string()),
            ("mine".to_string(), "running".to_string()),
            ("unnamed".to_string(), "stopped".to_string()),
        ]
    );
    assert_eq!(
        statuses(&pool, other).await,
        [("elsewhere".to_string(), "running".to_string())]
    );
}

#[tokio::test]
async fn removing_a_messages_subagents_takes_their_steps_and_leaves_the_rest() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let message = Uuid::new_v4();
    let gone = running(&pool, chat_id, "gone", Some(message)).await;
    let kept = running(&pool, chat_id, "kept", Some(Uuid::new_v4())).await;
    record_steps(&pool, gone, &[step("one", StepStatus::Running)])
        .await
        .expect("a step");

    let removed = remove(&pool, chat_id, message).await.expect("the removal");

    assert_eq!(removed, [gone]);
    let left: Vec<Uuid> = list(&pool, chat_id)
        .await
        .expect("the list")
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(left, [kept]);
    let orphans: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_subagent_steps WHERE subagent_id = $1")
            .bind(gone)
            .fetch_one(&pool)
            .await
            .expect("the count");
    assert_eq!(orphans, 0);
}
