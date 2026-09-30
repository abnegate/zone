//! Which model reviews a change: one that did not write it, when there is one.

use crate::services::backend;
use crate::services::endpoint::{self, Endpoint, Origin};
use crate::services::route::Route;
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
const NO_INSTALLED_TOOL_MODEL: &str = "no installed model can call tools, which a review session \
     offers; install one that can, or name one in ZONE_AUTO_REVIEW_MODELS";
const NO_SAVED_TOOL_MODEL: &str = "no model can call tools, which a review session offers; set \
     the Fast/Reasoning model in AI Settings to one that can";

/// Why no model can review a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Unavailable {
    #[error("no completion model is installed to review with")]
    NoModel,
    #[error("{}", no_tool_model(*.0))]
    NoToolModel(Origin),
    #[error(transparent)]
    Unset(#[from] endpoint::Error),
}

fn no_tool_model(origin: Origin) -> &'static str {
    match origin {
        Origin::Instance => NO_INSTALLED_TOOL_MODEL,
        Origin::Settings => NO_SAVED_TOOL_MODEL,
    }
}

/// How a person gives the reviews on an endpoint of `origin` another model:
/// `ZONE_AUTO_REVIEW_MODELS` names models on the instance's endpoint alone.
pub fn remedy(origin: Origin) -> &'static str {
    match origin {
        Origin::Instance => "name another in ZONE_AUTO_REVIEW_MODELS",
        Origin::Settings => "set the Fast/Reasoning model in AI Settings",
    }
}

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

