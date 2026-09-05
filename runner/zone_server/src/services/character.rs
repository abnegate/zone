//! Character cards for models that expect a persona.
//!
//! Zone chats default to a workspace-assistant prompt. Some custom models
//! expect a character card instead. This module accepts those cards (V1, V2,
//! V3 JSON, PNG `chara`/`ccv3` text chunks, or a plain system prompt) and
//! turns them into the system message those models need.

use base64::Engine;
use serde::{Deserialize, Serialize};

const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_CHUNK: usize = 2 * 1024 * 1024;

/// Persona stored on a chat and sent to the model as its system prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ChatCharacter {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_mes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mes_example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_history_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
}

impl ChatCharacter {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() {
            return Err("Character name cannot be blank");
        }
        Ok(())
    }

    /// Build the system prompt a card-trained model expects.
    pub fn system_prompt(&self) -> String {
        let name = self.name.trim();
        let replace = |value: &str| interpolate(value, name);
        let mut sections = Vec::new();

        if let Some(system) = nonempty(&self.system_prompt) {
            sections.push(replace(system));
        }
        if let Some(description) = nonempty(&self.description) {
            sections.push(replace(description));
        }
        if let Some(personality) = nonempty(&self.personality) {
            sections.push(format!("Personality:\n{}", replace(personality)));
        }
        if let Some(scenario) = nonempty(&self.scenario) {
            sections.push(format!("Scenario:\n{}", replace(scenario)));
        }
        if let Some(example) = nonempty(&self.mes_example) {
            sections.push(format!("Example dialogue:\n{}", replace(example)));
        }
        if let Some(after) = nonempty(&self.post_history_instructions) {
            sections.push(replace(after));
        }
        if sections.is_empty() {
            sections.push(format!(
                "You are {name}. Stay in character. Write {name}'s next reply."
            ));
        } else if self.system_prompt.is_none() {
            sections.push(format!(
                "Stay in character as {name}. Write {name}'s next reply."
            ));
        }

        sections.join("\n\n")
    }
}

/// Parse a dropped card, PNG, JSON object, or plain system prompt.
pub fn parse_character(input: &[u8], source_name: Option<&str>) -> Result<ChatCharacter, String> {
    if input.starts_with(PNG_SIGNATURE) {
        let encoded = png_text_chunk(input, &["chara", "ccv3"])
            .ok_or_else(|| "PNG has no character card metadata".to_string())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|_| "PNG character chunk is not valid base64".to_string())?;
        return parse_character_json(&decoded, source_name);
    }

    let text = std::str::from_utf8(input)
        .map_err(|_| "Character card must be JSON, a PNG card, or plain text".to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Character card is empty".to_string());
    }
    if trimmed.starts_with('{') {
        return parse_character_json(trimmed.as_bytes(), source_name);
    }

    Ok(ChatCharacter {
        name: "Character".to_string(),
        system_prompt: Some(trimmed.to_string()),
        source_name: source_name.map(str::to_string),
        ..ChatCharacter::default()
    })
}

fn parse_character_json(bytes: &[u8], source_name: Option<&str>) -> Result<ChatCharacter, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Character card JSON is invalid".to_string())?;
    let mut card = character_from_value(&value)?;
    if card.source_name.is_none() {
        card.source_name = source_name.map(str::to_string);
    }
    card.validate().map_err(str::to_string)?;
    Ok(card)
}

