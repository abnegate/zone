use super::example::Example;
use super::failed;
use super::format::Format;
use abnegate_comfy::host::FORMAT_FILE;
use abnegate_comfy::host::TRAIN_DATA_FILE;
use abnegate_comfy::host::VALID_DATA_FILE;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainImage;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

pub const EMPTY: &str = "training needs documents";
const MASK_PROMPT: &str = "mask_prompt";
const VALID_FRACTION: f64 = 0.1;
const VALID_MINIMUM: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    pub train: Vec<Value>,
    pub valid: Vec<Value>,
    pub mask_prompt: bool,
}

impl Dataset {
    pub fn from_documents(documents: &[TrainImage]) -> Result<Self, TrainError> {
        let mut examples = Vec::new();
        for document in documents {
            examples.extend(Example::from_document(document)?);
        }
        let format = Format::uniform(examples.iter().map(|example| example.format))?
            .ok_or(TrainError::Invalid(EMPTY))?;
        let valid = examples.split_off(examples.len() - valid_count(examples.len()));
        Ok(Self {
            train: examples.into_iter().map(|example| example.record).collect(),
            valid: valid.into_iter().map(|example| example.record).collect(),
            mask_prompt: format.masks_prompt(),
        })
    }

    pub fn len(&self) -> usize {
        self.train.len() + self.valid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.train.is_empty() && self.valid.is_empty()
    }

    pub fn write(&self, directory: &Path) -> Result<(), TrainError> {
        Format::uniform(self.train.iter().chain(&self.valid).filter_map(Format::of))?;
        fs::create_dir_all(directory).map_err(failed)?;
        write_jsonl(&directory.join(TRAIN_DATA_FILE), &self.train)?;
        if !self.valid.is_empty() {
            write_jsonl(&directory.join(VALID_DATA_FILE), &self.valid)?;
        }
        let format = serde_json::to_vec(&json!({ MASK_PROMPT: self.mask_prompt }))
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        fs::write(directory.join(FORMAT_FILE), format).map_err(failed)
    }
}

fn valid_count(examples: usize) -> usize {
    if examples < VALID_MINIMUM {
        return 0;
    }
    ((examples as f64 * VALID_FRACTION).round() as usize).min(examples)
}

