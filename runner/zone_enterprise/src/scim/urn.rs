pub const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
pub const GROUP: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
pub const ENTERPRISE_USER: &str = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
pub const LIST_RESPONSE: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
pub const PATCH_OP: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
pub const ERROR: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
pub const SERVICE_PROVIDER_CONFIG: &str =
    "urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig";
pub const RESOURCE_TYPE: &str = "urn:ietf:params:scim:schemas:core:2.0:ResourceType";
pub const SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Schema";
pub const CONTENT_TYPE: &str = "application/scim+json";

pub(crate) fn strip_core(path: &str) -> &str {
    [USER, GROUP]
        .into_iter()
        .find_map(|urn| {
            let prefix = path.get(..urn.len())?;
            let rest = path.get(urn.len()..)?;
            (prefix.eq_ignore_ascii_case(urn) && rest.starts_with(':')).then(|| &rest[1..])
        })
        .unwrap_or(path)
}

pub(crate) fn is_urn(path: &str) -> bool {
    path.get(..4)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("urn:"))
}
