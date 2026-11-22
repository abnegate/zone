use uuid::Uuid;

use super::namespace::Namespace;

const PREFIX: &str = "zone";
const CHAT: &str = "chat";
const FRAMES: &str = "frames";
const CONTROL: &str = "control";
const INSTANCE: &str = "instance";
const TASK_RUNS: &str = "task-runs";
const TITLES: &str = "titles";
const ACTIONS: &str = "actions";

/// Where a message on the bus goes. Each topic is one pub/sub channel, inside
/// the namespace the cluster's master key derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Topic {
    ChatFrames(Uuid),
    ChatControl(Uuid),
    Instance(Uuid),
    TaskRuns,
    Titles,
    Actions,
}

impl Topic {
    pub fn channel(&self, namespace: &Namespace) -> String {
        match self {
            Self::ChatFrames(chat) => format!("{PREFIX}:{namespace}:{CHAT}:{chat}:{FRAMES}"),
            Self::ChatControl(chat) => format!("{PREFIX}:{namespace}:{CHAT}:{chat}:{CONTROL}"),
            Self::Instance(instance) => format!("{PREFIX}:{namespace}:{INSTANCE}:{instance}"),
            Self::TaskRuns => format!("{PREFIX}:{namespace}:{TASK_RUNS}"),
            Self::Titles => format!("{PREFIX}:{namespace}:{TITLES}"),
            Self::Actions => format!("{PREFIX}:{namespace}:{ACTIONS}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    const PASSPHRASE: &str = "a-cluster-passphrase-of-at-least-32-bytes";
    const CHAT_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn every_topic_names_its_own_channel_inside_the_namespace() {
        let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());
        let chat = Uuid::parse_str(CHAT_ID).unwrap();
        let expected = [
            (Topic::ChatFrames(chat), format!("chat:{CHAT_ID}:frames")),
            (Topic::ChatControl(chat), format!("chat:{CHAT_ID}:control")),
            (Topic::Instance(chat), format!("instance:{CHAT_ID}")),
            (Topic::TaskRuns, "task-runs".to_string()),
            (Topic::Titles, "titles".to_string()),
            (Topic::Actions, "actions".to_string()),
        ];

        for (topic, suffix) in expected {
            assert_eq!(
                topic.channel(&namespace),
                format!("zone:{namespace}:{suffix}")
            );
        }
    }

    #[test]
    fn topics_for_the_same_id_never_share_a_channel() {
        let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());
        let id = Uuid::new_v4();
        let channels = [
            Topic::ChatFrames(id).channel(&namespace),
            Topic::ChatControl(id).channel(&namespace),
            Topic::Instance(id).channel(&namespace),
        ];

        assert_ne!(channels[0], channels[1]);
        assert_ne!(channels[1], channels[2]);
        assert_ne!(channels[0], channels[2]);
    }
}
