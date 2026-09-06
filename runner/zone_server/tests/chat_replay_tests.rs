//! Durable replay and productive-loop acceptance through real WebSocket turns.
mod common;

use axum::http::StatusCode;
use common::context::{
    Harness, answer, calls, finish, next, ordinary, pairs, send, successful, tool,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;
use wiremock::{
    Mock, Request,
    matchers::{method, path},
};

#[tokio::test]
async fn fresh_seven_large_results_and_errors_survive_restart_and_text_agent_transitions() {
    let mut harness = Harness::new(
        Some(200_000),
        false,
        vec![answer("Initial plaintext reply")],
    )
    .await;
    successful(&harness.turn("Plain text before tools").await);
    harness
        .client
        .put_json_auth(
            &format!("/api/chats/{}", harness.chat),
            &json!({"agent_enabled":true}),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::OK);
    let bodies: Vec<_> = (0..7)
        .map(|index| {
            format!(
                "EVIDENCE_{index}\n{}\nSUFFIX_{index}",
                "line with important detail\n".repeat(400)
            )
        })
        .collect();
    let paths: Vec<_> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| harness.file(&format!("file-{index}.txt"), body))
        .collect();
    let batch = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            tool(
                &format!("provider-{index}"),
                "read_file",
                json!({"path":path}),
            )
        })
        .collect();
    harness.script.push(calls("INTERMEDIATE_PROSE_ONCE", batch));
    harness.script.push(calls(
        "SECOND_INTERMEDIATE_PROSE_ONCE",
        vec![tool(
            "failure",
            "read_file",
            json!({"path":harness.directory().join("missing.txt")}),
        )],
    ));
    harness.script.push(answer("Final tool answer"));
    successful(
        &harness
            .turn("Inspect all seven files, then the missing file")
            .await,
    );
    let requests = harness.requests().await;
    let requests = ordinary(&requests);
    assert_eq!(requests.len(), 4);
    assert_eq!(pairs(requests[2]), 7);
    for body in &bodies {
        assert!(
            requests[2]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool" && message["content"] == *body),
            "fresh evidence truncated"
        );
    }
    assert_eq!(pairs(requests[3]), 8);
    let history = harness.history().await;
    assert_eq!(
        history
            .entries
            .iter()
            .filter(|entry| entry.message.role == zone_core::llm::Role::Tool)
            .count(),
        8
    );
    let error = history
        .entries
        .iter()
        .find(|entry| {
            entry.message.role == zone_core::llm::Role::Tool
                && entry
                    .message
                    .content
                    .as_deref()
                    .is_some_and(|content| content.starts_with("Error:"))
        })
        .unwrap()
        .message
        .content
        .clone()
        .unwrap();
    let snapshot: Vec<_> = history
        .entries
        .iter()
        .map(|entry| {
            (
                entry.id.clone(),
                serde_json::to_value(&entry.message).unwrap(),
            )
        })
        .collect();
    harness.restart().await;
    harness
        .client
        .put_json_auth(
            &format!("/api/chats/{}", harness.chat),
            &json!({"agent_enabled":false}),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::OK);
    harness.script.push(answer("Next plaintext answer"));
    successful(
        &harness
            .turn("Continue using the established evidence")
            .await,
    );
    let requests = harness.requests().await;
    let next = ordinary(&requests).last().unwrap().to_owned();
    assert_eq!(pairs(next), 8);
    assert!(
        next["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == error)
    );
    for prose in ["INTERMEDIATE_PROSE_ONCE", "SECOND_INTERMEDIATE_PROSE_ONCE"] {
        assert_eq!(
            next["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["content"] == prose)
                .count(),
            1
        );
    }
    let history = harness.history().await;
    for (id, body) in snapshot {
        assert_eq!(
            serde_json::to_value(
                &history
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .unwrap()
                    .message
            )
            .unwrap(),
            body
        );
    }
}

#[tokio::test]
async fn consumed_active_turn_group_compacts_atomically_while_new_result_and_user_remain_verbatim()
{
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    let first = format!("FIRST_ORIGINAL\n{}\nFIRST_TAIL", "a".repeat(80_000));
    let second = format!("SECOND_ORIGINAL\n{}\nSECOND_TAIL", "b".repeat(80_000));
    let first_path = harness.file("first.txt", &first);
    let second_path = harness.file("second.txt", &second);
    harness.script.push(calls(
        "First read",
        vec![tool("a", "read_file", json!({"path":first_path}))],
    ));
    harness.script.push(calls(
        "Second read",
        vec![tool("b", "read_file", json!({"path":second_path}))],
    ));
    harness.script.push(answer("Finished the pair"));
    let user = "ACTUAL_CURRENT_USER: preserve both results and corrections";
    successful(&harness.turn(user).await);
    let requests = harness.requests().await;
    let requests = ordinary(&requests);
    assert_eq!(requests.len(), 3);
    assert!(
        requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == first)
    );
    assert!(
        requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == second)
    );
    assert!(
        requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == user)
    );
    let history = harness.history().await;
    let checkpoint = history
        .summary
        .as_ref()
        .expect("active-turn consumed group should compact");
    let first_result = history
        .entries
        .iter()
        .find(|entry| entry.message.content.as_deref() == Some(&first))
        .unwrap();
    let second_result = history
        .entries
        .iter()
        .find(|entry| entry.message.content.as_deref() == Some(&second))
        .unwrap();
    assert!(checkpoint.entries.contains(&first_result.id));
    assert!(!checkpoint.entries.contains(&second_result.id));
    assert!(
        !checkpoint
            .entries
            .contains(history.latest_user.as_ref().unwrap())
    );
    zone_server::services::chat::history::validate(&history, checkpoint).unwrap();
    let previous = checkpoint.clone();
    for _ in 0..12 {
        let response = harness.preview("", None).await;
        response.assert_status(StatusCode::OK);
        assert_eq!(harness.history().await.summary.as_ref(), Some(&previous));
    }
    assert_eq!(
        harness
            .history()
            .await
            .entries
            .iter()
            .find(|entry| entry.id == first_result.id)
            .unwrap()
            .message
            .content
            .as_deref(),
        Some(first.as_str())
    );
}

