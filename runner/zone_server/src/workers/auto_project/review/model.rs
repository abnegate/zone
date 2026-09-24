//! Which model reviews a change: one that did not write it, when there is one.

use crate::db::{ai_settings, workspaces};
use crate::services::stages::{self, Catalog, Preferences};
use crate::state::AppState;
use uuid::Uuid;
use zone_core::llm::LlmBackend;

const UNRECORDED_AUTHOR: &str = "the model that wrote the change was not recorded, so no review \
     can be shown to be independent, and no review bot answered";
const CHOSEN_AUTHOR: &str = "the agent chose its own model to write the change, so no review can \
     be shown to be independent of it, and no review bot answered; set Fast and Reasoning models \
     the agent knows in AI settings";
const NO_OTHER_MODEL: &str = "no model other than the one that wrote the change is available to \
     review it, and no review bot answered";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    pub model: String,
    /// Not shown to differ from the author's model: nothing else was
    /// available, or the agent chose the author's model itself.
    pub same_model: bool,
}

/// Who wrote a change, as its run recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Author {
    Model(String),
    /// The agent chose its own model, which the run could not name.
    Chosen,
    Unrecorded,
}

impl Author {
    pub fn recorded(model: Option<String>) -> Self {
        match model {
            None => Self::Unrecorded,
            Some(model) if stages::is_auto(&model) => Self::Chosen,
            Some(model) => Self::Model(model),
        }
    }

