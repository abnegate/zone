use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct TagDetail {
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_id: Option<u64>,
}
