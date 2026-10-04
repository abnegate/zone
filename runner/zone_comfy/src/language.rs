//! Convert document dumps into MLX-LM jsonl and `data/format.json`.

use crate::lora::{TrainError, TrainImage};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const CHUNK_CHARS: usize = 2048;
const CHUNK_OVERLAP: usize = 200;
const VALID_FRACTION: f32 = 0.1;
const VALID_MIN: usize = 10;
const MIXED_FORMAT: &str = "training documents must be one format";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExampleKind {
    Chat,
    Completions,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Example {
    kind: ExampleKind,
    record: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    pub train: Vec<Value>,
    pub valid: Vec<Value>,
    pub mask_prompt: bool,
}

impl Dataset {
    pub fn len(&self) -> usize {
        self.train.len() + self.valid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.train.is_empty() && self.valid.is_empty()
    }
}

pub fn to_jsonl(documents: &[TrainImage]) -> Result<Dataset, TrainError> {
    let mut examples = Vec::new();
    for document in documents {
        examples.extend(examples_from(document)?);
    }
    let kind = one_kind(examples.iter().map(|example| example.kind))?
        .ok_or(TrainError::Invalid("training needs documents"))?;
    let mask_prompt = kind != ExampleKind::Text;
    let (train, valid) = split(examples);
    Ok(Dataset {
        train: train.into_iter().map(|example| example.record).collect(),
        valid: valid.into_iter().map(|example| example.record).collect(),
        mask_prompt,
    })
}

pub fn write(directory: &Path, dataset: &Dataset) -> Result<(), TrainError> {
    one_kind(
        dataset
            .train
            .iter()
            .chain(&dataset.valid)
            .filter_map(record_kind),
    )?;
    fs::create_dir_all(directory).map_err(|error| TrainError::Failed(error.to_string()))?;
    write_jsonl(&directory.join("train.jsonl"), &dataset.train)?;
    if !dataset.valid.is_empty() {
        write_jsonl(&directory.join("valid.jsonl"), &dataset.valid)?;
    }
    let encoded = serde_json::to_vec(&json!({ "mask_prompt": dataset.mask_prompt }))
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    fs::write(directory.join("format.json"), encoded)
        .map_err(|error| TrainError::Failed(error.to_string()))
}

pub(crate) fn document_bytes(document: &TrainImage) -> Result<Vec<u8>, TrainError> {
    if let Some(bytes) = &document.bytes {
        return Ok(bytes.clone());
    }
    if document.bytes_base64.trim().is_empty() {
        return Ok(Vec::new());
    }
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(document.bytes_base64.trim())
        .map_err(|_| TrainError::Invalid("document is not valid base64"))
}

fn examples_from(document: &TrainImage) -> Result<Vec<Example>, TrainError> {
    let text = utf8(document)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let name = document.filename.to_ascii_lowercase();
    if name.ends_with(".jsonl") {
        return parse_jsonl(&text);
    }
    if name.ends_with(".json") {
        return parse_json(&text);
    }
    if name.ends_with(".txt") || name.ends_with(".md") || name.ends_with(".markdown") {
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

fn utf8(document: &TrainImage) -> Result<String, TrainError> {
    String::from_utf8(document_bytes(document)?)
        .map_err(|_| TrainError::Invalid("document is not valid UTF-8"))
}

fn parse_json(text: &str) -> Result<Vec<Example>, TrainError> {
    let value: Value = serde_json::from_str(text.trim())
        .map_err(|_| TrainError::Invalid("document is not valid JSON"))?;
    match value {
        Value::Array(items) => Ok(items.into_iter().filter_map(example_from_value).collect()),
        other => Ok(example_from_value(other).into_iter().collect()),
    }
}

fn parse_jsonl(text: &str) -> Result<Vec<Example>, TrainError> {
    let mut examples = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| TrainError::Invalid("document is not valid JSON"))?;
        if let Some(example) = example_from_value(value) {
            examples.push(example);
        }
    }
    Ok(examples)
}

fn example_from_value(value: Value) -> Option<Example> {
    match value {
        Value::String(text) => text_example(&text),
        Value::Object(map) => {
            if let Some(messages) = map.get("messages").and_then(Value::as_array)
                && !messages.is_empty()
            {
                return Some(Example {
                    kind: ExampleKind::Chat,
                    record: json!({ "messages": messages }),
                });
            }
            if let (Some(prompt), Some(completion)) = (
                map.get("prompt").and_then(Value::as_str),
                map.get("completion").and_then(Value::as_str),
            ) {
                if completion.is_empty() {
                    return None;
                }
                return Some(Example {
                    kind: ExampleKind::Completions,
                    record: json!({ "prompt": prompt, "completion": completion }),
                });
            }
            let instruction = map.get("instruction").and_then(Value::as_str);
            let output = map.get("output").and_then(Value::as_str);
            if let (Some(instruction), Some(output)) = (instruction, output)
                && !output.is_empty()
            {
                let input = map.get("input").and_then(Value::as_str).unwrap_or("");
                let prompt = if input.trim().is_empty() {
                    instruction.to_string()
                } else {
                    format!("{instruction}\n\n{input}")
                };
                return Some(Example {
                    kind: ExampleKind::Completions,
                    record: json!({ "prompt": prompt, "completion": output }),
                });
            }
            map.get("text")
                .and_then(Value::as_str)
                .and_then(text_example)
        }
        _ => None,
    }
}

fn text_example(text: &str) -> Option<Example> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(Example {
        kind: ExampleKind::Text,
        record: json!({ "text": text }),
    })
}