#[tokio::test]
async fn image_references_roundtrip_canonical_storage_and_reconnect_without_wire_deserialization_loss()
 {
    let mut harness = Harness::new(
        Some(32768),
        false,
        vec![answer("Saw the picture"), answer("Still have the picture")],
    )
    .await;
    let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jZ1sAAAAASUVORK5CYII=";
    let mut socket = harness.connect().await;
    send(&mut socket,json!({"type":"send","content":"Inspect this attachment","metadata":{"attachments":[{"name":"pixel.png","mime":"image/png","url":image}]}})).await;
    successful(&finish(&mut socket).await);
    let _ = socket.close(None).await;
    let history = harness.history().await;
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.message.images == [image])
    );
    harness.restart().await;
    successful(&harness.turn("Use the same attachment again").await);
    let requests = harness.requests().await;
    let request = ordinary(&requests).last().unwrap().to_owned();
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["image_url"]["url"] == image)))
    );
}

#[tokio::test]
async fn denied_mutation_has_one_durable_error_result_and_never_writes_the_file() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    let path = harness.directory().join("denied.txt");
    harness
        .client
        .put_json_auth(
            &format!("/api/chats/{}", harness.chat),
            &json!({"auto_approve":false}),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::OK);
    harness.script.push(calls(
        "Awaiting permission",
        vec![tool(
            "denied",
            "write_file",
            json!({"path":path,"content":"must not be written"}),
        )],
    ));
    harness.script.push(answer("Permission declined"));
    let mut socket = harness.connect().await;
    send(
        &mut socket,
        json!({"type":"send","content":"Propose the write"}),
    )
    .await;
    loop {
        let frame = next(&mut socket).await;
        assert_ne!(frame["type"], "error", "{frame}");
        if frame["type"] == "tool_approval_required" {
            send(&mut socket,json!({"type":"approve_tool","tool_call_id":frame["tool_call_id"],"approved":false})).await;
            break;
        }
    }
    successful(&finish(&mut socket).await);
    assert!(!path.exists());
    let requests = harness.requests().await;
    let request = ordinary(&requests).last().unwrap().to_owned();
    assert_eq!(pairs(request), 1);
    let result = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(
        result.contains("denied") || result.contains("declined") || result.contains("not approved")
    );
    assert_eq!(
        harness
            .history()
            .await
            .entries
            .iter()
            .filter(|entry| entry.message.role == zone_core::llm::Role::Tool)
            .count(),
        1
    );
}

