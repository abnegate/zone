use crate::policy::agent::Agent;
use crate::policy::denial::Denial;
use crate::policy::domain;
use crate::policy::pattern;
use crate::policy::provider::Provider;
use crate::policy::rules::{Rules, sorted};
use crate::policy::tier::Tier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    organization: Rules,
    workspace: Rules,
}

impl Effective {
    pub fn new(organization: Rules, workspace: Rules) -> Self {
        Self {
            organization,
            workspace,
        }
    }

    pub fn provider(&self, provider: Provider) -> Result<(), Denial> {
        self.admit(
            |rules| rules.providers.as_deref(),
            |allowed| *allowed == provider,
            Denial::Provider(provider),
        )
    }

    pub fn agent(&self, agent: Agent) -> Result<(), Denial> {
        self.admit(
            |rules| rules.agents.as_deref(),
            |allowed| *allowed == agent,
            Denial::Agent(agent),
        )
    }

    pub fn model(&self, model: &str) -> Result<(), Denial> {
        self.admit(
            |rules| rules.models.as_deref(),
            |pattern| pattern::matches(pattern, model),
            Denial::Model,
        )
    }

    pub fn server(&self, server: &str) -> Result<(), Denial> {
        self.admit(
            |rules| rules.mcp_servers.as_deref(),
            |allowed| allowed == server,
            Denial::Server,
        )
    }

    pub fn domain(&self, host: &str) -> Result<(), Denial> {
        self.admit(
            |rules| rules.domains.as_deref(),
            |entry| domain::matches(entry, host),
            Denial::Domain,
        )
    }

    pub fn ceiling(&self) -> Option<Tier> {
        self.layers()
            .into_iter()
            .filter_map(|rules| rules.ceiling)
            .min()
    }

    pub fn restricts_providers(&self) -> bool {
        self.layers()
            .into_iter()
            .any(|rules| rules.providers.is_some())
    }

    pub fn restricts_network(&self) -> bool {
        self.layers()
            .into_iter()
            .any(|rules| rules.domains.is_some())
    }

    pub fn rules(&self) -> Rules {
        let (organization, workspace) = (&self.organization, &self.workspace);
        Rules {
            providers: intersection(&organization.providers, &workspace.providers, PartialEq::eq),
            agents: intersection(&organization.agents, &workspace.agents, PartialEq::eq),
            models: intersection(&organization.models, &workspace.models, |outer, inner| {
                pattern::covers(outer, inner)
            }),
            ceiling: self.ceiling(),
            mcp_servers: intersection(
                &organization.mcp_servers,
                &workspace.mcp_servers,
                PartialEq::eq,
            ),
            domains: intersection(&organization.domains, &workspace.domains, |outer, inner| {
                domain::covers(outer, inner)
            }),
        }
    }

    fn layers(&self) -> [&Rules; 2] {
        [&self.organization, &self.workspace]
    }

    fn admit<T>(
        &self,
        entries: impl Fn(&Rules) -> Option<&[T]>,
        allows: impl Fn(&T) -> bool,
        denial: Denial,
    ) -> Result<(), Denial> {
        let admitted = self
            .layers()
            .into_iter()
            .all(|rules| entries(rules).is_none_or(|entries| entries.iter().any(&allows)));
        if admitted { Ok(()) } else { Err(denial) }
    }
}

