use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use uuid::Uuid;
use zone_enterprise::scim::Error;

use crate::services::scim::Failure;

pub const USER_PARAMETER: &str = "user_id";
pub const GROUP_PARAMETER: &str = "group_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identifier(pub Uuid);

impl<S> FromRequestParts<S> for Identifier
where
    S: Send + Sync,
{
    type Rejection = Failure;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(parameters) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|_| Failure::Protocol(Error::NotFound))?;
        identify(&parameters)
            .map(Self)
            .ok_or(Failure::Protocol(Error::NotFound))
    }
}

fn identify(parameters: &HashMap<String, String>) -> Option<Uuid> {
    [USER_PARAMETER, GROUP_PARAMETER]
        .into_iter()
        .find_map(|name| parameters.get(name))
        .and_then(|id| Uuid::parse_str(id).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_is_named_by_its_uuid() {
        let id = Uuid::new_v4();
        let user = HashMap::from([(USER_PARAMETER.to_owned(), id.to_string())]);
        let group = HashMap::from([(GROUP_PARAMETER.to_owned(), id.to_string())]);
        assert_eq!(identify(&user), Some(id));
        assert_eq!(identify(&group), Some(id));
        let malformed = HashMap::from([(USER_PARAMETER.to_owned(), "jyoung".to_owned())]);
        assert_eq!(identify(&malformed), None);
        assert_eq!(identify(&HashMap::new()), None);
    }
}