#[tokio::test]
async fn cancellation_records_an_unknown_mutation_outcome_and_followup_never_reexecutes_it() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    harness.script.push(calls(
        "Running delayed command",
        vec![tool(
            "delayed",
            "run_command",
            json!({"command":"sleep","args":["5"],"timeout_secs":10}),
        )],
    ));
    harness.script.push(answer("Continued safely"));
    let mut socket = harness.connect().await;
    send(
        &mut socket,
        json!({"type":"send","content":"Run the delayed command"}),
    )
    .await;
    loop {
        let frame = next(&mut socket).await;
        assert_ne!(frame["type"], "error", "{frame}");
        if frame["type"] == "tool_call" {
            let envelopes: i64 =
                sqlx::query_scalar("SELECT count(*) FROM chat_calls WHERE chat_id=$1")
                    .bind(harness.chat)
                    .fetch_one(&harness.pool)
                    .await
                    .unwrap();
            assert_eq!(
                envelopes, 1,
                "assistant envelope must be committed before tool execution"
            );
            send(&mut socket, json!({"type":"cancel"})).await;
            break;
        }
    }
    let frames = finish(&mut socket).await;
    assert!(frames.iter().any(|frame| frame["type"] == "cancelled"));
    send(
        &mut socket,
        json!({"type":"send","content":"Continue without rerunning uncertain effects"}),
    )
    .await;
    successful(&finish(&mut socket).await);
    let requests = harness.requests().await;
    let request = ordinary(&requests).last().unwrap().to_owned();
    assert_eq!(pairs(request), 1);
    let result = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(result.contains("unknown") || result.contains("interrupted"));
    let calls: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_calls WHERE chat_id=$1")
        .bind(harness.chat)
        .fetch_one(&harness.pool)
        .await
        .unwrap();
    assert_eq!(calls, 1);
}

fn finalized(frames: &[Value]) -> bool {
    frames.iter().any(|frame| {
        frame["type"] == "status" && frame.to_string().to_ascii_lowercase().contains("finaliz")
    })
}

#[tokio::test]
async fn alternating_and_reordered_unchanged_reads_finalize_without_exhausting_the_loop() {
    for reordered in [false, true] {
        let harness = Harness::new(Some(200_000), true, vec![]).await;
        let a = harness.file("a.txt", "unchanged A");
        let b = harness.file("b.txt", "unchanged B");
        for index in 0..4 {
            let paths = if reordered {
                if index % 2 == 0 {
                    vec![&a, &b]
                } else {
                    vec![&b, &a]
                }
            } else if index % 2 == 0 {
                vec![&a]
            } else {
                vec![&b]
            };
            let batch = paths
                .into_iter()
                .enumerate()
                .map(|(position, path)| {
                    tool(
                        &format!("call-{index}-{position}"),
                        "read_file",
                        json!({"path":path}),
                    )
                })
                .collect();
            harness
                .script
                .push(calls("Inspecting unchanged evidence", batch));
        }
        harness.script.push(answer("Final response"));
        harness
            .script
            .finalize(answer("Finalized from existing evidence"));
        let frames = harness
            .turn("Inspect the evidence and conclude when nothing changes")
            .await;
        successful(&frames);
        assert!(
            finalized(&frames),
            "expected explicit no-progress finalization: {frames:?}"
        );
        assert!(ordinary(&harness.requests().await).len() <= 5);
    }
}

#[tokio::test]
async fn changed_read_evidence_and_successful_writes_allow_productive_rereads() {
    for write in [false, true] {
        let harness = Harness::new(Some(200_000), true, vec![]).await;
        let file = harness.file("progress.txt", "old evidence");
        harness.script.push(calls(
            "First read",
            vec![tool("first", "read_file", json!({"path":file}))],
        ));
        if write {
            harness.script.push(calls(
                "Apply change",
                vec![tool(
                    "write",
                    "write_file",
                    json!({"path":file,"content":"new evidence"}),
                )],
            ));
        }
        harness.script.push(calls(
            "Verify changed evidence",
            vec![tool("second", "read_file", json!({"path":file}))],
        ));
        harness.script.push(answer("Verified the change"));
        if !write {
            let sequence = Arc::new(AtomicUsize::new(0));
            let script = harness.script.clone();
            let file = file.clone();
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(move |request: &Request| {
                    if sequence.fetch_add(1, Ordering::SeqCst) == 1 {
                        std::fs::write(&file, "new evidence").unwrap();
                    }
                    script.respond(request)
                })
                .with_priority(1)
                .mount(&harness.provider)
                .await;
        }
        let frames = harness.turn("Inspect and verify real progress").await;
        successful(&frames);
        assert!(!finalized(&frames), "changed evidence is progress");
        let requests = harness.requests().await;
        let request = ordinary(&requests).last().unwrap().to_owned();
        assert!(
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool" && message["content"] == "new evidence")
        );
    }
}

