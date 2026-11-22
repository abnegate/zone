use serde_json::Value;
use uuid::Uuid;

use super::registry::Registry;
use crate::agent::question::{self, Answer};
use crate::db::actions;
use crate::workers::titles;

/// This process's own registries.
pub struct Local;

impl Registry for Local {
    fn answer(&self, run: Uuid, answers: Vec<Answer>) -> bool {
        question::answer(run, answers)
    }

    fn title(&self, chat: Uuid, title: String) {
        titles::deliver(chat, title);
    }

    fn action(&self, chat: Uuid, message: Value) {
        actions::deliver(chat, message);
    }
}
