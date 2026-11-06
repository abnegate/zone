use sqlx::{PgPool, Row};

use super::*;
use crate::db::chats::create_chat;

async fn pool() -> PgPool {
    PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
        .await
        .expect("the test database accepts connections")
}

const TURN_COMPLETED: &str = "completed";

/// A turn of `chat_id` in `status`, as the chat's store leaves it.
async fn turn(pool: &PgPool, chat_id: Uuid, status: &str) -> Uuid {
    let message = Uuid::new_v4();
    let turn = Uuid::new_v4();
    sqlx::query("INSERT INTO messages (id, chat_id, role, content) VALUES ($1, $2, 'user', 'Go')")
        .bind(message)
        .bind(chat_id)
        .execute(pool)
        .await
        .expect("a user message");
    sqlx::query(
        "INSERT INTO chat_turns (id, chat_id, user_message_id, fence, status) VALUES ($1, $2, $3, 1, $4)",
    )
    .bind(turn)
    .bind(chat_id)
    .bind(message)
    .bind(status)
    .execute(pool)
    .await
    .expect("a turn");
    turn
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
async fn a_step_that_arrives_after_its_subagent_stopped_is_failed() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    for status in [
        SubagentStatus::Stopped,
        SubagentStatus::Completed,
        SubagentStatus::Failed,
    ] {
        let external_id = status.as_str();
        let subagent = running(&pool, chat_id, external_id, None).await;
        record_steps(
            &pool,
            subagent,
            &[
                step("finished", StepStatus::Succeeded),
                step("open", StepStatus::Running),
            ],
        )
        .await
        .expect("its steps");
        record(
            &pool,
            &SubagentChange {
                status: Some(status),
                ..named(chat_id, external_id)
            },
        )
        .await
        .expect("its end");

        let late = record_steps(
            &pool,
            subagent,
            &[
                step("finished", StepStatus::Running),
                step("open", StepStatus::Running),
                step("late", StepStatus::Running),
                step("landed", StepStatus::Succeeded),
            ],
        )
        .await
        .expect("the late steps");

        assert_eq!(
            late,
            RecordedSteps {
                changed: vec![
                    step("open", StepStatus::Failed),
                    step("late", StepStatus::Failed),
                    step("landed", StepStatus::Succeeded),
                ],
                added: 2,
            },
            "{status:?}"
        );
        assert_eq!(
            stored(&pool, chat_id, subagent).await,
            [
                step("finished", StepStatus::Succeeded),
                step("open", StepStatus::Failed),
                step("late", StepStatus::Failed),
                step("landed", StepStatus::Succeeded),
            ],
            "{status:?}"
        );
    }
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
    let turn = turn(&pool, chat_id, TURN_RUNNING).await;
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
async fn a_turn_that_no_longer_runs_stops_no_other_turns_subagents() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let ended = turn(&pool, chat_id, TURN_COMPLETED).await;
    let newer = turn(&pool, chat_id, TURN_RUNNING).await;
    let subagent = running(&pool, chat_id, "newer", Some(newer)).await;
    record_steps(&pool, subagent, &[step("open", StepStatus::Running)])
        .await
        .expect("a running step");

    let stopped = stop_others(&pool, chat_id, ended).await.expect("the sweep");

    assert_eq!(stopped, []);
    assert_eq!(
        statuses(&pool, chat_id).await,
        [("newer".to_string(), "running".to_string())]
    );
    assert_eq!(
        stored(&pool, chat_id, subagent).await,
        [step("open", StepStatus::Running)],
        "a newer turn's steps run on"
    );
}

#[tokio::test]
async fn another_turn_reporting_a_finished_subagent_running_starts_a_new_run_of_it() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    for (external_id, outcome) in [
        ("thread-completed", SubagentStatus::Completed),
        ("thread-failed", SubagentStatus::Failed),
        ("thread-stopped", SubagentStatus::Stopped),
    ] {
        running(&pool, chat_id, external_id, Some(first)).await;
        let finished = record(
            &pool,
            &SubagentChange {
                message_id: Some(first),
                status: Some(outcome),
                summary: Some("Looks good"),
                error: Some("exit 1"),
                tool_uses: Some(4),
                ..named(chat_id, external_id)
            },
        )
        .await
        .expect("the first run's outcome");

        let resumed = record(
            &pool,
            &SubagentChange {
                message_id: Some(second),
                status: Some(SubagentStatus::Running),
                ..named(chat_id, external_id)
            },
        )
        .await
        .expect("the second run");

        assert_eq!(resumed.id, finished.id, "{outcome:?}");
        assert_eq!(resumed.message_id, Some(second), "{outcome:?}");
        assert_eq!(
            resumed.status,
            SubagentStatus::Running.as_str(),
            "{outcome:?}"
        );
        assert_eq!(resumed.finished_at, None, "{outcome:?}");
        assert!(resumed.started_at > finished.started_at, "{outcome:?}");
        assert_eq!(
            (resumed.summary, resumed.error, resumed.tool_uses),
            (None, None, None),
            "{outcome:?}"
        );

        let late = record(
            &pool,
            &SubagentChange {
                message_id: Some(first),
                status: Some(SubagentStatus::Running),
                ..named(chat_id, external_id)
            },
        )
        .await
        .expect("a late update of the first run");
        assert_eq!(late.message_id, Some(second), "{outcome:?}");
        assert_eq!(late.status, SubagentStatus::Running.as_str(), "{outcome:?}");
    }
}