fn text_chunks(text: &str) -> Vec<Example> {
    chunks(text, CHUNK_CHARS, CHUNK_OVERLAP)
        .into_iter()
        .filter_map(|chunk| text_example(&chunk))
        .collect()
}

fn chunks(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= size {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + size).min(chars.len());
        if end < chars.len()
            && let Some(split) = last_break(&chars[start..end], overlap)
        {
            end = start + split;
        }
        if end <= start {
            end = (start + size).min(chars.len());
        }
        out.push(chars[start..end].iter().collect());
        if end >= chars.len() {
            break;
        }
        let next = end.saturating_sub(overlap);
        start = if next <= start { end } else { next };
    }
    out
}

fn last_break(window: &[char], min: usize) -> Option<usize> {
    let min = min.max(1);
    paragraph_break(window, min).or_else(|| line_break(window, min))
}

fn paragraph_break(window: &[char], min: usize) -> Option<usize> {
    let mut index = window.len().saturating_sub(2);
    while index >= min {
        if window[index] == '\n' && window[index + 1] == '\n' {
            return Some(index);
        }
        index -= 1;
    }
    None
}

fn line_break(window: &[char], min: usize) -> Option<usize> {
    let mut index = window.len().saturating_sub(1);
    while index >= min {
        if window[index] == '\n' {
            return Some(index);
        }
        index -= 1;
    }
    None
}

fn one_kind(
    kinds: impl IntoIterator<Item = ExampleKind>,
) -> Result<Option<ExampleKind>, TrainError> {
    let mut found = None;
    for kind in kinds {
        match found {
            None => found = Some(kind),
            Some(expected) if expected != kind => {
                return Err(TrainError::Invalid(MIXED_FORMAT));
            }
            Some(_) => {}
        }
    }
    Ok(found)
}

fn record_kind(record: &Value) -> Option<ExampleKind> {
    let object = record.as_object()?;
    if object.contains_key("messages") {
        Some(ExampleKind::Chat)
    } else if object.contains_key("prompt") && object.contains_key("completion") {
        Some(ExampleKind::Completions)
    } else if object.contains_key("text") {
        Some(ExampleKind::Text)
    } else {
        None
    }
}

fn split(mut examples: Vec<Example>) -> (Vec<Example>, Vec<Example>) {
    if examples.len() < VALID_MIN {
        return (examples, Vec::new());
    }
    let valid_count = ((examples.len() as f32) * VALID_FRACTION).round() as usize;
    if valid_count == 0 {
        return (examples, Vec::new());
    }
    let split_at = examples.len() - valid_count.min(examples.len());
    let valid = examples.split_off(split_at);
    (examples, valid)
}

