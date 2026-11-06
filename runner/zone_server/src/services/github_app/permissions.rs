use serde::Serialize;

use super::access::Access;

#[derive(Debug, Clone, Serialize)]
pub struct Permissions {
    pub contents: Access,
    pub pull_requests: Access,
    pub metadata: Access,
    pub checks: Access,
    pub actions: Access,
    pub issues: Access,
    pub workflows: Access,
    pub statuses: Access,
    pub deployments: Access,
    pub repository_hooks: Access,
}

impl Permissions {
    pub fn zone() -> Self {
        Self {
            contents: Access::Write,
            pull_requests: Access::Write,
            metadata: Access::Read,
            checks: Access::Read,
            actions: Access::Read,
            issues: Access::Write,
            workflows: Access::Write,
            statuses: Access::Read,
            deployments: Access::Read,
            repository_hooks: Access::Write,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zone_asks_for_exactly_the_access_it_uses() {
        assert_eq!(
            serde_json::to_value(Permissions::zone()).unwrap(),
            json!({
                "contents": "write",
                "pull_requests": "write",
                "metadata": "read",
                "checks": "read",
                "actions": "read",
                "issues": "write",
                "workflows": "write",
                "statuses": "read",
                "deployments": "read",
                "repository_hooks": "write",
            })
        );
    }
}
