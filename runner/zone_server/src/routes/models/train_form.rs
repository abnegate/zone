use super::method::Method;
use super::subject::Subject;
use crate::services::comfy::language;
use crate::services::comfy::policy::VIDEO_REFUSAL;
use abnegate_comfy::host::Compute;
use abnegate_comfy::lora::TrainError;
use abnegate_comfy::lora::TrainImage;
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
    pub method: Method,
    #[serde(default)]
    pub images: Vec<TrainImage>,
    #[serde(default, rename = "provider")]
    pub compute: Compute,
}

impl TrainForm {
    /// The image request the crate trains, refusing video identity training,
    /// which the crate no longer has a method for.
    pub fn image(self, subject: TrainSubject) -> Result<TrainRequest, TrainError> {
        let method = self
            .method
            .train()
            .ok_or(TrainError::Invalid(VIDEO_REFUSAL))?;
        let request = TrainRequest::new(self.name, self.base, self.images)
            .with_subject(subject)
            .with_method(method);
        Ok(match self.trigger {
            Some(trigger) => request.with_trigger(trigger),
            None => request,
        })
    }

    /// The chat request zone trains, refusing video as it refuses every
    /// method a chat model cannot take.
    pub fn language(self) -> Result<language::Request, TrainError> {
        let method = self
            .method
            .train()
            .ok_or(TrainError::Invalid(language::UNAVAILABLE_METHOD))?;
        Ok(language::Request::new(self.name, self.base, self.images).with_method(method))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use abnegate_comfy::lora::TrainMethod;

    fn form(body: serde_json::Value) -> TrainForm {
        serde_json::from_value(body).expect("a train form")
    }

    fn refusal<T: std::fmt::Debug>(result: Result<T, TrainError>) -> &'static str {
        match result {
            Err(TrainError::Invalid(message)) => message,
            other => panic!("expected an invalid request, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_form_trains_a_local_lora_of_anything() {
        let form = form(serde_json::json!({"name": "jerry", "base": "flux-schnell"}));
        assert_eq!(form.subject, Subject::Other);
        assert_eq!(form.method, Method::Lora);
        assert_eq!(form.compute, Compute::Local);
        assert!(form.images.is_empty());
        let request = form.image(TrainSubject::Other).expect("a LoRA trains");
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
        let request = form
            .image(TrainSubject::Person)
            .expect("a fine-tune trains");
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
        let request = form.language().expect("a chat fine-tune trains");
        assert_eq!(request.name, "support-bot");
        assert_eq!(request.base, "llama3.2:1b");
        assert_eq!(request.method, TrainMethod::Finetune);
        assert_eq!(request.documents.len(), 1);
    }

    #[test]
    fn a_video_form_is_refused_before_it_becomes_a_request() {
        let video = |subject: Subject| {
            form(serde_json::json!({
                "name": "jerry",
                "base": "sdxl-people",
                "subject": subject.as_str(),
                "method": "video",
            }))
        };
        for subject in [Subject::Person, Subject::Other] {
            let form = video(subject);
            assert_eq!(form.method, Method::Video);
            let image = subject.image().expect("an image subject");
            assert_eq!(refusal(form.image(image)), VIDEO_REFUSAL);
        }
        assert_eq!(
            refusal(video(Subject::Language).language()),
            language::UNAVAILABLE_METHOD
        );
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
