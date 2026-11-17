use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use abnegate_agent::context::ContextUsage;
use sqlx::PgPool;
use tokio::runtime::Handle;
use tokio_util::task::TaskTracker;

use super::spent::Spent;

type Recording = Pin<Box<dyn Future<Output = ()> + Send>>;
type Recorder = Arc<dyn Fn(Spent) -> Recording + Send + Sync>;

pub struct Pending {
    recorder: Recorder,
    tasks: TaskTracker,
    spent: Spent,
    stored: bool,
}

impl Pending {
    pub fn new(pool: PgPool, tasks: TaskTracker, spent: Spent) -> Self {
        let recorder: Recorder = Arc::new(move |spent| {
            let pool = pool.clone();
            Box::pin(async move { super::record(&pool, &spent).await })
        });
        Self::with(recorder, tasks, spent)
    }

    fn with(recorder: Recorder, tasks: TaskTracker, spent: Spent) -> Self {
        Self {
            recorder,
            tasks,
            spent,
            stored: false,
        }
    }

    pub fn context(&mut self, context: ContextUsage) {
        self.spent.context = Some(context);
    }

    pub fn stored(&mut self) {
        self.stored = true;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.spent.ledger.is_empty() && !self.stored {
            return;
        }
        let Ok(runtime) = Handle::try_current() else {
            tracing::warn!(
                chat = %self.spent.chat,
                message = %self.spent.message,
                model = %self.spent.model,
                ledger = ?self.spent.ledger.shares(),
                "No runtime to record what this turn spent"
            );
            return;
        };
        self.tasks
            .spawn_on((self.recorder)(self.spent.clone()), &runtime);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chat::tokens::Ledger;
    use abnegate_llm::{ReasoningEffort, Usage};
    use std::sync::Mutex;
    use uuid::Uuid;

    fn spent(ledger: Ledger) -> Spent {
        Spent {
            message: Uuid::new_v4(),
            chat: Uuid::new_v4(),
            workspace: Uuid::new_v4(),
            model: "llama3".to_string(),
            effort: ReasoningEffort::Medium,
            ledger,
            context: None,
        }
    }

    fn ledger() -> Ledger {
        let mut ledger = Ledger::default();
        ledger.round("llama3");
        ledger.add("llama3", Usage::new(10, 5));
        ledger
    }

    fn recording() -> (Recorder, Arc<Mutex<Vec<Spent>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let recorder: Recorder = Arc::new(move |spent| {
            let sink = sink.clone();
            Box::pin(async move { sink.lock().expect("the recorded turns").push(spent) })
        });
        (recorder, recorded)
    }

    async fn dropped(spent: Spent, stored: bool) -> Vec<Spent> {
        let (recorder, recorded) = recording();
        let tasks = TaskTracker::new();
        let mut pending = Pending::with(recorder, tasks.clone(), spent);
        if stored {
            pending.stored();
        }
        drop(pending);
        tasks.close();
        tasks.wait().await;
        recorded.lock().expect("the recorded turns").clone()
    }

    #[tokio::test]
    async fn a_turn_that_spent_tokens_is_recorded_once_when_it_is_dropped() {
        let spent = spent(ledger());
        let turn = spent.message;

        let recorded = dropped(spent, false).await;

        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0].message, turn);
    }

    #[tokio::test]
    async fn a_stored_answer_is_recorded_even_when_nothing_was_spent() {
        let recorded = dropped(spent(Ledger::default()), true).await;

        assert_eq!(recorded.len(), 1, "{recorded:?}");
    }

    #[tokio::test]
    async fn a_stored_answer_that_spent_tokens_is_still_recorded_once() {
        let recorded = dropped(spent(ledger()), true).await;

        assert_eq!(recorded.len(), 1, "{recorded:?}");
    }

    #[tokio::test]
    async fn a_turn_that_spent_nothing_and_stored_nothing_records_nothing() {
        let recorded = dropped(spent(Ledger::default()), false).await;

        assert!(recorded.is_empty(), "{recorded:?}");
    }

    #[tokio::test]
    async fn a_known_context_rides_with_the_recorded_turn() {
        let (recorder, recorded) = recording();
        let tasks = TaskTracker::new();
        let mut pending = Pending::with(recorder, tasks.clone(), spent(ledger()));
        pending.context(ContextUsage::default());
        drop(pending);
        tasks.close();
        tasks.wait().await;

        let recorded = recorded.lock().expect("the recorded turns");
        assert!(recorded[0].context.is_some());
    }
}
