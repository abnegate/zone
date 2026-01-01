import database/queries/chats
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import models/chat.{CreateChatRequest, SendMessageRequest}
import test_db

// =============================================================================
// Chat CRUD Tests
// =============================================================================

pub fn create_chat_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)

    let chat = chats.create_chat(db, req) |> should.be_ok()

    chat.title |> should.equal("New Chat")
    chat.model_name |> should.equal("llama3.2")
    chat.archived |> should.equal(False)
  })
}

pub fn create_chat_with_first_message_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateChatRequest(
        model_name: "llama3.2",
        first_message: Some("Hello, how are you?"),
      )

    let chat = chats.create_chat(db, req) |> should.be_ok()

    // Title should be derived from first message
    chat.title |> should.equal("Hello, how are you?")

    // Should have one message
    let messages = chats.list_messages(db, chat.id) |> should.be_ok()
    list.length(messages) |> should.equal(1)

    let assert [msg] = messages
    msg.role |> should.equal(chat.User)
    msg.content |> should.equal("Hello, how are you?")
  })
}

pub fn list_chats_empty_test() {
  test_db.with_db(fn(db) {
    chats.list_chats(db, None)
    |> should.be_ok()
    |> should.equal([])
  })
}

pub fn list_chats_returns_created_chats_test() {
  test_db.with_db(fn(db) {
    let req1 = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let req2 = CreateChatRequest(model_name: "gpt-4", first_message: None)

    let chat1 = chats.create_chat(db, req1) |> should.be_ok()
    let chat2 = chats.create_chat(db, req2) |> should.be_ok()

    let all_chats = chats.list_chats(db, None) |> should.be_ok()
    // Check that both created chats are in the list (there may be more from parallel tests)
    let ids = list.map(all_chats, fn(c) { c.id })
    list.contains(ids, chat1.id) |> should.be_true()
    list.contains(ids, chat2.id) |> should.be_true()
  })
}

pub fn list_chats_filter_archived_test() {
  test_db.with_db(fn(db) {
    let req1 = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let req2 = CreateChatRequest(model_name: "gpt-4", first_message: None)

    let chat1 = chats.create_chat(db, req1) |> should.be_ok()
    let chat2 = chats.create_chat(db, req2) |> should.be_ok()

    // Archive one chat
    let _ = chats.archive_chat(db, chat1.id) |> should.be_ok()

    // Filter by archived = false should include chat2 but not chat1
    let active = chats.list_chats(db, Some(False)) |> should.be_ok()
    let active_ids = list.map(active, fn(c) { c.id })
    list.contains(active_ids, chat2.id) |> should.be_true()
    list.contains(active_ids, chat1.id) |> should.be_false()

    // Filter by archived = true should include chat1 but not chat2
    let archived = chats.list_chats(db, Some(True)) |> should.be_ok()
    let archived_ids = list.map(archived, fn(c) { c.id })
    list.contains(archived_ids, chat1.id) |> should.be_true()
    list.contains(archived_ids, chat2.id) |> should.be_false()
  })
}

pub fn get_chat_not_found_test() {
  test_db.with_db(fn(db) {
    chats.get_chat(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_chat_found_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let created = chats.create_chat(db, req) |> should.be_ok()

    let found =
      chats.get_chat(db, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.model_name |> should.equal("llama3.2")
  })
}

pub fn get_chat_with_messages_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateChatRequest(model_name: "llama3.2", first_message: Some("Hello!"))
    let chat = chats.create_chat(db, req) |> should.be_ok()

    // Add assistant response
    let _ =
      chats.add_assistant_message(db, chat.id, "Hi there!") |> should.be_ok()

    let chat_with_msgs =
      chats.get_chat_with_messages(db, chat.id)
      |> should.be_ok()
      |> should.be_some()

    chat_with_msgs.chat.id |> should.equal(chat.id)
    list.length(chat_with_msgs.messages) |> should.equal(2)
  })
}

