use serde_json::{Value, json};

use crate::scim::list_response::ListResponse;
use crate::scim::meta::{GROUP_RESOURCE, USER_RESOURCE};
use crate::scim::page::Page;
use crate::scim::urn;

const SERVICE_PROVIDER_CONFIG_RESOURCE: &str = "ServiceProviderConfig";
const RESOURCE_TYPE_RESOURCE: &str = "ResourceType";
const SCHEMA_RESOURCE: &str = "Schema";
const UNIQUE: &str = "server";
const NOT_UNIQUE: &str = "none";
const READ_WRITE: &str = "readWrite";
const READ_ONLY: &str = "readOnly";

pub fn service_provider_config(base_url: &str) -> Value {
    json!({
        "schemas": [urn::SERVICE_PROVIDER_CONFIG],
        "patch": { "supported": true },
        "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
        "filter": { "supported": true, "maxResults": Page::MAX_RESULTS },
        "changePassword": { "supported": false },
        "sort": { "supported": false },
        "etag": { "supported": false },
        "authenticationSchemes": [{
            "type": "oauthbearertoken",
            "name": "OAuth Bearer Token",
            "description": "A SCIM token created in the Zone console, sent as a bearer token in the Authorization header.",
            "primary": true,
        }],
        "meta": {
            "resourceType": SERVICE_PROVIDER_CONFIG_RESOURCE,
            "location": format!("{}/{SERVICE_PROVIDER_CONFIG_RESOURCE}", trimmed(base_url)),
        },
    })
}

pub fn resource_types(base_url: &str) -> ListResponse<Value> {
    let types = vec![user_type(base_url), group_type(base_url)];
    let total = u64::try_from(types.len()).unwrap_or(u64::MAX);
    ListResponse::new(types, total, 1)
}

pub fn resource_type(base_url: &str, name: &str) -> Option<Value> {
    if name.eq_ignore_ascii_case(USER_RESOURCE) {
        Some(user_type(base_url))
    } else if name.eq_ignore_ascii_case(GROUP_RESOURCE) {
        Some(group_type(base_url))
    } else {
        None
    }
}

pub fn schemas() -> ListResponse<Value> {
    let schemas = vec![user_schema(), group_schema()];
    let total = u64::try_from(schemas.len()).unwrap_or(u64::MAX);
    ListResponse::new(schemas, total, 1)
}

pub fn schema(urn: &str) -> Option<Value> {
    if urn.eq_ignore_ascii_case(urn::USER) {
        Some(user_schema())
    } else if urn.eq_ignore_ascii_case(urn::GROUP) {
        Some(group_schema())
    } else {
        None
    }
}

fn trimmed(base_url: &str) -> &str {
    base_url.trim_end_matches('/')
}

fn user_type(base_url: &str) -> Value {
    resource(
        base_url,
        USER_RESOURCE,
        "/Users",
        "A member of the Zone organization.",
        urn::USER,
    )
}

fn group_type(base_url: &str) -> Value {
    resource(
        base_url,
        GROUP_RESOURCE,
        "/Groups",
        "A group of organization members that can be mapped to an organization role.",
        urn::GROUP,
    )
}

fn resource(base_url: &str, name: &str, endpoint: &str, description: &str, schema: &str) -> Value {
    json!({
        "schemas": [urn::RESOURCE_TYPE],
        "id": name,
        "name": name,
        "endpoint": endpoint,
        "description": description,
        "schema": schema,
        "meta": {
            "resourceType": RESOURCE_TYPE_RESOURCE,
            "location": format!("{}/ResourceTypes/{name}", trimmed(base_url)),
        },
    })
}

fn user_schema() -> Value {
    json!({
        "schemas": [urn::SCHEMA],
        "id": urn::USER,
        "name": USER_RESOURCE,
        "description": "A member of the Zone organization.",
        "attributes": [
            string("userName", "The member's unique sign-in identifier, usually their email address.", true, UNIQUE),
            complex("name", "The member's name.", false, READ_WRITE, vec![
                string("givenName", "The member's given name.", false, NOT_UNIQUE),
                string("familyName", "The member's family name.", false, NOT_UNIQUE),
                string("formatted", "The member's full name, accepted but not stored.", false, NOT_UNIQUE),
            ]),
            string("displayName", "The name shown for the member in Zone.", false, NOT_UNIQUE),
            complex("emails", "The member's email address. Zone keeps the primary work address.", true, READ_WRITE, vec![
                string("value", "The email address.", false, NOT_UNIQUE),
                string("type", "The kind of address, such as work.", false, NOT_UNIQUE),
                boolean("primary", "Whether this is the primary address."),
            ]),
            boolean("active", "Whether the member can use the organization. False suspends the membership."),
            complex("groups", "The groups the member belongs to.", true, READ_ONLY, vec![
                string("value", "The group's id.", false, NOT_UNIQUE),
                reference("$ref", "The group's URI.", "Group"),
                string("display", "The group's display name.", false, NOT_UNIQUE),
            ]),
        ],
        "meta": { "resourceType": SCHEMA_RESOURCE },
    })
}

fn group_schema() -> Value {
    json!({
        "schemas": [urn::SCHEMA],
        "id": urn::GROUP,
        "name": GROUP_RESOURCE,
        "description": "A group of organization members that can be mapped to an organization role.",
        "attributes": [
            string("displayName", "The group's name, unique in the organization.", true, UNIQUE),
            complex("members", "The group's members.", true, READ_WRITE, vec![
                string("value", "The member's id.", false, NOT_UNIQUE),
                reference("$ref", "The member's URI.", "User"),
                string("display", "The member's display name.", false, NOT_UNIQUE),
            ]),
        ],
        "meta": { "resourceType": SCHEMA_RESOURCE },
    })
}

fn string(name: &str, description: &str, required: bool, uniqueness: &str) -> Value {
    json!({
        "name": name,
        "type": "string",
        "multiValued": false,
        "description": description,
        "required": required,
        "caseExact": false,
        "mutability": READ_WRITE,
        "returned": "default",
        "uniqueness": uniqueness,
    })
}

fn boolean(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "type": "boolean",
        "multiValued": false,
        "description": description,
        "required": false,
        "mutability": READ_WRITE,
        "returned": "default",
    })
}

fn reference(name: &str, description: &str, resource: &str) -> Value {
    json!({
        "name": name,
        "type": "reference",
        "referenceTypes": [resource],
        "multiValued": false,
        "description": description,
        "required": false,
        "caseExact": true,
        "mutability": READ_ONLY,
        "returned": "default",
        "uniqueness": NOT_UNIQUE,
    })
}

fn complex(
    name: &str,
    description: &str,
    multi_valued: bool,
    mutability: &str,
    sub_attributes: Vec<Value>,
) -> Value {
    json!({
        "name": name,
        "type": "complex",
        "multiValued": multi_valued,
        "description": description,
        "required": false,
        "mutability": mutability,
        "returned": "default",
        "subAttributes": sub_attributes,
    })
}
