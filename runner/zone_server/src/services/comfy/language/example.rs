use super::document_bytes;
use super::format::COMPLETION;
use super::format::Format;
use super::format::MESSAGES;
use super::format::PROMPT;
use super::format::TEXT;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainImage;
use serde_json::Value;
use serde_json::json;

const CHUNK_CHARACTERS: usize = 2048;
const CHUNK_OVERLAP: usize = 200;
const INSTRUCTION: &str = "instruction";
const INPUT: &str = "input";
const OUTPUT: &str = "output";
const JSONL_EXTENSION: &str = ".jsonl";
const JSON_EXTENSION: &str = ".json";
const TEXT_EXTENSIONS: &[&str] = &[".txt", ".md", ".markdown"];
const INVALID_JSON: &str = "document is not valid JSON";
pub const INVALID_UTF8: &str = "document is not valid UTF-8";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub format: Format,
    pub record: Value,
}

impl Example {
    pub fn from_document(document: &TrainImage) -> Result<Vec<Self>, TrainError> {
        let text = String::from_utf8(document_bytes(document)?)
            .map_err(|_| TrainError::Invalid(INVALID_UTF8))?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let name = document.filename.to_ascii_lowercase();
        if name.ends_with(JSONL_EXTENSION) {
            return parse_jsonl(&text);
        }
        if name.ends_with(JSON_EXTENSION) {
            return parse_json(&text);
        }
        if TEXT_EXTENSIONS
            .iter()
            .any(|extension| name.ends_with(extension))
        {
            return Ok(text_chunks(&text));
        }
        if let Ok(examples) = parse_json(&text)
            && !examples.is_empty()
        {
            return Ok(examples);
        }
        if let Ok(examples) = parse_jsonl(&text)
            && !examples.is_empty()
        {
            return Ok(examples);
        }
        Ok(text_chunks(&text))
    }

    pub fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::String(text) => Self::text(&text),
            Value::Object(map) => {
                if let Some(messages) = map.get(MESSAGES).and_then(Value::as_array)
                    && !messages.is_empty()
                {
                    return Some(Self {
                        format: Format::Chat,
                        record: json!({ MESSAGES: messages }),
                    });
                }
                if let (Some(prompt), Some(completion)) = (
                    map.get(PROMPT).and_then(Value::as_str),
                    map.get(COMPLETION).and_then(Value::as_str),
                ) {
                    return (!completion.is_empty()).then(|| Self::completion(prompt, completion));
                }
                let instruction = map.get(INSTRUCTION).and_then(Value::as_str);
                let output = map.get(OUTPUT).and_then(Value::as_str);
                if let (Some(instruction), Some(output)) = (instruction, output)
                    && !output.is_empty()
                {
                    let input = map.get(INPUT).and_then(Value::as_str).unwrap_or_default();
                    let prompt = if input.trim().is_empty() {
                        instruction.to_string()
                    } else {
                        format!("{instruction}\n\n{input}")
                    };
                    return Some(Self::completion(&prompt, output));
                }
                map.get(TEXT).and_then(Value::as_str).and_then(Self::text)
            }
            _ => None,
        }
    }

    pub fn text(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(Self {
            format: Format::Text,
            record: json!({ TEXT: text }),
        })
    }

    fn completion(prompt: &str, completion: &str) -> Self {
        Self {
            format: Format::Completions,
            record: json!({ PROMPT: prompt, COMPLETION: completion }),
        }
    }
}

fn parse_json(text: &str) -> Result<Vec<Example>, TrainError> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|_| TrainError::Invalid(INVALID_JSON))?;
    match value {
        Value::Array(items) => Ok(items.into_iter().filter_map(Example::from_value).collect()),
        other => Ok(Example::from_value(other).into_iter().collect()),
    }
}

fn parse_jsonl(text: &str) -> Result<Vec<Example>, TrainError> {
    let mut examples = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|_| TrainError::Invalid(INVALID_JSON))?;
        examples.extend(Example::from_value(value));
    }
    Ok(examples)
}

fn text_chunks(text: &str) -> Vec<Example> {
    chunks(text, CHUNK_CHARACTERS, CHUNK_OVERLAP)
        .iter()
        .filter_map(|chunk| Example::text(chunk))
        .collect()
}

fn chunks(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let characters: Vec<char> = text.chars().collect();
    if characters.len() <= size {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let mut end = (start + size).min(characters.len());
        if end < characters.len()
            && let Some(split) = last_break(&characters[start..end], overlap)
        {
            end = start + split;
        }
        if end <= start {
            end = (start + size).min(characters.len());
        }
        chunks.push(characters[start..end].iter().collect());
        if end >= characters.len() {
            break;
        }
        let next = end.saturating_sub(overlap);
        start = if next <= start { end } else { next };
    }
    chunks
}

fn last_break(window: &[char], minimum: usize) -> Option<usize> {
    let minimum = minimum.max(1);
    paragraph_break(window, minimum).or_else(|| line_break(window, minimum))
}

fn paragraph_break(window: &[char], minimum: usize) -> Option<usize> {
    let mut index = window.len().saturating_sub(2);
    while index >= minimum {
        if window[index] == '\n' && window[index + 1] == '\n' {
            return Some(index);
        }
        index -= 1;
    }
    None
}

fn line_break(window: &[char], minimum: usize) -> Option<usize> {
    let mut index = window.len().saturating_sub(1);
    while index >= minimum {
        if window[index] == '\n' {
            return Some(index);
        }
        index -= 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_splits_on_paragraphs_with_overlap() {
        let paragraph = format!("{}\n\n", "a".repeat(1000));
        let pieces = chunks(&paragraph.repeat(3), CHUNK_CHARACTERS, CHUNK_OVERLAP);
        assert_eq!(pieces.len(), 2);
        assert!(
            pieces
                .iter()
                .all(|piece| piece.chars().count() <= CHUNK_CHARACTERS)
        );
        assert!(
            pieces[0].ends_with('a'),
            "a chunk ends on a paragraph break"
        );
    }

    #[test]
    fn text_without_breaks_splits_at_the_chunk_size() {
        let pieces = chunks(&"b".repeat(5000), CHUNK_CHARACTERS, CHUNK_OVERLAP);
        assert_eq!(pieces[0].chars().count(), CHUNK_CHARACTERS);
        assert_eq!(
            pieces
                .iter()
                .map(|piece| piece.chars().count())
                .sum::<usize>(),
            5000 + CHUNK_OVERLAP * (pieces.len() - 1)
        );
    }

    #[test]
    fn empty_completions_and_messages_are_skipped() {
        assert!(Example::from_value(json!({ PROMPT: "Q", COMPLETION: "" })).is_none());
        assert!(Example::from_value(json!({ MESSAGES: [] })).is_none());
        assert!(Example::from_value(json!({ INSTRUCTION: "do", OUTPUT: "" })).is_none());
        assert!(Example::from_value(json!(42)).is_none());
    }

    #[test]
    fn alpaca_without_input_prompts_with_the_instruction_alone() {
        let example =
            Example::from_value(json!({ INSTRUCTION: "Greet", INPUT: " ", OUTPUT: "hi" })).unwrap();
        assert_eq!(example.format, Format::Completions);
        assert_eq!(example.record, json!({ PROMPT: "Greet", COMPLETION: "hi" }));
    }
}
