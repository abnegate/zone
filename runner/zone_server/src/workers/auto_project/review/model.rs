//! Which model reviews a change: one that did not write it, when there is one.

use crate::db::{ai_settings, workspaces};
use crate::services::stages::{self, Catalog, Preferences};
use crate::state::AppState;
use uuid::Uuid;

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
        if name.is_empty() || stages::is_auto(name) {
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

/// The workspace's model preferences and what is installed, read the way a
/// run reads them.
pub async fn preferences(state: &AppState, workspace_id: Uuid) -> (Preferences, Catalog) {
    let catalog = Catalog::load(&state.config().ollama_host).await;
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
            },
            &[],
            1,
        );
        assert_eq!(alone.model, "author");
        assert!(alone.same_model);
    }
}