fn intersection<T: Clone + Ord>(
    organization: &Option<Vec<T>>,
    workspace: &Option<Vec<T>>,
    covers: impl Fn(&T, &T) -> bool,
) -> Option<Vec<T>> {
    match (organization, workspace) {
        (Some(organization), Some(workspace)) => {
            let within = |entries: &[T], outer: &[T]| -> Vec<T> {
                entries
                    .iter()
                    .filter(|entry| outer.iter().any(|limit| covers(limit, entry)))
                    .cloned()
                    .collect()
            };
            let mut entries = within(workspace, organization);
            entries.extend(within(organization, workspace));
            Some(sorted(entries))
        }
        (Some(organization), None) => Some(organization.clone()),
        (None, workspace) => workspace.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organization() -> Rules {
        Rules {
            providers: Some(vec![
                Provider::Anthropic,
                Provider::Bedrock,
                Provider::Local,
            ]),
            agents: Some(vec![Agent::Claude]),
            models: Some(vec!["claude-*".into(), "sonnet".into()]),
            ceiling: Some(Tier::Host),
            mcp_servers: Some(vec!["github".into(), "linear".into()]),
            domains: Some(vec!["example.com".into(), "10.0.0.1".into()]),
        }
    }

    fn workspace() -> Rules {
        Rules {
            providers: Some(vec![Provider::Bedrock, Provider::Local]),
            agents: Some(vec![Agent::Claude, Agent::Codex]),
            models: Some(vec!["claude-opus-*".into(), "sonnet".into()]),
            ceiling: Some(Tier::Write),
            mcp_servers: Some(vec!["github".into()]),
            domains: Some(vec!["api.example.com".into()]),
        }
    }

    #[test]
    fn neither_layer_restricts_anything() {
        let effective = Effective::new(Rules::default(), Rules::default());
        for provider in Provider::ALL {
            assert_eq!(effective.provider(provider), Ok(()));
        }
        for agent in Agent::ALL {
            assert_eq!(effective.agent(agent), Ok(()));
        }
        assert_eq!(effective.model("anything"), Ok(()));
        assert_eq!(effective.server("anything"), Ok(()));
        assert_eq!(effective.domain("evil.test"), Ok(()));
        assert_eq!(effective.ceiling(), None);
        assert!(!effective.restricts_providers());
        assert!(!effective.restricts_network());
        assert_eq!(effective.rules(), Rules::default());
    }

    #[test]
    fn the_organization_layer_alone_restricts() {
        let effective = Effective::new(organization(), Rules::default());
        assert_eq!(effective.provider(Provider::Bedrock), Ok(()));
        assert_eq!(
            effective.provider(Provider::OpenAI),
            Err(Denial::Provider(Provider::OpenAI))
        );
        assert_eq!(effective.agent(Agent::Claude), Ok(()));
        assert_eq!(
            effective.agent(Agent::Codex),
            Err(Denial::Agent(Agent::Codex))
        );
        assert_eq!(effective.model("claude-haiku-4-5"), Ok(()));
        assert_eq!(effective.model("sonnet"), Ok(()));
        assert_eq!(effective.model("gpt-5"), Err(Denial::Model));
        assert_eq!(effective.server("linear"), Ok(()));
        assert_eq!(effective.server("sentry"), Err(Denial::Server));
        assert_eq!(effective.domain("docs.example.com"), Ok(()));
        assert_eq!(effective.domain("10.0.0.1"), Ok(()));
        assert_eq!(effective.domain("evil.test"), Err(Denial::Domain));
        assert_eq!(effective.ceiling(), Some(Tier::Host));
        assert!(effective.restricts_providers());
        assert!(effective.restricts_network());
        assert_eq!(effective.rules(), organization());
    }

    #[test]
    fn the_workspace_layer_alone_restricts() {
        let effective = Effective::new(Rules::default(), workspace());
        assert_eq!(effective.provider(Provider::Local), Ok(()));
        assert_eq!(
            effective.provider(Provider::Anthropic),
            Err(Denial::Provider(Provider::Anthropic))
        );
        assert_eq!(effective.agent(Agent::Codex), Ok(()));
        assert_eq!(effective.model("claude-opus-4-1"), Ok(()));
        assert_eq!(effective.model("claude-haiku-4-5"), Err(Denial::Model));
        assert_eq!(effective.server("linear"), Err(Denial::Server));
        assert_eq!(effective.domain("api.example.com"), Ok(()));
        assert_eq!(effective.domain("example.com"), Err(Denial::Domain));
        assert_eq!(effective.ceiling(), Some(Tier::Write));
        assert_eq!(effective.rules(), workspace());
    }

    #[test]
    fn both_layers_must_admit() {
        let effective = Effective::new(organization(), workspace());
        assert_eq!(effective.provider(Provider::Bedrock), Ok(()));
        assert_eq!(
            effective.provider(Provider::Anthropic),
            Err(Denial::Provider(Provider::Anthropic))
        );
        assert_eq!(effective.agent(Agent::Claude), Ok(()));
        assert_eq!(
            effective.agent(Agent::Codex),
            Err(Denial::Agent(Agent::Codex))
        );
        assert_eq!(effective.model("claude-opus-4-1"), Ok(()));
        assert_eq!(effective.model("sonnet"), Ok(()));
        assert_eq!(effective.model("claude-haiku-4-5"), Err(Denial::Model));
        assert_eq!(effective.server("github"), Ok(()));
        assert_eq!(effective.server("linear"), Err(Denial::Server));
        assert_eq!(effective.domain("v2.api.example.com"), Ok(()));
        assert_eq!(effective.domain("example.com"), Err(Denial::Domain));
        assert_eq!(effective.domain("10.0.0.1"), Err(Denial::Domain));
    }

    #[test]
    fn an_empty_list_admits_nothing() {
        let nothing = Rules {
            providers: Some(vec![]),
            agents: Some(vec![]),
            models: Some(vec![]),
            mcp_servers: Some(vec![]),
            domains: Some(vec![]),
            ..Rules::default()
        };
        let effective = Effective::new(Rules::default(), nothing);
        assert_eq!(
            effective.provider(Provider::Local),
            Err(Denial::Provider(Provider::Local))
        );
        assert_eq!(
            effective.agent(Agent::Claude),
            Err(Denial::Agent(Agent::Claude))
        );
        assert_eq!(effective.model("sonnet"), Err(Denial::Model));
        assert_eq!(effective.server("github"), Err(Denial::Server));
        assert_eq!(effective.domain("example.com"), Err(Denial::Domain));
        assert!(effective.restricts_providers());
        assert!(effective.restricts_network());
    }

    #[test]
    fn auto_is_admitted_only_where_models_are_unset_or_name_it() {
        let models = |models: &[&str]| Rules {
            models: Some(models.iter().map(|&model| model.to_owned()).collect()),
            ..Rules::default()
        };
        assert_eq!(
            Effective::new(Rules::default(), Rules::default()).model("auto"),
            Ok(())
        );
        for wildcard in [&["*"][..], &["a*"], &["claude-*", "au*"]] {
            let effective = Effective::new(models(wildcard), Rules::default());
            assert_eq!(effective.model("auto"), Err(Denial::Model), "{wildcard:?}");
            assert_eq!(effective.model(""), Err(Denial::Model), "{wildcard:?}");
        }
        let named = Effective::new(models(&["auto", "claude-*"]), Rules::default());
        assert_eq!(named.model("auto"), Ok(()));
        let narrowed = Effective::new(models(&["auto", "claude-*"]), models(&["claude-*"]));
        assert_eq!(narrowed.model("auto"), Err(Denial::Model));
        assert_eq!(narrowed.rules().models, Some(vec!["claude-*".to_owned()]));
        let widened = Effective::new(models(&["*"]), models(&["auto"]));
        assert_eq!(widened.model("auto"), Err(Denial::Model));
        assert_eq!(widened.rules().models, Some(vec![]));
    }

    #[test]
    fn the_lower_ceiling_wins() {
        for (organization, workspace, expected) in [
            (Some(Tier::Host), Some(Tier::Write), Some(Tier::Write)),
            (Some(Tier::Write), Some(Tier::Host), Some(Tier::Write)),
            (Some(Tier::Host), None, Some(Tier::Host)),
            (None, Some(Tier::Write), Some(Tier::Write)),
            (None, None, None),
        ] {
            let effective = Effective::new(
                Rules {
                    ceiling: organization,
                    ..Rules::default()
                },
                Rules {
                    ceiling: workspace,
                    ..Rules::default()
                },
            );
            assert_eq!(
                effective.ceiling(),
                expected,
                "{organization:?} and {workspace:?}"
            );
            assert_eq!(effective.rules().ceiling, expected);
        }
    }

    #[test]
    fn either_layer_restricts_providers_or_the_network() {
        let providers = Rules {
            providers: Some(vec![Provider::Local]),
            ..Rules::default()
        };
        let domains = Rules {
            domains: Some(vec![]),
            ..Rules::default()
        };
        let effective = Effective::new(Rules::default(), providers.clone());
        assert!(effective.restricts_providers() && !effective.restricts_network());
        let effective = Effective::new(domains.clone(), Rules::default());
        assert!(!effective.restricts_providers() && effective.restricts_network());
        let effective = Effective::new(providers, domains);
        assert!(effective.restricts_providers() && effective.restricts_network());
    }

    #[test]
    fn the_view_intersects_both_layers() {
        let effective = Effective::new(organization(), workspace());
        assert_eq!(
            effective.rules(),
            Rules {
                providers: Some(vec![Provider::Local, Provider::Bedrock]),
                agents: Some(vec![Agent::Claude]),
                models: Some(vec!["claude-opus-*".into(), "sonnet".into()]),
                ceiling: Some(Tier::Write),
                mcp_servers: Some(vec!["github".into()]),
                domains: Some(vec!["api.example.com".into()]),
            }
        );
    }

    #[test]
    fn the_view_keeps_what_both_layers_admit_when_the_workspace_is_wider() {
        let organization = Rules {
            models: Some(vec!["claude-opus".into(), "gpt-5".into()]),
            domains: Some(vec!["api.example.com".into()]),
            ..Rules::default()
        };
        let workspace = Rules {
            models: Some(vec!["claude-*".into(), "sonnet".into()]),
            domains: Some(vec!["example.com".into(), "evil.test".into()]),
            ..Rules::default()
        };
        let effective = Effective::new(organization, workspace);
        let rules = effective.rules();
        assert_eq!(rules.models, Some(vec!["claude-opus".to_owned()]));
        assert_eq!(rules.domains, Some(vec!["api.example.com".to_owned()]));
        for model in rules.models.unwrap() {
            assert_eq!(effective.model(&model), Ok(()));
        }
        assert_eq!(effective.model("sonnet"), Err(Denial::Model));
    }
}