fn write_jsonl(path: &Path, records: &[Value]) -> Result<(), TrainError> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record)
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        bytes.push(b'\n');
    }
    fs::write(path, bytes).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::super::INVALID_BASE64;
    use super::super::document_bytes;
    use super::super::example::INVALID_UTF8;
    use super::super::format::MIXED;
    use super::*;

    const CHAT: &str =
        r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#;

    fn document(filename: &str, body: &str) -> TrainImage {
        TrainImage::from_bytes(filename, "", body.as_bytes())
    }

    fn records(documents: &[TrainImage]) -> Vec<Value> {
        let dataset = Dataset::from_documents(documents).unwrap();
        let mut records = dataset.train;
        records.extend(dataset.valid);
        records
    }

    fn mask_prompt(directory: &Path) -> Value {
        let format: Value =
            serde_json::from_slice(&fs::read(directory.join(FORMAT_FILE)).unwrap()).unwrap();
        format[MASK_PROMPT].clone()
    }

    #[test]
    fn chat_messages_stay_chat() {
        let records = records(&[document("chat.jsonl", CHAT)]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["messages"][0]["content"], "hi");
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
    fn json_array_rejects_mixed_shapes() {
        assert!(matches!(
            Dataset::from_documents(&[document(
                "dump.json",
                r#"[{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]},{"prompt":"Q","completion":"A"},{"text":"loose"}]"#,
            )]),
            Err(TrainError::Invalid(MIXED))
        ));
    }

    #[test]
    fn mixed_chat_and_text_are_rejected() {
        assert!(matches!(
            Dataset::from_documents(&[
                document("chat.jsonl", CHAT),
                document("notes.txt", "loose"),
            ]),
            Err(TrainError::Invalid(MIXED))
        ));
    }

    #[test]
    fn mixed_completions_and_chat_are_rejected() {
        let body = format!("{}\n{CHAT}", r#"{"prompt":"Q","completion":"A"}"#);
        assert!(matches!(
            Dataset::from_documents(&[document("dump.jsonl", &body)]),
            Err(TrainError::Invalid(MIXED))
        ));
    }

    #[test]
    fn unnamed_documents_are_sniffed_as_json_then_jsonl_then_text() {
        let json = records(&[document("dump", r#"[{"prompt":"Q","completion":"A"}]"#)]);
        assert_eq!(json, vec![json!({ "prompt": "Q", "completion": "A" })]);
        let jsonl = records(&[document("dump", "\"one\"\n\"two\"")]);
        assert_eq!(
            jsonl,
            vec![json!({ "text": "one" }), json!({ "text": "two" })]
        );
        let text = records(&[document("dump", "{ not json")]);
        assert_eq!(text, vec![json!({ "text": "{ not json" })]);
    }

    #[test]
    fn write_rejects_mixed_records() {
        let root = tempfile::tempdir().unwrap();
        let mixed = Dataset {
            train: vec![
                json!({ "prompt": "Q", "completion": "A" }),
                json!({ "text": "loose" }),
            ],
            valid: Vec::new(),
            mask_prompt: false,
        };
        assert!(matches!(
            mixed.write(root.path()),
            Err(TrainError::Invalid(MIXED))
        ));
        assert!(!root.path().join(TRAIN_DATA_FILE).exists());
    }

    #[test]
    fn format_json_masks_prompt_only_without_text() {
        let root = tempfile::tempdir().unwrap();
        let chat = Dataset::from_documents(&[document("chat.json", CHAT)]).unwrap();
        chat.write(root.path()).unwrap();
        assert_eq!(mask_prompt(root.path()), true);
        assert!(root.path().join(TRAIN_DATA_FILE).is_file());
        assert!(!root.path().join(VALID_DATA_FILE).exists());

        let completions = Dataset::from_documents(&[document(
            "pairs.jsonl",
            r#"{"prompt":"Q","completion":"A"}"#,
        )])
        .unwrap();
        completions.write(root.path()).unwrap();
        assert_eq!(mask_prompt(root.path()), true);

        let text = Dataset::from_documents(&[document("notes.txt", "loose")]).unwrap();
        text.write(root.path()).unwrap();
        assert_eq!(mask_prompt(root.path()), false);
    }

    #[test]
    fn document_bytes_rejects_invalid_base64() {
        let document = TrainImage::new("notes.txt", "", "!!!not base64!!!");
        assert!(matches!(
            document_bytes(&document),
            Err(TrainError::Invalid(INVALID_BASE64))
        ));
    }

    #[test]
    fn document_bytes_reads_base64_and_treats_blank_as_empty() {
        let encoded = TrainImage::new("notes.txt", "", " aGk= ");
        assert_eq!(document_bytes(&encoded).unwrap(), b"hi");
        let blank = TrainImage::new("notes.txt", "", "  ");
        assert!(document_bytes(&blank).unwrap().is_empty());
    }

    #[test]
    fn invalid_utf8_documents_are_rejected() {
        let document = TrainImage::from_bytes("notes.txt", "", vec![0xff, 0xfe]);
        assert!(matches!(
            Dataset::from_documents(&[document]),
            Err(TrainError::Invalid(INVALID_UTF8))
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
        let dataset = Dataset::from_documents(&[document(
            "pairs.json",
            &serde_json::to_string(&items).unwrap(),
        )])
        .unwrap();
        assert_eq!(dataset.len(), 10);
        assert!(!dataset.is_empty());
        assert_eq!(dataset.valid.len(), 1);
        assert_eq!(dataset.train.len(), 9);
        assert_eq!(dataset.valid[0]["prompt"], "q9");
        assert!(dataset.mask_prompt);

        let root = tempfile::tempdir().unwrap();
        dataset.write(root.path()).unwrap();
        let valid = fs::read_to_string(root.path().join(VALID_DATA_FILE)).unwrap();
        assert!(valid.ends_with('\n'));
        let record: Value = serde_json::from_str(valid.trim_end()).unwrap();
        assert_eq!(record, json!({ "prompt": "q9", "completion": "a9" }));
        let train = fs::read_to_string(root.path().join(TRAIN_DATA_FILE)).unwrap();
        assert_eq!(train.lines().count(), 9);
    }

    #[test]
    fn zero_examples_are_rejected() {
        assert!(matches!(
            Dataset::from_documents(&[document("empty.txt", "   ")]),
            Err(TrainError::Invalid(EMPTY))
        ));
        assert!(matches!(
            Dataset::from_documents(&[]),
            Err(TrainError::Invalid(EMPTY))
        ));
    }
}
