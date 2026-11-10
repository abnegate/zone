use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Organization,
    Instance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_serialize_as_the_frontend_reads_them() {
        assert_eq!(
            serde_json::to_value([Source::Organization, Source::Instance]).unwrap(),
            serde_json::json!(["organization", "instance"])
        );
    }
}
