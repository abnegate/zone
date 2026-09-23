//! Which model reviews a change: one that did not write it, when there is one.

use crate::db::{ai_settings, workspaces};
use crate::services::stages::{self, Catalog, Preferences};
use crate::state::AppState;
use uuid::Uuid;
use zone_core::llm::LlmBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub model: String,
    /// The author's own model, because nothing else was available.
    pub same_model: bool,
}

/// Pick a reviewer for `round` of a change `author` wrote.
///
/// Candidates in order: the operator's configured reviewers, the workspace's
/// reasoning and fast models, then everything installed, largest first. The
/// author is dropped, and successive rounds rotate through what is left so a
/// change that keeps coming back is read by different eyes. With nothing left
/// the author reviews itself, and says so.
///
/// On an agent, a candidate is a model the agent knows, and the agent's own
/// models stand in for what is installed.
pub fn select(
    author: Option<&str>,
    prefs: &Preferences,
    catalog: &Catalog,
    configured: &[String],
    round: u32,
) -> Reviewer {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.trim();
        if name.is_empty() || stages::is_auto(name) || !catalog.accepts(name) {
            return;
        }
        if author.is_some_and(|author| stages::same_model(author, name)) {
            return;
        }
        if candidates
            .iter()
            .any(|known| stages::same_model(known, name))
        {
            return;
        }
        candidates.push(name.to_string());
    };
    for name in configured {
        push(name);
    }
    if let Some(name) = prefs.reasoning.as_deref() {
        push(name);
    }
    if let Some(name) = prefs.fast.as_deref() {
        push(name);
    }
    let mut installed: Vec<_> = catalog.completions().collect();
    installed.sort_by_key(|model| std::cmp::Reverse(model.bytes));
    for model in installed {
        if catalog.contains(&model.name) {
            push(&model.name);
        }
    }
    if candidates.is_empty() {
        return Reviewer {
            model: author
                .map(str::to_string)
                .or_else(|| prefs.reasoning.clone())
                .or_else(|| prefs.fast.clone())
                .unwrap_or_else(|| stages::AUTO.to_string()),
            same_model: true,
        };
    }
    let index = usize::try_from(round.saturating_sub(1)).unwrap_or(0) % candidates.len();
    Reviewer {
        model: candidates[index].clone(),
        same_model: false,
    }
}

/// The model that wrote a change, as its run recorded it. A run whose agent
/// chose its own model recorded [`stages::AUTO`], which names no model, so no
/// review can be shown to be independent of it.
pub fn author(recorded: Option<String>) -> Option<String> {
    recorded.filter(|model| !stages::is_auto(model))
}

/// The workspace's model preferences and what `backend` can run, read the way
/// a run reads them.
pub async fn preferences(
    state: &AppState,
    workspace_id: Uuid,
    backend: &LlmBackend,
) -> (Preferences, Catalog) {
    let catalog = Catalog::for_backend(&state.config().ollama_host, backend).await;
    let settings = match workspaces::get_workspace(state.db(), workspace_id).await {
        Ok(Some(workspace)) => ai_settings::get_effective_ai_settings(
            state.db(),
            workspace.organization_id,
            workspace_id,
        )
        .await
        .ok(),
        _ => None,
    };
    (
        Preferences::from_optional_settings(
            settings.as_ref(),
            &state.config().comfyui.classifier_model,
        ),
        catalog,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stages::Installed;
    use zone_core::llm::AgentKind;

    fn installed(name: &str, bytes: u64) -> Installed {
        Installed {
            name: name.into(),
            bytes,
            million_params: None,
            embedding: false,
            vision: false,
            reranker: false,
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            models: vec![
                installed("small:latest", 1),
                installed("big:latest", 9),
                installed("author:latest", 5),
            ],
            agent: None,
        }
    }

    #[test]
    fn the_author_is_never_its_own_reviewer_while_another_model_exists() {
        let prefs = Preferences::default();
        let first = select(Some("author"), &prefs, &catalog(), &[], 1);
        assert_eq!(first.model, "big:latest");
        assert!(!first.same_model);
        let second = select(Some("author"), &prefs, &catalog(), &[], 2);
        assert_eq!(
            second.model, "small:latest",
            "rounds rotate through the rest"
        );
        let third = select(Some("author"), &prefs, &catalog(), &[], 3);
        assert_eq!(third.model, "big:latest");
    }

    #[test]
    fn configured_and_workspace_models_come_first_and_the_author_reviews_itself_last() {
        let prefs = Preferences {
            reasoning: Some("reasoner".into()),
            fast: Some("author".into()),
            ..Preferences::default()
        };
        let picked = select(
            Some("author"),
            &prefs,
            &catalog(),
            &["ops-reviewer".into()],
            1,
        );
        assert_eq!(picked.model, "ops-reviewer");
        let alone = select(
            Some("author"),
            &Preferences::default(),
            &Catalog {
                models: vec![installed("author", 5)],
                agent: None,
            },
            &[],
            1,
        );
        assert_eq!(alone.model, "author");
        assert!(alone.same_model);
    }

    #[test]
    fn an_agent_reviews_on_models_it_knows_and_never_an_installed_one() {
        let prefs = Preferences {
            reasoning: Some("llama3.1:70b".into()),
            ..Preferences::default()
        };
        let claude = Catalog::agent(AgentKind::Claude);
        let configured = ["qwen3:32b".to_string(), "opus".to_string()];

        assert_eq!(
            select(Some("sonnet"), &prefs, &claude, &configured, 1),
            Reviewer {
                model: "opus".into(),
                same_model: false,
            }
        );
        assert_eq!(
            select(Some("sonnet"), &prefs, &claude, &configured, 2).model,
            "haiku",
            "the agent's other models follow the ones configured"
        );
        assert_eq!(
            select(Some("sonnet"), &prefs, &claude, &configured, 3).model,
            "opus"
        );
    }

    #[test]
    fn a_run_left_to_the_agent_s_choice_names_no_author() {
        assert_eq!(author(Some(stages::AUTO.into())), None);
        assert_eq!(author(Some("sonnet".into())), Some("sonnet".into()));
        assert_eq!(author(None), None);
    }
}
