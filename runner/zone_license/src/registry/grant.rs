use crate::registry::access::Access;
use crate::registry::action::Action;
use crate::registry::repositories::Repositories;
use crate::registry::resource_type::ResourceType;
use crate::registry::scope::Scope;

const GRANTED_SEPARATOR: &str = " ";
const NAME_SEPARATOR: &str = ":";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    pub access: Vec<Access>,
    pub dropped: usize,
}

impl Grant {
    pub fn granted(&self) -> String {
        self.access
            .iter()
            .flat_map(|access| {
                access
                    .actions
                    .iter()
                    .map(|action| format!("{}{NAME_SEPARATOR}{action}", access.name))
            })
            .collect::<Vec<_>>()
            .join(GRANTED_SEPARATOR)
    }
}

pub fn grant(scopes: &[Scope], repositories: &Repositories) -> Grant {
    let mut granted = Grant::default();
    let mut refused: Vec<(&ResourceType, &str, &Action)> = Vec::new();
    for scope in scopes {
        let grantable =
            scope.kind == ResourceType::Repository && repositories.contains(&scope.name);
        for action in &scope.actions {
            if grantable && *action == Action::Pull {
                if !granted
                    .access
                    .iter()
                    .any(|access| access.name == scope.name)
                {
                    granted.access.push(Access {
                        kind: ResourceType::Repository,
                        name: scope.name.clone(),
                        actions: vec![Action::Pull],
                    });
                }
            } else {
                let request = (&scope.kind, scope.name.as_str(), action);
                if !refused.contains(&request) {
                    refused.push(request);
                }
            }
        }
    }
    granted.dropped = refused.len();
    granted
}
