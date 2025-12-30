import database/connection.{Connection, DbConfig}
import database/queries/chats
import gleam/option.{None}
import gleeunit/should
import models/chat

// =============================================================================
// Test helpers
// =============================================================================

fn get_test_db() -> connection.Connection {
  Connection(DbConfig(host: "", database: "", user: "", password: ""))
}

// =============================================================================
// Chat queries tests (stubbed database)
// =============================================================================

pub fn list_chats_returns_empty_list_test() {
  let db = get_test_db()

  chats.list_chats(db, None)
  |> should.be_ok()
  |> should.equal([])
}

pub fn list_chats_with_filter_returns_empty_list_test() {
  let db = get_test_db()

  chats.list_chats(db, option.Some(True))
  |> should.be_ok()
  |> should.equal([])
}

pub fn get_chat_returns_none_test() {
  let db = get_test_db()

  chats.get_chat(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn get_chat_with_messages_returns_none_test() {
  let db = get_test_db()

  chats.get_chat_with_messages(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn create_chat_returns_error_test() {
  let db = get_test_db()
  let req = chat.CreateChatRequest(model_name: "llama3.2", first_message: None)

  chats.create_chat(db, req)
  |> should.be_error()
}

pub fn update_chat_title_returns_none_test() {
  let db = get_test_db()

  chats.update_chat_title(db, "nonexistent-id", "New Title")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn archive_chat_returns_none_test() {
  let db = get_test_db()

  chats.archive_chat(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn unarchive_chat_returns_none_test() {
  let db = get_test_db()

  chats.unarchive_chat(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn delete_chat_returns_false_test() {
  let db = get_test_db()

  chats.delete_chat(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(False)
}

// =============================================================================
// Message queries tests (stubbed database)
// =============================================================================

pub fn list_messages_returns_empty_list_test() {
  let db = get_test_db()

  chats.list_messages(db, "chat-id")
  |> should.be_ok()
  |> should.equal([])
}

pub fn get_message_returns_none_test() {
  let db = get_test_db()

  chats.get_message(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(None)
}

pub fn add_user_message_returns_error_test() {
  let db = get_test_db()
  let req = chat.SendMessageRequest(content: "Hello!")

  chats.add_user_message(db, "chat-id", req)
  |> should.be_error()
}

pub fn add_assistant_message_returns_error_test() {
  let db = get_test_db()

  chats.add_assistant_message(db, "chat-id", "Hello, I'm an assistant!")
  |> should.be_error()
}

pub fn add_system_message_returns_error_test() {
  let db = get_test_db()

  chats.add_system_message(db, "chat-id", "System prompt here")
  |> should.be_error()
}

pub fn delete_message_returns_false_test() {
  let db = get_test_db()

  chats.delete_message(db, "nonexistent-id")
  |> should.be_ok()
  |> should.equal(False)
}

pub fn touch_chat_returns_ok_test() {
  let db = get_test_db()

  chats.touch_chat(db, "chat-id")
  |> should.be_ok()
}