    pub fn model(&self) -> Option<&str> {
        match self {
            Self::Model(model) => Some(model),
            Self::Chosen | Self::Unrecorded => None,
        }
    }
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
/// models stand in for what is installed, less those it gates behind consent,
/// which review only when named. A change the agent wrote on a model of its
/// own choosing may have come from any of them.
pub fn select(
    author: &Author,
    prefs: &Preferences,
    catalog: &Catalog,
    configured: &[String],
    round: u32,
) -> Reviewer {
    let named = author.model();
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.trim();
        if name.is_empty() || stages::is_auto(name) || !catalog.accepts(name) {
            return;
        }
        if named.is_some_and(|author| stages::same_model(author, name)) {
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
    let mut installed: Vec<_> = catalog.unattended().collect();
    installed.sort_by_key(|model| std::cmp::Reverse(model.bytes));
    for model in installed {
        if catalog.contains(&model.name) {
            push(&model.name);
        }
    }
    if candidates.is_empty() {
        return Reviewer {
            model: named
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
        same_model: *author == Author::Chosen && catalog.chooses(),
    }
}

/// Why a review by `reviewer` cannot count as independent of `author`, or
/// `None` when it can.
pub fn objection(author: &Author, reviewer: &Reviewer) -> Option<&'static str> {
    match author {
        Author::Unrecorded => Some(UNRECORDED_AUTHOR),
        Author::Chosen => Some(CHOSEN_AUTHOR),
        Author::Model(_) => reviewer.same_model.then_some(NO_OTHER_MODEL),
    }
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
        let author = Author::Model("author".into());
        let first = select(&author, &prefs, &catalog(), &[], 1);
        assert_eq!(first.model, "big:latest");
        assert!(!first.same_model);
        let second = select(&author, &prefs, &catalog(), &[], 2);
        assert_eq!(
            second.model, "small:latest",
            "rounds rotate through the rest"
        );
        let third = select(&author, &prefs, &catalog(), &[], 3);
        assert_eq!(third.model, "big:latest");
    }

    #[test]
    fn configured_and_workspace_models_come_first_and_the_author_reviews_itself_last() {
        let prefs = Preferences {
            reasoning: Some("reasoner".into()),
            fast: Some("author".into()),
            ..Preferences::default()
        };
        let author = Author::Model("author".into());
        let picked = select(&author, &prefs, &catalog(), &["ops-reviewer".into()], 1);
        assert_eq!(picked.model, "ops-reviewer");
        let alone = select(
            &author,
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
        let sonnet = Author::Model("sonnet".into());

        assert_eq!(
            select(&sonnet, &prefs, &claude, &configured, 1),
            Reviewer {
                model: "opus".into(),
                same_model: false,
            }
        );
        assert_eq!(
            select(&sonnet, &prefs, &claude, &configured, 2).model,
            "haiku",
            "the agent's other models follow the ones configured"
        );
        assert_eq!(
            select(&sonnet, &prefs, &claude, &configured, 3).model,
            "opus"
        );
    }

    #[test]
    fn with_no_reviewer_named_a_change_written_on_sonnet_never_rotates_onto_fable() {
        let claude = Catalog::agent(AgentKind::Claude);
        let sonnet = Author::Model("sonnet".into());

        let rotation: Vec<String> = (1..=4)
            .map(|round| select(&sonnet, &Preferences::default(), &claude, &[], round).model)
            .collect();

        assert_eq!(rotation, ["opus", "haiku", "opus", "haiku"]);
    }

    #[test]
    fn fable_reviews_when_zone_auto_review_models_or_ai_settings_name_it() {
        let claude = Catalog::agent(AgentKind::Claude);
        let sonnet = Author::Model("sonnet".into());
        let fable = || Some("fable".to_string());

        assert_eq!(
            select(
                &sonnet,
                &Preferences::default(),
                &claude,
                &["fable".into()],
                1
            )
            .model,
            "fable",
            "ZONE_AUTO_REVIEW_MODELS=fable"
        );
        for prefs in [
            Preferences {
                reasoning: fable(),
                ..Preferences::default()
            },
            Preferences {
                fast: fable(),
                ..Preferences::default()
            },
        ] {
            assert_eq!(
                select(&sonnet, &prefs, &claude, &[], 1).model,
                "fable",
                "{prefs:?}"
            );
        }
    }

    #[test]
    fn a_run_left_to_the_agent_s_choice_is_never_reviewed_as_independent() {
        let claude = Catalog::agent(AgentKind::Claude);
        let author = Author::recorded(Some(stages::AUTO.into()));

        for round in 1..=3 {
            let reviewer = select(&author, &Preferences::default(), &claude, &[], round);
            assert!(
                reviewer.same_model,
                "round {round}: {reviewer:?} counted as independent of a model the agent chose"
            );
            assert_eq!(objection(&author, &reviewer), Some(CHOSEN_AUTHOR));
        }
    }

    #[test]
    fn an_agent_s_choice_is_not_called_an_endpoint_reviewer_s_own_model_nor_independent_of_it() {
        let author = Author::recorded(Some(stages::AUTO.into()));

        let reviewer = select(&author, &Preferences::default(), &catalog(), &[], 1);

        assert_eq!(
            reviewer,
            Reviewer {
                model: "big:latest".into(),
                same_model: false,
            }
        );
        assert_eq!(objection(&author, &reviewer), Some(CHOSEN_AUTHOR));
    }

    #[test]
    fn only_a_named_author_and_another_model_make_a_review_independent() {
        let claude = Catalog::agent(AgentKind::Claude);
        let sonnet = Author::Model("sonnet".into());
        let alone = Catalog {
            models: vec![installed("author", 5)],
            agent: None,
        };
        let author = Author::Model("author".into());

        let other = select(&sonnet, &Preferences::default(), &claude, &[], 1);
        assert_eq!(objection(&sonnet, &other), None, "{other:?}");
        let itself = select(&author, &Preferences::default(), &alone, &[], 1);
        assert_eq!(objection(&author, &itself), Some(NO_OTHER_MODEL));
        let unknown = select(
            &Author::Unrecorded,
            &Preferences::default(),
            &claude,
            &[],
            1,
        );
        assert!(!unknown.same_model);
        assert_eq!(
            objection(&Author::Unrecorded, &unknown),
            Some(UNRECORDED_AUTHOR)
        );
    }

    #[test]
    fn a_run_names_its_author_only_when_it_recorded_a_model() {
        assert_eq!(Author::recorded(Some(stages::AUTO.into())), Author::Chosen);
        assert_eq!(
            Author::recorded(Some("sonnet".into())),
            Author::Model("sonnet".into())
        );
        assert_eq!(Author::recorded(None), Author::Unrecorded);
        assert_eq!(Author::Chosen.model(), None);
        assert_eq!(Author::Model("sonnet".into()).model(), Some("sonnet"));
    }
}