pub fn update_chat_title_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let updated =
      chats.update_chat_title(db, chat.id, "New Title")
      |> should.be_ok()
      |> should.be_some()

    updated.title |> should.equal("New Title")
  })
}

pub fn update_chat_title_not_found_test() {
  test_db.with_db(fn(db) {
    chats.update_chat_title(db, "00000000-0000-0000-0000-000000000000", "New Title")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn archive_chat_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()
    chat.archived |> should.equal(False)

    let archived =
      chats.archive_chat(db, chat.id)
      |> should.be_ok()
      |> should.be_some()

    archived.archived |> should.equal(True)
  })
}

pub fn unarchive_chat_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    // Archive then unarchive
    let _ = chats.archive_chat(db, chat.id) |> should.be_ok()

    let unarchived =
      chats.unarchive_chat(db, chat.id)
      |> should.be_ok()
      |> should.be_some()

    unarchived.archived |> should.equal(False)
  })
}

pub fn delete_chat_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    chats.delete_chat(db, chat.id)
    |> should.be_ok()
    |> should.equal(True)

    // Chat should be gone
    chats.get_chat(db, chat.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_chat_not_found_test() {
  test_db.with_db(fn(db) {
    chats.delete_chat(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(False)
  })
}

// =============================================================================
// Message CRUD Tests
// =============================================================================

pub fn add_user_message_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let msg =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "Hello!"))
      |> should.be_ok()

    msg.chat_id |> should.equal(chat.id)
    msg.role |> should.equal(chat.User)
    msg.content |> should.equal("Hello!")
  })
}

pub fn add_assistant_message_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let msg =
      chats.add_assistant_message(db, chat.id, "Hi there!")
      |> should.be_ok()

    msg.role |> should.equal(chat.Assistant)
    msg.content |> should.equal("Hi there!")
  })
}

pub fn add_system_message_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let msg =
      chats.add_system_message(db, chat.id, "You are a helpful assistant.")
      |> should.be_ok()

    msg.role |> should.equal(chat.System)
    msg.content |> should.equal("You are a helpful assistant.")
  })
}

pub fn list_messages_empty_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    chats.list_messages(db, chat.id)
    |> should.be_ok()
    |> should.equal([])
  })
}

pub fn list_messages_in_order_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let _ =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "First"))
      |> should.be_ok()
    let _ = chats.add_assistant_message(db, chat.id, "Second") |> should.be_ok()
    let _ =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "Third"))
      |> should.be_ok()

    let messages = chats.list_messages(db, chat.id) |> should.be_ok()

    list.length(messages) |> should.equal(3)

    let assert [m1, m2, m3] = messages
    m1.content |> should.equal("First")
    m2.content |> should.equal("Second")
    m3.content |> should.equal("Third")
  })
}

pub fn get_message_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let created =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "Hello!"))
      |> should.be_ok()

    let found =
      chats.get_message(db, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.content |> should.equal("Hello!")
  })
}

pub fn delete_message_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let msg =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "Hello!"))
      |> should.be_ok()

    chats.delete_message(db, msg.id)
    |> should.be_ok()
    |> should.equal(True)

    chats.get_message(db, msg.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn messages_cascade_delete_with_chat_test() {
  test_db.with_db(fn(db) {
    let req = CreateChatRequest(model_name: "llama3.2", first_message: None)
    let chat = chats.create_chat(db, req) |> should.be_ok()

    let _ =
      chats.add_user_message(db, chat.id, SendMessageRequest(content: "Hello!"))
      |> should.be_ok()
    let _ = chats.add_assistant_message(db, chat.id, "Hi!") |> should.be_ok()

    // Verify messages exist
    let messages_before = chats.list_messages(db, chat.id) |> should.be_ok()
    list.length(messages_before) |> should.equal(2)

    // Delete chat
    let _ = chats.delete_chat(db, chat.id) |> should.be_ok()

    // Messages should be cascade deleted
    chats.list_messages(db, chat.id)
    |> should.be_ok()
    |> should.equal([])
  })
}
