use serde::{Deserialize, Serialize};

use crate::policy::agent::Agent;
use crate::policy::domain;
use crate::policy::error::Error;
use crate::policy::field::Field;
use crate::policy::patch::Patch;
use crate::policy::pattern;
use crate::policy::provider::Provider;
use crate::policy::tier::Tier;

const SERVER_MAXIMUM_LENGTH: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub providers: Option<Vec<Provider>>,
    pub agents: Option<Vec<Agent>>,
    pub models: Option<Vec<String>>,
    pub ceiling: Option<Tier>,
    pub mcp_servers: Option<Vec<String>>,
    pub domains: Option<Vec<String>>,
}

impl Rules {
    pub fn normalized(self) -> Result<Rules, Error> {
        if self
            .ceiling
            .is_some_and(|ceiling| !Tier::CEILINGS.contains(&ceiling))
        {
            return Err(Error::CeilingInvalid);
        }
        Ok(Rules {
            providers: self.providers.map(sorted),
            agents: self.agents.map(sorted),
            models: self
                .models
                .map(|models| normalize(models, model, Error::ModelInvalid))
                .transpose()?,
            ceiling: self.ceiling,
            mcp_servers: self
                .mcp_servers
                .map(|servers| normalize(servers, server, Error::ServerInvalid))
                .transpose()?,
            domains: self
                .domains
                .map(|domains| normalize(domains, domain::normalized, Error::DomainInvalid))
                .transpose()?,
        })
    }

    pub fn patched(&self, patch: &Patch) -> Rules {
        Rules {
            providers: replaced(&self.providers, &patch.providers),
            agents: replaced(&self.agents, &patch.agents),
            models: replaced(&self.models, &patch.models),
            ceiling: replaced(&self.ceiling, &patch.ceiling),
            mcp_servers: replaced(&self.mcp_servers, &patch.mcp_servers),
            domains: replaced(&self.domains, &patch.domains),
        }
    }

    pub fn within(&self, organization: &Rules, fields: &[Field]) -> Result<(), Error> {
        match fields
            .iter()
            .find(|field| !self.narrows(organization, **field))
        {
            Some(field) => Err(Error::Wider(*field)),
            None => Ok(()),
        }
    }

    fn narrows(&self, organization: &Rules, field: Field) -> bool {
        match field {
            Field::Providers => covered(&self.providers, &organization.providers, PartialEq::eq),
            Field::Agents => covered(&self.agents, &organization.agents, PartialEq::eq),
            Field::Models => covered(&self.models, &organization.models, |outer, inner| {
                pattern::covers(outer, inner)
            }),
            Field::Ceiling => match (self.ceiling, organization.ceiling) {
                (Some(own), Some(limit)) => own <= limit,
                _ => true,
            },
            Field::McpServers => {
                covered(&self.mcp_servers, &organization.mcp_servers, PartialEq::eq)
            }
            Field::Domains => covered(&self.domains, &organization.domains, |outer, inner| {
                domain::covers(outer, inner)
            }),
        }
    }
}

pub(super) fn sorted<T: Ord>(mut entries: Vec<T>) -> Vec<T> {
    entries.sort_unstable();
    entries.dedup();
    entries
}

fn normalize(
    entries: Vec<String>,
    normalized: impl Fn(&str) -> Option<String>,
    invalid: impl Fn(String) -> Error,
) -> Result<Vec<String>, Error> {
    entries
        .iter()
        .map(|entry| normalized(entry).ok_or_else(|| invalid(entry.trim().to_owned())))
        .collect::<Result<Vec<String>, Error>>()
        .map(sorted)
}

fn model(entry: &str) -> Option<String> {
    let entry = entry.trim();
    pattern::valid(entry).then(|| entry.to_owned())
}

fn server(entry: &str) -> Option<String> {
    let entry = entry.trim();
    let valid = (1..=SERVER_MAXIMUM_LENGTH).contains(&entry.len())
        && entry.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        });
    valid.then(|| entry.to_owned())
}

fn replaced<T: Clone>(current: &Option<T>, change: &Option<Option<T>>) -> Option<T> {
    match change {
        Some(value) => value.clone(),
        None => current.clone(),
    }
}