/// The reviewers for `round` of a change `author` wrote, in the order the
/// round asks them: the first is the round's own, and each after it stands in
/// when those before it do not answer. Never empty.
///
/// Candidates in order: the operator's configured reviewers, the workspace's
/// reasoning and fast models, then everything installed, largest first. The
/// author is dropped, and so is any model the catalog lists as unable to call
/// tools, since a review session offers them; a name the catalog does not
/// list, or lists without its capabilities, stays. Successive rounds rotate
/// through what is left so a change that keeps coming back is read by
/// different eyes. With nothing left the author reviews itself, and says so,
/// unless it too cannot call tools.
///
/// On an agent, a candidate is a model the agent knows, and the agent's own
/// models stand in for what is installed, less those it runs only when named.
/// A change the agent wrote on a model of its own choosing may have come from
/// any of them. `origin` is the endpoint the reviews run on, which decides
/// where a person is told to name a model when none can review.
pub fn lineup(
    origin: Origin,
    author: &Author,
    preferences: &Preferences,
    catalog: &Catalog,
    configured: &[String],
    round: u32,
) -> Result<Vec<Reviewer>, Unavailable> {
    let named = author.model();
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.trim();
        if name.is_empty()
            || stages::is_auto(name)
            || !catalog.accepts(name)
            || catalog.refuses_tools(name)
        {
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
    if let Some(name) = preferences.reasoning.as_deref() {
        push(name);
    }
    if let Some(name) = preferences.fast.as_deref() {
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
        let fallbacks: Vec<&str> = [
            named,
            preferences.reasoning.as_deref(),
            preferences.fast.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|name| !stages::is_auto(name))
        .collect();
        return match fallbacks.iter().find(|name| !catalog.refuses_tools(name)) {
            Some(model) => Ok(vec![Reviewer {
                model: (*model).to_string(),
                same_model: true,
            }]),
            None if !fallbacks.is_empty() || catalog.completions().next().is_some() => {
                Err(Unavailable::NoToolModel(origin))
            }
            None => Err(Unavailable::NoModel),
        };
    }
    let index = usize::try_from(round.saturating_sub(1)).unwrap_or(0) % candidates.len();
    candidates.rotate_left(index);
    let same_model = *author == Author::Chosen && catalog.chooses();
    Ok(candidates
        .into_iter()
        .map(|model| Reviewer { model, same_model })
        .collect())
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

/// Where a workspace's reviews and merge summaries run, and what they may run
/// there, read from its settings once.
pub struct Venue {
    pub backend: LlmBackend,
    pub endpoint: Endpoint,
    pub preferences: Preferences,
    pub catalog: Catalog,
}

impl Venue {
    /// The venue the workspace's settings name, the instance's when it saves
    /// none, or why its saved endpoint cannot be used.
    pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Result<Self, backend::Error> {
        let config = state.config();
        let route = Route::for_workspace(state, workspace).await;
        let backend = route.backend(state).await?;
        let preferences = route.preferences(&config.comfyui.classifier_model);
        let endpoint = route.into_endpoint()?;
        let catalog = endpoint.catalog(&config.ollama_host, &backend).await;
        Ok(Self {
            backend,
            preferences,
            endpoint,
            catalog,
        })
    }

    /// The [`lineup`] for `round`. `configured` names models on the instance's
    /// endpoint, so an endpoint the settings name is reviewed on the Fast and
    /// Reasoning models the settings save, and on none when they save none.
    pub fn lineup(
        &self,
        author: &Author,
        configured: &[String],
        round: u32,
    ) -> Result<Vec<Reviewer>, Unavailable> {
        let configured = match self.endpoint.origin() {
            Origin::Instance => configured,
            Origin::Settings => {
                let saved = [
                    self.preferences.reasoning.as_deref(),
                    self.preferences.fast.as_deref(),
                ]
                .into_iter()
                .flatten()
                .find(|name| !stages::is_auto(name))
                .unwrap_or(stages::AUTO);
                self.endpoint.model(saved)?;
                &[]
            }
        };
        lineup(
            self.endpoint.origin(),
            author,
            &self.preferences,
            &self.catalog,
            configured,
            round,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ai_settings::EffectiveAiSettings;
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
            tools: None,
        }
    }

    fn without_tools(model: Installed) -> Installed {
        Installed {
            tools: Some(false),
            ..model
        }
    }

    fn with_tools(model: Installed) -> Installed {
        Installed {
            tools: Some(true),
            ..model
        }
    }

    fn pick(
        author: &Author,
        preferences: &Preferences,
        catalog: &Catalog,
        configured: &[String],
        round: u32,
    ) -> Reviewer {
        lineup(
            Origin::Instance,
            author,
            preferences,
            catalog,
            configured,
            round,
        )
        .expect("a model can review")
        .swap_remove(0)
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
        let preferences = Preferences::default();
        let author = Author::Model("author".into());
        let first = pick(&author, &preferences, &catalog(), &[], 1);
        assert_eq!(first.model, "big:latest");
        assert!(!first.same_model);
        let second = pick(&author, &preferences, &catalog(), &[], 2);
        assert_eq!(
            second.model, "small:latest",
            "rounds rotate through the rest"
        );
        let third = pick(&author, &preferences, &catalog(), &[], 3);
        assert_eq!(third.model, "big:latest");
    }

    #[test]
    fn the_round_after_a_failed_one_goes_to_the_next_configured_reviewer() {
        let author = Author::Model("author".into());
        let configured = ["gemma3:27b".to_string(), "qwen3:32b".to_string()];
        let failed = pick(&author, &Preferences::default(), &catalog(), &configured, 1);
        let next = pick(&author, &Preferences::default(), &catalog(), &configured, 2);
        assert_eq!(failed.model, "gemma3:27b");
        assert_eq!(next.model, "qwen3:32b");
    }

    #[test]
    fn a_round_s_lineup_starts_at_its_own_reviewer_and_goes_on_through_the_rest() {
        let author = Author::Model("author".into());
        let configured = ["gemma3:27b".to_string(), "qwen3:32b".to_string()];
        let names = |round| -> Vec<String> {
            lineup(
                Origin::Instance,
                &author,
                &Preferences::default(),
                &catalog(),
                &configured,
                round,
            )
            .expect("a model can review")
            .into_iter()
            .map(|reviewer| reviewer.model)
            .collect()
        };

        assert_eq!(
            names(1),
            ["gemma3:27b", "qwen3:32b", "big:latest", "small:latest"]
        );
        assert_eq!(
            names(2),
            ["qwen3:32b", "big:latest", "small:latest", "gemma3:27b"]
        );
    }

    #[test]
    fn configured_and_workspace_models_come_first_and_the_author_reviews_itself_last() {
        let preferences = Preferences {
            reasoning: Some("reasoner".into()),
            fast: Some("author".into()),
            ..Preferences::default()
        };
        let author = Author::Model("author".into());
        let picked = pick(
            &author,
            &preferences,
            &catalog(),
            &["ops-reviewer".into()],
            1,
        );
        assert_eq!(picked.model, "ops-reviewer");
        let alone = pick(
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
        let preferences = Preferences {
            reasoning: Some("llama3.1:70b".into()),
            ..Preferences::default()
        };
        let claude = Catalog::agent(AgentKind::Claude);
        let configured = ["qwen3:32b".to_string(), "opus".to_string()];
        let sonnet = Author::Model("sonnet".into());

        assert_eq!(
            pick(&sonnet, &preferences, &claude, &configured, 1),
            Reviewer {
                model: "opus".into(),
                same_model: false,
            }
        );
        assert_eq!(
            pick(&sonnet, &preferences, &claude, &configured, 2).model,
            "haiku",
            "the agent's other models follow the ones configured"
        );
        assert_eq!(
            pick(&sonnet, &preferences, &claude, &configured, 3).model,
            "opus"
        );
    }

    #[test]
    fn with_no_reviewer_named_a_change_written_on_sonnet_never_rotates_onto_fable() {
        let claude = Catalog::agent(AgentKind::Claude);
        let sonnet = Author::Model("sonnet".into());

        let rotation: Vec<String> = (1..=4)
            .map(|round| pick(&sonnet, &Preferences::default(), &claude, &[], round).model)
            .collect();

        assert_eq!(rotation, ["opus", "haiku", "opus", "haiku"]);
    }

    #[test]
    fn fable_reviews_when_zone_auto_review_models_or_ai_settings_name_it() {
        let claude = Catalog::agent(AgentKind::Claude);
        let sonnet = Author::Model("sonnet".into());
        let fable = || Some("fable".to_string());

        assert_eq!(
            pick(
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
        for preferences in [
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
                pick(&sonnet, &preferences, &claude, &[], 1).model,
                "fable",
                "{preferences:?}"
            );
        }
    }

    #[test]
    fn a_run_left_to_the_agent_s_choice_is_never_reviewed_as_independent() {
        let claude = Catalog::agent(AgentKind::Claude);
        let author = Author::recorded(Some(stages::AUTO.into()));

        for round in 1..=3 {
            let reviewer = pick(&author, &Preferences::default(), &claude, &[], round);
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

        let reviewer = pick(&author, &Preferences::default(), &catalog(), &[], 1);

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

        let other = pick(&sonnet, &Preferences::default(), &claude, &[], 1);
        assert_eq!(objection(&sonnet, &other), None, "{other:?}");
        let itself = pick(&author, &Preferences::default(), &alone, &[], 1);
        assert_eq!(objection(&author, &itself), Some(NO_OTHER_MODEL));
        let unknown = pick(
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

    const LLAVA: &str = "llava:7b";
    const NOROMAID: &str = "hf.co/Ttimofeyka/MistralRP-Noromaid-NSFW-Mistral-7B-GGUF:latest";

    fn catalog_with_models_that_cannot_call_tools() -> Catalog {
        Catalog {
            models: vec![
                without_tools(Installed {
                    vision: true,
                    ..installed(LLAVA, 4_733_363_377)
                }),
                with_tools(installed("qwen2.5:7b-instruct", 4_683_087_332)),
                without_tools(installed(NOROMAID, 4_140_374_100)),
                with_tools(installed("llama3.2:3b", 2_019_393_189)),
            ],
            agent: None,
        }
    }

    #[test]
    fn reviewers_rotate_only_onto_models_that_can_call_tools() {
        let author = Author::Model("qwen2.5:7b-instruct".into());
        let catalog = catalog_with_models_that_cannot_call_tools();

        let rotation: Vec<String> = (1..=4)
            .map(|round| pick(&author, &Preferences::default(), &catalog, &[], round).model)
            .collect();

        assert_eq!(rotation, ["llama3.2:3b"; 4]);
    }

    #[test]
    fn a_configured_or_preferred_model_that_cannot_call_tools_is_skipped() {
        let author = Author::Model("qwen2.5:7b-instruct".into());
        let preferences = Preferences {
            reasoning: Some(LLAVA.into()),
            fast: Some(NOROMAID.into()),
            ..Preferences::default()
        };

        let reviewer = pick(
            &author,
            &preferences,
            &catalog_with_models_that_cannot_call_tools(),
            &[LLAVA.into(), "ops-reviewer".into()],
            1,
        );

        assert_eq!(reviewer.model, "ops-reviewer");
    }

    #[test]
    fn a_model_whose_capabilities_are_unknown_still_reviews() {
        let author = Author::Model("author".into());
        let catalog = Catalog {
            models: vec![
                without_tools(installed(LLAVA, 9)),
                installed("unlisted-capabilities", 5),
                installed("author", 1),
            ],
            agent: None,
        };

        let reviewer = pick(&author, &Preferences::default(), &catalog, &[], 1);

        assert_eq!(reviewer.model, "unlisted-capabilities");
        assert!(!reviewer.same_model);
    }

    #[test]
    fn the_fallback_reviewer_is_never_a_model_that_cannot_call_tools() {
        let preferences = Preferences {
            reasoning: Some(LLAVA.into()),
            fast: Some("llama3.2:3b".into()),
            ..Preferences::default()
        };
        let catalog = Catalog {
            models: vec![
                without_tools(installed(LLAVA, 9)),
                installed("llama3.2:3b", 2),
            ],
            agent: None,
        };
        let author = Author::Model("llama3.2:3b".into());

        let reviewers = lineup(Origin::Instance, &author, &preferences, &catalog, &[], 1);

        assert_eq!(
            reviewers,
            Ok(vec![Reviewer {
                model: "llama3.2:3b".into(),
                same_model: true,
            }]),
            "the author reviews itself rather than handing the review to a model that cannot"
        );
    }

    #[test]
    fn with_only_models_that_cannot_call_tools_the_review_says_so() {
        let refusing = Catalog {
            models: vec![
                without_tools(installed(LLAVA, 9)),
                without_tools(installed(NOROMAID, 5)),
            ],
            agent: None,
        };
        let preferences = Preferences {
            reasoning: Some(LLAVA.into()),
            ..Preferences::default()
        };

        for (author, preferences) in [
            (Author::Unrecorded, preferences.clone()),
            (Author::Unrecorded, Preferences::default()),
            (Author::Model(NOROMAID.into()), Preferences::default()),
        ] {
            assert_eq!(
                lineup(
                    Origin::Instance,
                    &author,
                    &preferences,
                    &refusing,
                    &[LLAVA.into()],
                    1
                ),
                Err(Unavailable::NoToolModel(Origin::Instance)),
                "{author:?} {preferences:?}"
            );
        }
        assert_eq!(
            lineup(
                Origin::Instance,
                &Author::Unrecorded,
                &Preferences::default(),
                &Catalog::default(),
                &[],
                1
            ),
            Err(Unavailable::NoModel),
            "an endpoint that lists nothing has nothing to review with"
        );
    }

    #[test]
    fn a_missing_tool_model_is_named_where_the_endpoint_takes_one() {
        let instance = Unavailable::NoToolModel(Origin::Instance).to_string();
        let saved = Unavailable::NoToolModel(Origin::Settings).to_string();

        assert!(instance.contains("ZONE_AUTO_REVIEW_MODELS"), "{instance}");
        assert!(saved.contains("AI Settings"), "{saved}");
        assert!(!saved.contains("ZONE_AUTO_REVIEW_MODELS"), "{saved}");
    }

    fn saved_endpoint(
        fast: Option<&str>,
        reasoning: Option<&str>,
        configured: &[String],
    ) -> Result<Vec<Reviewer>, Unavailable> {
        let settings = EffectiveAiSettings {
            litellm_host: Some("http://models.example:4000".into()),
            model_fast: fast.map(str::to_string),
            model_reasoning: reasoning.map(str::to_string),
            ..crate::services::endpoint::testing::settings(
                zone_context::embeddings::providers::PROVIDER_SELF_HOSTED,
            )
        };
        let endpoint = Endpoint::resolve(&crate::state::test_config(), &settings);
        assert_eq!(endpoint.origin(), Origin::Settings);
        let venue = Venue {
            backend: LlmBackend::Http,
            preferences: Preferences::for_endpoint(&settings, "instance-classifier", &endpoint),
            endpoint,
            catalog: Catalog::default(),
        };
        venue.lineup(&Author::Unrecorded, configured, 1)
    }

    #[test]
    fn an_endpoint_the_settings_name_is_reviewed_only_on_the_models_they_save() {
        let configured = ["instance-reviewer".to_string()];

        let names: Vec<String> = saved_endpoint(Some("gpt-4o-mini"), Some("o3"), &configured)
            .expect("the saved models review")
            .into_iter()
            .map(|reviewer| reviewer.model)
            .collect();

        assert_eq!(names, ["o3", "gpt-4o-mini"]);
    }

    #[test]
    fn an_endpoint_the_settings_name_without_a_saved_model_has_nobody_to_review() {
        let configured = ["instance-reviewer".to_string()];

        for (fast, reasoning) in [(None, None), (Some(stages::AUTO), None)] {
            let reviewers = saved_endpoint(fast, reasoning, &configured);

            assert_eq!(
                reviewers,
                Err(Unavailable::Unset(endpoint::Error::ModelUnset)),
                "{fast:?} {reasoning:?}"
            );
            assert_eq!(
                reviewers.unwrap_err().to_string(),
                endpoint::Error::ModelUnset.to_string(),
                "the task pauses with the settings' own remedy"
            );
        }
    }
}
