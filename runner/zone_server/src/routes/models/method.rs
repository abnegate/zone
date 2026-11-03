use abnegate_comfy::lora::TrainMethod;
use serde::{Deserialize, Serialize};

const VIDEO: &str = "video";

/// How a training set becomes a weight, as the client names it: the methods
/// the crate trains, and video, which zone still recognises so it can refuse
/// it by name instead of training a LoRA in its place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    #[default]
    Lora,
    Finetune,
    Pivotal,
    Video,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self.train() {
            Some(method) => method.as_str(),
            None => VIDEO,
        }
    }

    /// The method the crate trains, or `None` for video, which nothing trains.
    pub fn train(self) -> Option<TrainMethod> {
        match self {
            Self::Lora => Some(TrainMethod::Lora),
            Self::Finetune => Some(TrainMethod::Finetune),
            Self::Pivotal => Some(TrainMethod::Pivotal),
            Self::Video => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_reads_back_from_its_own_name() {
        for method in [
            Method::Lora,
            Method::Finetune,
            Method::Pivotal,
            Method::Video,
        ] {
            assert_eq!(
                serde_json::from_value::<Method>(serde_json::json!(method.as_str())).unwrap(),
                method
            );
        }
    }

    #[test]
    fn every_method_but_video_is_one_the_crate_trains() {
        for method in [Method::Lora, Method::Finetune, Method::Pivotal] {
            let trained = method.train().expect("the crate trains it");
            assert_eq!(trained.as_str(), method.as_str());
        }
        assert_eq!(Method::Video.train(), None);
        assert_eq!(Method::Video.as_str(), VIDEO);
    }
}
