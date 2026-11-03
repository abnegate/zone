use super::subject::Subject;
use crate::services::comfy::language;
use abnegate_comfy::host::Compute;
use abnegate_comfy::lora::TrainImage;
use abnegate_comfy::lora::TrainMethod;
use abnegate_comfy::lora::TrainRequest;
use abnegate_comfy::lora::TrainSubject;
use serde::Deserialize;

/// A training run as the client and the chat tool ask for it, before it is
/// split into an image request for the crate or a chat request for zone.
#[derive(Debug, Deserialize)]
pub struct TrainForm {
    pub name: String,
    pub base: String,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub subject: Subject,
    #[serde(default)]
    pub method: TrainMethod,
    #[serde(default)]
    pub images: Vec<TrainImage>,
    #[serde(default, rename = "provider")]
    pub compute: Compute,
}

impl TrainForm {
    pub fn image(self, subject: TrainSubject) -> TrainRequest {
        let request = TrainRequest::new(self.name, self.base, self.images)
            .with_subject(subject)
            .with_method(self.method);
        match self.trigger {
            Some(trigger) => request.with_trigger(trigger),
            None => request,
        }
    }

    pub fn language(self) -> language::Request {
        language::Request::new(self.name, self.base, self.images).with_method(self.method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(body: serde_json::Value) -> TrainForm {
        serde_json::from_value(body).expect("a train form")
    }

    #[test]
    fn a_bare_form_trains_a_local_lora_of_anything() {
        let form = form(serde_json::json!({"name": "jerry", "base": "flux-schnell"}));
        assert_eq!(form.subject, Subject::Other);
        assert_eq!(form.method, TrainMethod::Lora);
        assert_eq!(form.compute, Compute::Local);
        assert!(form.images.is_empty());
        let request = form.image(TrainSubject::Other);
        assert_eq!(request.trigger, None);
        assert_eq!(request.compute, Compute::Local);
    }

    #[test]
    fn the_wire_names_reach_the_request() {
        let form = form(serde_json::json!({
            "name": "jerry",
            "base": "sdxl-people",
            "trigger": "ohwx",
            "subject": "person",
            "method": "finetune",
            "provider": "runpod",
            "images": [{"filename": "one.png", "caption": "", "bytes_base64": "AA=="}],
        }));
        assert_eq!(form.subject, Subject::Person);
        assert_eq!(form.compute, Compute::Runpod);
        let request = form.image(TrainSubject::Person);
        assert_eq!(request.name, "jerry");
        assert_eq!(request.base, "sdxl-people");
        assert_eq!(request.trigger.as_deref(), Some("ohwx"));
        assert_eq!(request.subject, TrainSubject::Person);
        assert_eq!(request.method, TrainMethod::Finetune);
        assert_eq!(request.images.len(), 1);
    }

    #[test]
    fn a_language_form_keeps_its_documents_and_method() {
        let form = form(serde_json::json!({
            "name": "support-bot",
            "base": "llama3.2:1b",
            "subject": "language",
            "method": "finetune",
            "images": [{"filename": "faq.jsonl", "caption": ""}],
        }));
        assert_eq!(form.subject, Subject::Language);
        let request = form.language();
        assert_eq!(request.name, "support-bot");
        assert_eq!(request.base, "llama3.2:1b");
        assert_eq!(request.method, TrainMethod::Finetune);
        assert_eq!(request.documents.len(), 1);
    }

    #[test]
    fn an_unknown_subject_is_refused() {
        let refused = serde_json::from_value::<TrainForm>(serde_json::json!({
            "name": "jerry",
            "base": "flux-schnell",
            "subject": "robot",
        }));
        assert!(refused.is_err());
    }
}
