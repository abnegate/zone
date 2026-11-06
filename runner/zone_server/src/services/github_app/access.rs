use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Write,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_serializes_as_the_level_github_expects() {
        assert_eq!(serde_json::to_value(Access::Read).unwrap(), "read");
        assert_eq!(serde_json::to_value(Access::Write).unwrap(), "write");
    }
}