fn covered<T>(
    own: &Option<Vec<T>>,
    limit: &Option<Vec<T>>,
    covers: impl Fn(&T, &T) -> bool,
) -> bool {
    match (own, limit) {
        (Some(own), Some(limit)) => own
            .iter()
            .all(|entry| limit.iter().any(|outer| covers(outer, entry))),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn organization() -> Rules {
        Rules {
            providers: Some(vec![Provider::Anthropic, Provider::Bedrock]),
            agents: Some(vec![Agent::Claude]),
            models: Some(vec!["claude-*".into(), "sonnet".into()]),
            ceiling: Some(Tier::Host),
            mcp_servers: Some(vec!["github".into(), "linear".into()]),
            domains: Some(vec!["example.com".into(), "10.0.0.1".into()]),
        }
    }

    fn invalid(rules: Rules) -> &'static str {
        rules.normalized().unwrap_err().code()
    }

    #[test]
    fn an_empty_object_restricts_nothing() {
        let rules: Rules = serde_json::from_str("{}").unwrap();
        assert_eq!(rules, Rules::default());
    }

    #[test]
    fn always_serializes_all_six_keys() {
        assert_eq!(
            serde_json::to_value(Rules::default()).unwrap(),
            json!({
                "providers": null,
                "agents": null,
                "models": null,
                "ceiling": null,
                "mcp_servers": null,
                "domains": null,
            })
        );
        assert_eq!(
            serde_json::to_value(organization()).unwrap(),
            json!({
                "providers": ["anthropic", "bedrock"],
                "agents": ["claude"],
                "models": ["claude-*", "sonnet"],
                "ceiling": "host",
                "mcp_servers": ["github", "linear"],
                "domains": ["example.com", "10.0.0.1"],
            })
        );
    }

    #[test]
    fn round_trips_and_ignores_unknown_keys() {
        let value = serde_json::to_value(organization()).unwrap();
        assert_eq!(
            serde_json::from_value::<Rules>(value).unwrap(),
            organization()
        );
        let rules: Rules = serde_json::from_value(json!({"agents": [], "future": true})).unwrap();
        assert_eq!(rules.agents, Some(vec![]));
        assert_eq!(rules.providers, None);
    }

    #[test]
    fn refuses_unknown_values() {
        for body in [
            json!({"providers": ["gemini"]}),
            json!({"agents": ["claude_code"]}),
            json!({"ceiling": "none"}),
        ] {
            assert!(
                serde_json::from_value::<Rules>(body.clone()).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn normalizes_trims_sorts_and_dedupes() {
        let rules = Rules {
            providers: Some(vec![Provider::Other, Provider::Local, Provider::Other]),
            agents: Some(vec![Agent::Codex, Agent::Claude, Agent::Codex]),
            models: Some(vec![" sonnet ".into(), "claude-*".into(), "sonnet".into()]),
            ceiling: Some(Tier::Write),
            mcp_servers: Some(vec!["linear".into(), " github ".into(), "linear".into()]),
            domains: Some(vec![
                "*.Example.com".into(),
                "api.example.com.".into(),
                "example.com".into(),
                "[::1]".into(),
            ]),
        }
        .normalized()
        .unwrap();
        assert_eq!(
            rules,
            Rules {
                providers: Some(vec![Provider::Local, Provider::Other]),
                agents: Some(vec![Agent::Claude, Agent::Codex]),
                models: Some(vec!["claude-*".into(), "sonnet".into()]),
                ceiling: Some(Tier::Write),
                mcp_servers: Some(vec!["github".into(), "linear".into()]),
                domains: Some(vec![
                    "::1".into(),
                    "api.example.com".into(),
                    "example.com".into(),
                ]),
            }
        );
    }

    #[test]
    fn keeps_unrestricted_and_empty_lists_apart() {
        let empty = Rules {
            providers: Some(vec![]),
            models: Some(vec![]),
            domains: Some(vec![]),
            ..Rules::default()
        };
        assert_eq!(empty.clone().normalized().unwrap(), empty);
        assert_eq!(Rules::default().normalized().unwrap(), Rules::default());
    }

    #[test]
    fn accepts_write_and_host_ceilings() {
        for ceiling in [Tier::Write, Tier::Host] {
            let rules = Rules {
                ceiling: Some(ceiling),
                ..Rules::default()
            };
            assert_eq!(rules.clone().normalized().unwrap(), rules);
        }
    }

    #[test]
    fn refuses_read_and_outward_ceilings() {
        for ceiling in [Tier::Read, Tier::Outward] {
            let rules = Rules {
                ceiling: Some(ceiling),
                ..Rules::default()
            };
            assert_eq!(rules.normalized(), Err(Error::CeilingInvalid));
        }
        let read: Rules = serde_json::from_value(json!({"ceiling": "read"})).unwrap();
        assert_eq!(invalid(read), "policy_ceiling_invalid");
        let outward: Rules = serde_json::from_value(json!({"ceiling": "outward"})).unwrap();
        assert_eq!(invalid(outward), "policy_ceiling_invalid");
    }

    #[test]
    fn refuses_invalid_model_patterns_naming_the_entry() {
        for entry in [
            "",
            "  ",
            "claude opus",
            "*-opus",
            "claude-**",
            &"m".repeat(129),
        ] {
            let rules = Rules {
                models: Some(vec!["sonnet".into(), entry.to_owned()]),
                ..Rules::default()
            };
            assert_eq!(
                rules.normalized(),
                Err(Error::ModelInvalid(entry.trim().to_owned())),
                "{entry:?}"
            );
        }
        assert_eq!(
            invalid(Rules {
                models: Some(vec!["a b".into()]),
                ..Rules::default()
            }),
            "policy_model_invalid"
        );
    }

    #[test]
    fn refuses_invalid_domains_naming_the_entry() {
        for entry in [
            "https://example.com",
            "example.com:8080",
            "example.com/v1",
            " exa_mple.com ",
            "",
        ] {
            let rules = Rules {
                domains: Some(vec![entry.to_owned()]),
                ..Rules::default()
            };
            assert_eq!(
                rules.normalized(),
                Err(Error::DomainInvalid(entry.trim().to_owned())),
                "{entry:?}"
            );
        }
        assert_eq!(
            invalid(Rules {
                domains: Some(vec!["*".into()]),
                ..Rules::default()
            }),
            "policy_domain_invalid"
        );
    }

    #[test]
    fn refuses_invalid_server_names_naming_the_entry() {
        for entry in [
            "",
            "git hub",
            "github.com",
            "linear/tools",
            "é",
            &"s".repeat(33),
        ] {
            let rules = Rules {
                mcp_servers: Some(vec![entry.to_owned()]),
                ..Rules::default()
            };
            assert_eq!(
                rules.normalized(),
                Err(Error::ServerInvalid(entry.trim().to_owned())),
                "{entry:?}"
            );
        }
        assert_eq!(
            invalid(Rules {
                mcp_servers: Some(vec!["a.b".into()]),
                ..Rules::default()
            }),
            "policy_mcp_server_invalid"
        );
        let valid = Rules {
            mcp_servers: Some(vec!["My_Server-2".into(), "s".repeat(32)]),
            ..Rules::default()
        };
        assert!(valid.normalized().is_ok());
    }

    #[test]
    fn a_missing_key_leaves_the_field_unchanged() {
        let patch: Patch = serde_json::from_str("{}").unwrap();
        assert_eq!(organization().patched(&patch), organization());
    }

    #[test]
    fn null_clears_the_field() {
        let patch: Patch = serde_json::from_value(json!({
            "providers": null,
            "ceiling": null,
            "domains": null,
        }))
        .unwrap();
        assert_eq!(
            organization().patched(&patch),
            Rules {
                providers: None,
                ceiling: None,
                domains: None,
                ..organization()
            }
        );
    }

    #[test]
    fn a_value_replaces_the_field() {
        let patch: Patch = serde_json::from_value(json!({
            "agents": ["codex"],
            "models": [],
            "ceiling": "write",
            "mcp_servers": ["sentry"],
        }))
        .unwrap();
        assert_eq!(
            organization().patched(&patch),
            Rules {
                agents: Some(vec![Agent::Codex]),
                models: Some(vec![]),
                ceiling: Some(Tier::Write),
                mcp_servers: Some(vec!["sentry".into()]),
                ..organization()
            }
        );
        assert_eq!(
            Rules::default().patched(&patch).providers,
            None,
            "an unrestricted field stays unrestricted"
        );
    }

    #[test]
    fn anything_is_within_an_unrestricted_organization() {
        assert_eq!(
            organization().within(&Rules::default(), &Field::ALL),
            Ok(())
        );
    }

    #[test]
    fn an_unrestricted_workspace_is_within_any_organization() {
        assert_eq!(
            Rules::default().within(&organization(), &Field::ALL),
            Ok(())
        );
    }

    #[test]
    fn narrower_rules_are_within() {
        let workspace = Rules {
            providers: Some(vec![Provider::Bedrock]),
            agents: Some(vec![]),
            models: Some(vec![
                "claude-opus-*".into(),
                "claude-haiku".into(),
                "sonnet".into(),
            ]),
            ceiling: Some(Tier::Write),
            mcp_servers: Some(vec!["github".into()]),
            domains: Some(vec!["api.example.com".into(), "10.0.0.1".into()]),
        };
        assert_eq!(workspace.within(&organization(), &Field::ALL), Ok(()));
        assert_eq!(organization().within(&organization(), &Field::ALL), Ok(()));
    }

    #[test]
    fn each_wider_field_is_refused() {
        for (field, workspace) in [
            (
                Field::Providers,
                Rules {
                    providers: Some(vec![Provider::Bedrock, Provider::OpenAI]),
                    ..Rules::default()
                },
            ),
            (
                Field::Agents,
                Rules {
                    agents: Some(vec![Agent::Codex]),
                    ..Rules::default()
                },
            ),
            (
                Field::Models,
                Rules {
                    models: Some(vec!["claude*".into()]),
                    ..Rules::default()
                },
            ),
            (
                Field::Models,
                Rules {
                    models: Some(vec!["sonnet-*".into()]),
                    ..Rules::default()
                },
            ),
            (
                Field::Ceiling,
                Rules {
                    ceiling: Some(Tier::Outward),
                    ..Rules::default()
                },
            ),
            (
                Field::McpServers,
                Rules {
                    mcp_servers: Some(vec!["sentry".into()]),
                    ..Rules::default()
                },
            ),
            (
                Field::Domains,
                Rules {
                    domains: Some(vec!["com".into()]),
                    ..Rules::default()
                },
            ),
            (
                Field::Domains,
                Rules {
                    domains: Some(vec!["10.0.0.10".into()]),
                    ..Rules::default()
                },
            ),
        ] {
            assert_eq!(
                workspace.within(&organization(), &Field::ALL),
                Err(Error::Wider(field)),
                "{workspace:?}"
            );
            assert_eq!(
                workspace
                    .within(&organization(), &[field])
                    .unwrap_err()
                    .code(),
                "policy_wider_than_organization"
            );
        }
    }

    #[test]
    fn checks_only_the_listed_fields() {
        let workspace = Rules {
            providers: Some(vec![Provider::OpenAI]),
            agents: Some(vec![Agent::Claude]),
            ceiling: Some(Tier::Write),
            domains: Some(vec!["evil.test".into()]),
            ..Rules::default()
        };
        assert_eq!(
            workspace.within(&organization(), &[Field::Agents, Field::Ceiling]),
            Ok(())
        );
        assert_eq!(workspace.within(&organization(), &[]), Ok(()));
        assert_eq!(
            workspace.within(&organization(), &[Field::Domains, Field::Providers]),
            Err(Error::Wider(Field::Domains))
        );
    }

    #[test]
    fn an_organization_that_allows_nothing_admits_only_empty_lists() {
        let nothing = Rules {
            providers: Some(vec![]),
            models: Some(vec![]),
            ..Rules::default()
        };
        let empty = Rules {
            providers: Some(vec![]),
            models: Some(vec![]),
            ..Rules::default()
        };
        assert_eq!(empty.within(&nothing, &Field::ALL), Ok(()));
        let wider = Rules {
            models: Some(vec!["sonnet".into()]),
            ..Rules::default()
        };
        assert_eq!(
            wider.within(&nothing, &Field::ALL),
            Err(Error::Wider(Field::Models))
        );
    }
}
