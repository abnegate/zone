mod common;

use common::create_test_pool;
use zone_server::db::chats;

#[tokio::test]
async fn automatic_title_fallback_is_persisted_once() {
    use std::time::Duration;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    use zone_server::workers::titles;

    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&provider)
        .await;
    let pool = create_test_pool().await;
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    let state = common::create_test_state(config, pool.clone());
    for (content, expected) in [
        ("", "Attachment discussion"),
        ("Help plan my holiday", "Help plan my holiday"),
    ] {
        let chat = chats::create_chat_with_title(
            &pool,
            None,
            "New chat",
            "image-checkpoint",
            (false, true),
            true,
            false,
        )
        .await
        .unwrap();
        let message = chats::create_message(&pool, chat.id, "user", content, None)
            .await
            .unwrap();
        let mut updates = titles::subscribe();
        titles::spawn(state.clone(), &message);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (id, title) = updates.recv().await.unwrap();
                if id == chat.id {
                    assert_eq!(title, expected);
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            chats::get_chat(&pool, chat.id)
                .await
                .unwrap()
                .unwrap()
                .title,
            expected
        );
        let second = chats::create_message(&pool, chat.id, "user", "Second message", None)
            .await
            .unwrap();
        assert!(!second.title_claimed);
        titles::spawn(state.clone(), &second);
        chats::delete_chat(&pool, chat.id).await.unwrap();
    }
    let requests = provider.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body["model"],
        common::test_config().comfyui.classifier_model
    );
}

#[tokio::test]
async fn automatic_title_claim_is_first_user_only_and_concurrent_safe() {
    let pool = create_test_pool().await;
    let chat = chats::create_chat_with_title(
        &pool,
        None,
        "New chat",
        "llama3.2:3b",
        (false, true),
        true,
        false,
    )
    .await
    .unwrap();
    let system = chats::create_message(&pool, chat.id, "system", "context", None)
        .await
        .unwrap();
    assert!(!system.title_claimed);
    let (first, second) = tokio::join!(
        chats::create_message(&pool, chat.id, "user", "first", None),
        chats::create_message(&pool, chat.id, "user", "second", None),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_ne!(first.title_claimed, second.title_claimed);
    let owner = if first.title_claimed { first } else { second };
    assert!(
        chats::complete_title(&pool, chat.id, owner.id, "Summary")
            .await
            .unwrap()
    );
    assert!(
        !chats::complete_title(&pool, chat.id, owner.id, "Replacement")
            .await
            .unwrap()
    );
    let later = chats::create_message(&pool, chat.id, "user", "later", None)
        .await
        .unwrap();
    assert!(!later.title_claimed);
    chats::delete_chat(&pool, chat.id).await.unwrap();
}

#[tokio::test]
async fn manual_title_even_unchanged_wins_pending_generation() {
    let pool = create_test_pool().await;
    for title in ["New chat", "My custom title"] {
        let chat = chats::create_chat_with_title(
            &pool,
            None,
            "New chat",
            "llama3.2:3b",
            (false, true),
            true,
            false,
        )
        .await
        .unwrap();
        let message = chats::create_message(&pool, chat.id, "user", "Plan travel", None)
            .await
            .unwrap();
        assert!(message.title_claimed);
        chats::update_chat(&pool, chat.id, Some(title), None, None, None)
            .await
            .unwrap();
        assert!(
            !chats::complete_title(&pool, chat.id, message.id, "Travel planning")
                .await
                .unwrap()
        );
        assert_eq!(
            chats::get_chat(&pool, chat.id)
                .await
                .unwrap()
                .unwrap()
                .title,
            title
        );
        chats::delete_chat(&pool, chat.id).await.unwrap();
    }
}

#[tokio::test]
async fn custom_titles_and_renamed_empty_chats_are_not_claimed() {
    let pool = create_test_pool().await;
    for automatic in [false, true] {
        let chat = chats::create_chat_with_title(
            &pool,
            None,
            "Custom",
            "llama3.2:3b",
            (false, true),
            automatic,
            false,
        )
        .await
        .unwrap();
        if automatic {
            chats::update_chat(&pool, chat.id, Some("Custom"), None, None, None)
                .await
                .unwrap();
        }
        let message = chats::create_message(&pool, chat.id, "user", "Plan travel", None)
            .await
            .unwrap();
        assert!(!message.title_claimed);
        chats::delete_chat(&pool, chat.id).await.unwrap();
    }
}
