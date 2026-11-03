use abnegate_comfy::lora::TrainError;
use serde_json::Value;

pub const MESSAGES: &str = "messages";
pub const PROMPT: &str = "prompt";
pub const COMPLETION: &str = "completion";
pub const TEXT: &str = "text";
pub const MIXED: &str = "training documents must be one format";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Chat,
    Completions,
    Text,
}

impl Format {
    pub fn of(record: &Value) -> Option<Self> {
        let object = record.as_object()?;
        if object.contains_key(MESSAGES) {
            Some(Self::Chat)
        } else if object.contains_key(PROMPT) && object.contains_key(COMPLETION) {
            Some(Self::Completions)
        } else if object.contains_key(TEXT) {
            Some(Self::Text)
        } else {
            None
        }
    }

    pub fn uniform(formats: impl IntoIterator<Item = Self>) -> Result<Option<Self>, TrainError> {
        let mut found = None;
        for format in formats {
            match found {
                None => found = Some(format),
                Some(expected) if expected != format => return Err(TrainError::Invalid(MIXED)),
                Some(_) => {}
            }
        }
        Ok(found)
    }

    pub fn masks_prompt(self) -> bool {
        self != Self::Text
    }
}
