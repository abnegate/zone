use uuid::Uuid;
use zone_core::llm::LlmBackend;
use zone_enterprise::policy::Denial;

use super::{Denied, Subject, record};
use crate::db::audit::actions;
use crate::services::route::Route;
use crate::services::stages::{self, Catalog, Preferences};
use crate::state::AppState;

/// A completion Zone runs on its own for `subject`, in `workspace` when it has one: a title, a
/// pull request's subject, a merge summary. Nobody waits on it, so a denial skips it and is
/// audited, as started by `actor` when someone did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Background {
    pub workspace: Option<Uuid>,
    pub subject: Subject,
    pub actor: Option<Uuid>,
}

impl Background {
    /// The first of `candidates` the policy `route` runs under lets run on `backend`. None when
    /// it lets none of them run, which is audited against the first.
    pub async fn model(
        &self,
        state: &AppState,
        route: &Route,
        backend: &LlmBackend,
        candidates: &[String],
    ) -> Option<String> {
        let mut refusal = None;
        for candidate in candidates {
            match route.admit(state, backend, candidate).await {
                Ok(()) => return Some(candidate.clone()),
                Err(denial) => {
                    refusal.get_or_insert(denial);
                }
            }
        }
        if let Some(denial) = refusal {
            self.refused(state, denial, candidates.first().map(String::as_str))
                .await;
        }
        None
    }

    /// Audits that the policy refused this completion with `denial`, on `model` when one was
    /// chosen.
    pub async fn refused(&self, state: &AppState, denial: Denial, model: Option<&str>) {
        let Some(workspace) = self.workspace else {
            return;
        };
        tracing::warn!(%workspace, code = denial.code(), model, "The agent policy refused a background completion, so it was skipped");
        record(
            state,
            actions::AGENT_POLICY_TURN_DENIED,
            &Denied {
                workspace,
                organization: None,
                actor: self.actor,
                subject: self.subject,
                denial,
                detail: model,
            },
        )
        .await;
    }
}

/// The models a background stage may run on `catalog`, in the order it prefers them:
/// `configured`, the model the stage chose, then the Fast, Reasoning and `chat` models, each
/// once. An agent runs only names it knows, or [`stages::AUTO`] to choose its own; an endpoint
/// runs only names `preferences` let a completion be sent under.
pub fn candidates(
    configured: &str,
    preferences: &Preferences,
    catalog: &Catalog,
    chat: &str,
) -> Vec<String> {
    let mut names = vec![configured.trim().to_string()];
    let alternatives = [
        preferences.fast.as_deref(),
        preferences.reasoning.as_deref(),
        Some(chat),
    ];
    for name in alternatives.into_iter().flatten().map(str::trim) {
        let runnable = match stages::is_auto(name) {
            true => catalog.chooses(),
            false => catalog.accepts(name) && preferences.admits(name),
        };
        if runnable && !names.iter().any(|named| stages::same_model(named, name)) {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::AgentKind;
    use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;
    use zone_enterprise::policy::{Effective, Rules};

    use super::*;
    use crate::services::endpoint::testing::settings;
    use crate::services::stages::{AUTO, Scope};
    use crate::state::{test_config, unreachable_pool};

    fn preferences(fast: &str, reasoning: &str, scope: Scope) -> Preferences {
        Preferences {
            fast: Some(fast.to_string()),
            reasoning: Some(reasoning.to_string()),
            scope,
            ..Preferences::default()
        }
    }

    #[tokio::test]
    async fn the_first_candidate_the_policy_admits_runs_and_none_runs_when_it_admits_none() {
        let state = AppState::new(test_config(), unreachable_pool(), None);
        let rules = Rules {
            models: Some(vec!["qwen*".to_string()]),
            ..Rules::default()
        };
        let route = Route::new(
            state.config(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            settings(PROVIDER_SELF_HOSTED),
        )
        .with_policy(Some(Effective::new(rules, Rules::default())));
        let background = Background {
            workspace: None,
            subject: Subject::Task(Uuid::new_v4()),
            actor: None,
        };
        let fallback = ["llama3.2:3b".to_string(), "qwen3:32b".to_string()];
        let denied = ["llama3.2:3b".to_string()];

        let admitted = background
            .model(&state, &route, &LlmBackend::Http, &fallback)
            .await;
        let skipped = background
            .model(&state, &route, &LlmBackend::Http, &denied)
            .await;

        assert_eq!(admitted.as_deref(), Some("qwen3:32b"));
        assert_eq!(skipped, None);
    }

    #[test]
    fn the_configured_model_comes_first_then_the_fast_reasoning_and_chat_models_once_each() {
        let preferences = preferences("llama3.2:3b", "qwen3:32b", Scope::Open);

        let names = candidates("llama3.2:3b", &preferences, &Catalog::default(), "gpt-4");

        assert_eq!(names, ["llama3.2:3b", "qwen3:32b", "gpt-4"]);
    }

    #[test]
    fn an_endpoint_never_falls_back_to_auto_or_a_model_its_settings_do_not_save() {
        let preferences = preferences("saved-fast", "saved-reasoning", Scope::Saved);

        let named = candidates("saved-fast", &preferences, &Catalog::default(), "unsaved");
        let unnamed = candidates("saved-fast", &preferences, &Catalog::default(), AUTO);

        assert_eq!(named, ["saved-fast", "saved-reasoning"]);
        assert_eq!(unnamed, ["saved-fast", "saved-reasoning"]);
    }

    #[test]
    fn an_agent_falls_back_only_to_models_it_knows_or_to_choosing_its_own() {
        let preferences = preferences("haiku", "gpt-5", Scope::Open);
        let catalog = Catalog::agent(AgentKind::Claude);

        let names = candidates("sonnet", &preferences, &catalog, AUTO);

        assert_eq!(names, ["sonnet", "haiku", AUTO]);
    }
}
