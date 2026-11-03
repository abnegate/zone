use crate::services::comfy::language;
use abnegate_comfy::lora::TrainSubject;
use serde::{Deserialize, Serialize};

const OTHER: &str = "other";
const PERSON: &str = "person";

/// What a training set shows, as the client names it: the image subjects
/// the crate trains, and the chat documents zone trains itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    #[default]
    Other,
    Person,
    Language,
}

impl Subject {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OTHER => Some(Self::Other),
            PERSON => Some(Self::Person),
            language::SUBJECT => Some(Self::Language),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Other => OTHER,
            Self::Person => PERSON,
            Self::Language => language::SUBJECT,
        }
    }

    /// The image subject the crate trains, or `None` for chat documents.
    pub fn image(self) -> Option<TrainSubject> {
        match self {
            Self::Other => Some(TrainSubject::Other),
            Self::Person => Some(TrainSubject::Person),
            Self::Language => None,
        }
    }

    pub fn is_other(&self) -> bool {
        *self == Self::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subject_reads_back_from_its_own_name() {
        for subject in [Subject::Other, Subject::Person, Subject::Language] {
            assert_eq!(Subject::parse(subject.as_str()), Some(subject));
            assert_eq!(
                serde_json::to_value(subject).unwrap(),
                serde_json::json!(subject.as_str())
            );
        }
        assert_eq!(Subject::parse("robot"), None);
    }

    #[test]
    fn only_language_leaves_the_crate() {
        assert_eq!(Subject::Other.image(), Some(TrainSubject::Other));
        assert_eq!(Subject::Person.image(), Some(TrainSubject::Person));
        assert_eq!(Subject::Language.image(), None);
    }
}