fn write_jsonl(path: &Path, records: &[Value]) -> Result<(), TrainError> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record)
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        bytes.push(b'\n');
    }
    fs::write(path, bytes).map_err(|error| TrainError::Failed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lora::TrainImage;

    fn document(filename: &str, body: &str) -> TrainImage {
        TrainImage {
            filename: filename.into(),
            caption: String::new(),
            bytes_base64: String::new(),
            bytes: Some(body.as_bytes().to_vec()),
            before_base64: None,
            before: None,
            group: None,
        }
    }

    fn records(documents: &[TrainImage]) -> Vec<Value> {
        let dataset = to_jsonl(documents).unwrap();
        let mut records = dataset.train;
        records.extend(dataset.valid);
        records
    }

    #[test]
    fn chat_messages_stay_chat() {
        let records = records(&[document(
            "chat.jsonl",
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
        )]);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]["messages"][0]["content"],
            Value::String("hi".into())
        );
        assert!(records[0].get("text").is_none());
    }

    #[test]
    fn prompt_completion_stays_completions() {
        let records = records(&[document(
            "pairs.jsonl",
            r#"{"prompt":"Q","completion":"A"}"#,
        )]);
        assert_eq!(records[0]["prompt"], "Q");
        assert_eq!(records[0]["completion"], "A");
    }

    #[test]
    fn alpaca_becomes_completions() {
        let records = records(&[document(
            "alpaca.json",
            r#"{"instruction":"Translate","input":"hola","output":"hello"}"#,
        )]);
        assert_eq!(records[0]["prompt"], "Translate\n\nhola");
        assert_eq!(records[0]["completion"], "hello");
    }

    #[test]
    fn txt_chunks_are_text_records() {
        let records = records(&[document("notes.txt", "a short dump")]);
        assert_eq!(records, vec![json!({ "text": "a short dump" })]);
    }

    #[test]
    fn json_array_accepts_mixed_shapes() {
        assert!(matches!(
            to_jsonl(&[document(
                "dump.json",
                r#"[{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]},{"prompt":"Q","completion":"A"},{"text":"loose"}]"#,
            )]),
            Err(TrainError::Invalid(MIXED_FORMAT))
        ));
    }

    #[test]
    fn mixed_chat_and_text_are_rejected() {
        assert!(matches!(
            to_jsonl(&[
                document(
                    "chat.jsonl",
                    r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
                ),
                document("notes.txt", "loose"),
            ]),
            Err(TrainError::Invalid(MIXED_FORMAT))
        ));
    }

    #[test]
    fn mixed_completions_and_chat_are_rejected() {
        assert!(matches!(
            to_jsonl(&[document(
                "dump.jsonl",
                concat!(
                    r#"{"prompt":"Q","completion":"A"}"#,
                    "\n",
                    r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
                ),
            )]),
            Err(TrainError::Invalid(MIXED_FORMAT))
        ));
    }

    #[test]
    fn write_rejects_mixed_records() {
        let root = tempfile::tempdir().unwrap();
        let mixed = Dataset {
            train: vec![
                json!({"prompt": "Q", "completion": "A"}),
                json!({"text": "loose"}),
            ],
            valid: vec![],
            mask_prompt: false,
        };
        assert!(matches!(
            write(root.path(), &mixed),
            Err(TrainError::Invalid(MIXED_FORMAT))
        ));
        assert!(!root.path().join("train.jsonl").exists());
    }

    #[test]
    fn format_json_masks_prompt_only_without_text() {
        let root = tempfile::tempdir().unwrap();
        let chat = to_jsonl(&[document(
            "chat.json",
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
        )])
        .unwrap();
        write(root.path(), &chat).unwrap();
        let format: Value =
            serde_json::from_slice(&fs::read(root.path().join("format.json")).unwrap()).unwrap();
        assert_eq!(format["mask_prompt"], true);
        assert!(root.path().join("train.jsonl").is_file());
        assert!(!root.path().join("valid.jsonl").exists());

        let completions = to_jsonl(&[document(
            "pairs.jsonl",
            r#"{"prompt":"Q","completion":"A"}"#,
        )])
        .unwrap();
        write(root.path(), &completions).unwrap();
        let format: Value =
            serde_json::from_slice(&fs::read(root.path().join("format.json")).unwrap()).unwrap();
        assert_eq!(format["mask_prompt"], true);

        let text = to_jsonl(&[document("notes.txt", "loose")]).unwrap();
        write(root.path(), &text).unwrap();
        let format: Value =
            serde_json::from_slice(&fs::read(root.path().join("format.json")).unwrap()).unwrap();
        assert_eq!(format["mask_prompt"], false);
    }

    #[test]
    fn document_bytes_rejects_invalid_base64() {
        let document = TrainImage {
            filename: "notes.txt".into(),
            caption: String::new(),
            bytes_base64: "!!!not base64!!!".into(),
            bytes: None,
            before_base64: None,
            before: None,
            group: None,
        };
        assert!(matches!(
            document_bytes(&document),
            Err(TrainError::Invalid("document is not valid base64"))
        ));
    }

    #[test]
    fn invalid_utf8_documents_are_rejected() {
        let document = TrainImage {
            filename: "notes.txt".into(),
            caption: String::new(),
            bytes_base64: String::new(),
            bytes: Some(vec![0xff, 0xfe]),
            before_base64: None,
            before: None,
            group: None,
        };
        assert!(matches!(
            to_jsonl(&[document]),
            Err(TrainError::Invalid("document is not valid UTF-8"))
        ));
    }

    #[test]
    fn txt_is_never_wrapped_as_chat() {
        let body = "paragraph one\n\n".repeat(80);
        let records = records(&[document("notes.md", &body)]);
        assert!(!records.is_empty());
        assert!(
            records
                .iter()
                .all(|record| record.get("text").is_some() && record.get("messages").is_none())
        );
    }

    #[test]
    fn ten_examples_write_a_valid_split() {
        let items: Vec<Value> = (0..10)
            .map(
                |index| json!({ "prompt": format!("q{index}"), "completion": format!("a{index}") }),
            )
            .collect();
        let dataset = to_jsonl(&[document(
            "pairs.json",
            &serde_json::to_string(&items).unwrap(),
        )])
        .unwrap();
        assert_eq!(dataset.len(), 10);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.valid.len(), 1);
        assert_eq!(dataset.train.len(), 9);
        assert!(dataset.mask_prompt);
    }

    #[test]
    fn zero_examples_are_rejected() {
        assert!(matches!(
            to_jsonl(&[document("empty.txt", "   ")]),
            Err(TrainError::Invalid("training needs documents"))
        ));
        assert!(matches!(
            to_jsonl(&[]),
            Err(TrainError::Invalid("training needs documents"))
        ));
    }
}
