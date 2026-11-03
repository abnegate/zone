use super::subject::Subject;
use abnegate_comfy::lora;
use abnegate_comfy::lora::TrainMethod;
use abnegate_comfy::lora::TrainSubject;
use serde::Serialize;

/// A base the client offers for training: an image base the crate trains,
/// or an installed chat model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TrainBase {
    pub id: String,
    pub label: String,
    pub edit: bool,
    #[serde(skip_serializing_if = "Subject::is_other")]
    pub subject: Subject,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub finetune: bool,
}

impl From<lora::TrainBase> for TrainBase {
    fn from(base: lora::TrainBase) -> Self {
        let subject = if base.subjects.contains(&TrainSubject::Person) {
            Subject::Person
        } else {
            Subject::Other
        };
        Self {
            finetune: base.methods.contains(&TrainMethod::Finetune),
            id: base.id,
            label: base.label,
            edit: base.edit,
            subject,
        }
    }
}