#[tokio::test]
async fn the_turn_that_ran_a_finished_subagent_never_runs_it_again() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let first = Uuid::new_v4();
    running(&pool, chat_id, "thread-1", Some(first)).await;
    let completed = record(
        &pool,
        &SubagentChange {
            message_id: Some(first),
            status: Some(SubagentStatus::Completed),
            summary: Some("Looks good"),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("its completion");

    let late = record(
        &pool,
        &SubagentChange {
            message_id: Some(first),
            status: Some(SubagentStatus::Running),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("a late update");

    assert_eq!(late.status, SubagentStatus::Completed.as_str());
    assert_eq!(late.finished_at, completed.finished_at);
    assert_eq!(late.summary.as_deref(), Some("Looks good"));
}

#[tokio::test]
async fn failing_a_finished_subagents_running_steps_returns_them_in_order() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let subagent = running(&pool, chat_id, "toolu_01Agent", None).await;
    record_steps(
        &pool,
        subagent,
        &[
            step("second", StepStatus::Running),
            step("done", StepStatus::Succeeded),
            step("third", StepStatus::Running),
        ],
    )
    .await
    .expect("its steps");

    let failed = fail_running_steps(&pool, subagent)
        .await
        .expect("the failure");

    assert_eq!(
        failed,
        [
            step("second", StepStatus::Failed),
            step("third", StepStatus::Failed),
        ]
    );
    assert_eq!(
        stored(&pool, chat_id, subagent).await,
        [
            step("second", StepStatus::Failed),
            step("done", StepStatus::Succeeded),
            step("third", StepStatus::Failed),
        ]
    );
    assert_eq!(
        fail_running_steps(&pool, subagent)
            .await
            .expect("a second failure"),
        []
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

async fn generic_plan(pool: &PgPool, statement: &str) -> String {
    let mut transaction = pool.begin().await.expect("a transaction");
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *transaction)
        .await
        .expect("sequential scans off");
    let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "EXPLAIN (GENERIC_PLAN) {statement}"
    )))
    .fetch_all(&mut *transaction)
    .await
    .expect("the plan");
    let lines: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
    lines.join("\n")
}

#[tokio::test]
async fn the_running_sweeps_can_use_the_running_index_whatever_they_are_bound_to() {
    let pool = pool().await;
    for statement in [STOP, STOP_OTHERS, ABANDON] {
        let plan = generic_plan(&pool, statement).await;
        assert!(
            plan.contains("chat_subagents_running_idx"),
            "a sweep's generic plan should reach the running index:\n{plan}"
        );
    }
}

/// Waits until some other session is waiting on a lock session `pid` holds.
async fn blocking(pool: &PgPool, pid: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .expect("the lock waits");
            if blocked {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a session waiting on the lock in time");
}

#[tokio::test]
async fn a_starting_turns_sweep_begun_before_a_newer_turn_took_a_subagent_leaves_that_run_alone() {
    let pool = pool().await;
    let chat_id = chat(&pool).await;
    let sweeping = turn(&pool, chat_id, TURN_RUNNING).await;
    let earlier = Uuid::new_v4();
    let newer = Uuid::new_v4();
    let subagent = running(&pool, chat_id, "thread-1", Some(earlier)).await;
    let mut successor = pool.begin().await.expect("a transaction");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *successor)
        .await
        .expect("its session");
    sqlx::query("SELECT 1 FROM chat_subagents WHERE id = $1 FOR UPDATE")
        .bind(subagent)
        .execute(&mut *successor)
        .await
        .expect("the subagent held");
    let sweep = tokio::spawn({
        let pool = pool.clone();
        async move { stop_others(&pool, chat_id, sweeping).await }
    });
    blocking(&pool, pid).await;

    record(
        &mut *successor,
        &SubagentChange {
            message_id: Some(earlier),
            status: Some(SubagentStatus::Completed),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("the earlier run's outcome");
    sqlx::query("UPDATE chat_turns SET status = $2 WHERE id = $1")
        .bind(sweeping)
        .bind(TURN_COMPLETED)
        .execute(&mut *successor)
        .await
        .expect("the sweeping turn over");
    let resumed = record(
        &mut *successor,
        &SubagentChange {
            message_id: Some(newer),
            status: Some(SubagentStatus::Running),
            ..named(chat_id, "thread-1")
        },
    )
    .await
    .expect("the newer turn's run");
    record_steps(
        &mut *successor,
        subagent,
        &[step("open", StepStatus::Running)],
    )
    .await
    .expect("the newer run's step");
    successor.commit().await.expect("the newer turn's commit");
    let stopped = sweep.await.expect("the sweep ran").expect("the sweep");

    assert_eq!(resumed.message_id, Some(newer));
    assert_eq!(stopped, [], "the run the sweep saw had already ended");
    assert_eq!(
        statuses(&pool, chat_id).await,
        [("thread-1".to_string(), "running".to_string())]
    );
    assert_eq!(
        stored(&pool, chat_id, subagent).await,
        [step("open", StepStatus::Running)],
        "the newer run's steps run on"
    );
}
