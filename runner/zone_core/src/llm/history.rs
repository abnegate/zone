//! Explicit persistence representation for canonical CLI history. The provider
//! Message serializer expands images into content parts and cannot round-trip.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{GeneratedImage, ImageUrl, Message, Role, ToolCall};

#[derive(Serialize, Deserialize)]
struct Record {
    role: Role,
    content: Option<String>,
    name: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
    tool_call_id: Option<String>,
    #[serde(default)]
    images: Vec<String>,
    #[serde(default)]
    generated_images: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    thinking_blocks: Vec<serde_json::Value>,
}

pub fn serialize<S: Serializer>(messages: &[Message], serializer: S) -> Result<S::Ok, S::Error> {
    messages
        .iter()
        .map(|message| Record {
            role: message.role,
            content: message.content.clone(),
            name: message.name.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
            images: message.images.clone(),
            generated_images: message
                .generated_images
                .iter()
                .map(|image| image.image_url.url.clone())
                .collect(),
            reasoning_content: message.reasoning_content.clone(),
            thinking_blocks: message.thinking_blocks.clone(),
        })
        .collect::<Vec<_>>()
        .serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Message>, D::Error> {
    Ok(Vec::<Record>::deserialize(deserializer)?
        .into_iter()
        .map(|record| Message {
            role: record.role,
            content: record.content,
            name: record.name,
            tool_calls: record.tool_calls,
            tool_call_id: record.tool_call_id,
            images: record.images,
            generated_images: record
                .generated_images
                .into_iter()
                .map(|url| GeneratedImage {
                    image_url: ImageUrl { url },
                })
                .collect(),
            reasoning_content: record.reasoning_content,
            thinking_blocks: record.thinking_blocks,
        })
        .collect())
}