#[tokio::test]
async fn productive_work_continues_beyond_the_former_twelve_round_budget() {
    let harness = Harness::new(Some(200_000), true, vec![]).await;
    let file = harness.file("progressive.txt", "initial");
    for index in 0..14 {
        harness.script.push(calls(
            "Making measurable progress",
            vec![tool(
                &format!("write-{index}"),
                "write_file",
                json!({"path":file,"content":format!("progress-{index}")}),
            )],
        ));
    }
    harness
        .script
        .push(answer("Completed fourteen productive rounds"));
    let frames = harness.turn("Complete each of the planned changes").await;
    successful(&frames);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "progress-13");
    assert_eq!(ordinary(&harness.requests().await).len(), 15);
    assert!(!finalized(&frames));
}

#[tokio::test]
async fn evidence_retrieval_is_scoped_to_the_current_chat_even_inside_one_workspace() {
    let harness = Harness::new(Some(200_000), true, vec![]).await;
    let file = harness.file("private.txt", "PRIVATE_CANONICAL_EVIDENCE");
    harness.script.push(calls(
        "Read private evidence",
        vec![tool("original", "read_file", json!({"path":file}))],
    ));
    harness.script.push(answer("Read"));
    successful(&harness.turn("Read the file").await);
    let history = harness.history().await;
    let reference = history
        .entries
        .iter()
        .find(|entry| entry.message.role == zone_core::llm::Role::Tool)
        .unwrap()
        .id
        .clone();
    let second=harness.client.post_json_auth("/api/chats",&json!({"workspace_id":harness.workspace,"title":"Different chat","model_name":"context-test","agent_enabled":true,"automatic_title":false,"auto_approve":true}),&harness.token).await.json_value();
    let second: Uuid = second["chat"]["id"].as_str().unwrap().parse().unwrap();
    harness.script.push(calls(
        "Attempt foreign reference",
        vec![tool(
            "foreign",
            "read_chat_evidence",
            json!({"id":reference,"offset":0,"limit":100}),
        )],
    ));
    harness.script.push(answer("Cannot access that chat"));
    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{}/ws/chats/{second}", harness.address))
            .await
            .unwrap();
    send(&mut socket, json!({"type":"auth","token":harness.token})).await;
    assert_eq!(next(&mut socket).await["type"], "init");
    send(
        &mut socket,
        json!({"type":"send","content":"Read that referenced evidence"}),
    )
    .await;
    successful(&finish(&mut socket).await);
    let requests = harness.requests().await;
    let request = ordinary(&requests).last().unwrap().to_owned();
    assert_eq!(pairs(request), 1);
    assert!(!request.to_string().contains("PRIVATE_CANONICAL_EVIDENCE"));
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("Error:"))
    );
}

#[tokio::test]
async fn deleting_a_user_turn_removes_its_tool_evidence_from_future_replay() {
    let harness = Harness::new(Some(200_000), true, vec![]).await;
    let file = harness.file("remove.txt", "REMOVED_TOOL_EVIDENCE");
    harness.script.push(calls(
        "Intermediate visible prose",
        vec![tool("remove", "read_file", json!({"path":file}))],
    ));
    harness.script.push(answer("Visible final prose"));
    let frames = harness.turn("User message to delete").await;
    successful(&frames);
    let user = frames
        .iter()
        .find(|frame| frame["type"] == "message_saved" && frame["role"] == "user")
        .unwrap()["message_id"]
        .as_str()
        .unwrap();
    harness
        .client
        .delete_auth(
            &format!("/api/chats/{}/messages/{user}", harness.chat),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    harness.script.push(answer("Continued after deletion"));
    successful(&harness.turn("Continue").await);
    let requests = harness.requests().await;
    let request = ordinary(&requests).last().unwrap().to_owned();
    assert!(!request.to_string().contains("REMOVED_TOOL_EVIDENCE"));
    assert!(!request.to_string().contains("User message to delete"));
    assert_eq!(pairs(request), 0);
}