fn character_from_value(value: &serde_json::Value) -> Result<ChatCharacter, String> {
    if let Ok(card) = serde_json::from_value::<ChatCharacter>(value.clone())
        && !card.name.trim().is_empty()
    {
        return Ok(card);
    }

    let spec = value
        .get("spec")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let data = if spec.contains("chara_card") {
        value.get("data").unwrap_or(value)
    } else {
        value
    };

    let name = data
        .get("name")
        .or_else(|| value.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Character card is missing a name".to_string())?;

    Ok(ChatCharacter {
        name: name.to_string(),
        description: string_field(data, "description"),
        personality: string_field(data, "personality"),
        scenario: string_field(data, "scenario"),
        first_mes: string_field(data, "first_mes"),
        mes_example: string_field(data, "mes_example"),
        system_prompt: string_field(data, "system_prompt"),
        post_history_instructions: string_field(data, "post_history_instructions"),
        stop_sequences: stop_field(data),
        source_name: None,
    })
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn stop_field(value: &serde_json::Value) -> Vec<String> {
    let raw = value
        .get("stop_sequences")
        .or_else(|| value.pointer("/extensions/stop_sequences"));
    match raw {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        Some(serde_json::Value::String(item)) => {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn interpolate(value: &str, name: &str) -> String {
    value
        .replace("{{char}}", name)
        .replace("{{Char}}", name)
        .replace("{{user}}", "User")
        .replace("{{User}}", "User")
}

fn png_text_chunk(bytes: &[u8], keywords: &[&str]) -> Option<String> {
    if bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }
    let mut offset = PNG_SIGNATURE.len();
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        if length > MAX_CHUNK {
            return None;
        }
        let kind = &bytes[offset + 4..offset + 8];
        let data_start = offset + 8;
        let data_end = data_start.checked_add(length)?;
        if data_end + 4 > bytes.len() {
            return None;
        }
        let data = &bytes[data_start..data_end];
        if kind == b"tEXt" || kind == b"iTXt" {
            if let Some(text) = png_keyword_text(data, keywords, kind == b"iTXt") {
                return Some(text);
            }
        }
        if kind == b"IEND" {
            break;
        }
        offset = data_end + 4;
    }
    None
}

fn png_keyword_text(data: &[u8], keywords: &[&str], itxt: bool) -> Option<String> {
    let split = data.iter().position(|byte| *byte == 0)?;
    let keyword = std::str::from_utf8(&data[..split]).ok()?;
    if !keywords
        .iter()
        .any(|wanted| wanted.eq_ignore_ascii_case(keyword))
    {
        return None;
    }
    let rest = &data[split + 1..];
    let text = if itxt {
        // keyword, compression flag, method, language, translated keyword, text
        let mut cursor = rest;
        if cursor.is_empty() {
            return None;
        }
        let compressed = cursor[0] != 0;
        if compressed {
            return None;
        }
        cursor = cursor.get(2..)?;
        let lang_end = cursor.iter().position(|byte| *byte == 0)?;
        cursor = cursor.get(lang_end + 1..)?;
        let translated_end = cursor.iter().position(|byte| *byte == 0)?;
        cursor = cursor.get(translated_end + 1..)?;
        std::str::from_utf8(cursor).ok()?
    } else {
        std::str::from_utf8(rest).ok()?
    };
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_with_text(keyword: &str, text: &str) -> Vec<u8> {
        let mut data = Vec::from(keyword.as_bytes());
        data.push(0);
        data.extend_from_slice(text.as_bytes());
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(&data);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png
    }

    #[test]
    fn parses_v2_card_and_builds_persona_prompt() {
        let json = serde_json::json!({
            "spec": "chara_card_v2",
            "data": {
                "name": "Noromaid",
                "description": "{{char}} is a tavern keeper.",
                "personality": "Warm, direct",
                "scenario": "A quiet inn",
                "first_mes": "Welcome in.",
                "system_prompt": "Write in-character replies as {{char}}.",
                "extensions": { "stop_sequences": ["END"] }
            }
        });
        let card = parse_character(json.to_string().as_bytes(), Some("noromaid.json")).unwrap();
        assert_eq!(card.name, "Noromaid");
        assert_eq!(card.first_mes.as_deref(), Some("Welcome in."));
        assert_eq!(card.stop_sequences, vec!["END"]);
        let prompt = card.system_prompt();
        assert!(prompt.contains("Write in-character replies as Noromaid."));
        assert!(prompt.contains("Noromaid is a tavern keeper."));
        assert!(prompt.contains("Personality:\nWarm, direct"));
        assert!(!prompt.contains("{{char}}"));
    }

    #[test]
    fn parses_plain_system_prompt() {
        let card = parse_character(b"You are a tired ship's cook.", None).unwrap();
        assert_eq!(card.name, "Character");
        assert_eq!(card.system_prompt(), "You are a tired ship's cook.");
    }

    #[test]
    fn parses_png_chara_chunk() {
        let payload = serde_json::json!({
            "spec": "chara_card_v2",
            "data": { "name": "Pixel", "description": "A drawn figure" }
        });
        let encoded = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
        let png = png_with_text("chara", &encoded);
        let card = parse_character(&png, Some("pixel.png")).unwrap();
        assert_eq!(card.name, "Pixel");
        assert_eq!(card.source_name.as_deref(), Some("pixel.png"));
    }

    #[test]
    fn rejects_nameless_json() {
        let err = parse_character(br#"{"description":"nobody"}"#, None).unwrap_err();
        assert!(err.contains("name"));
    }
}
