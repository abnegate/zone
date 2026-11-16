use uuid::Uuid;

use crate::db::audit::resources;

/// What a policy denied: a chat's turn, a task's run, or something the workspace started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Chat(Uuid),
    Task(Uuid),
    Workspace(Uuid),
}

impl Subject {
    pub fn resource(self) -> &'static str {
        match self {
            Self::Chat(_) => resources::CHAT,
            Self::Task(_) => resources::TASK,
            Self::Workspace(_) => resources::WORKSPACE,
        }
    }

    pub fn id(self) -> Uuid {
        match self {
            Self::Chat(id) | Self::Task(id) | Self::Workspace(id) => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_subject_is_audited_as_its_own_resource() {
        let id = Uuid::new_v4();
        for (subject, resource) in [
            (Subject::Chat(id), "chat"),
            (Subject::Task(id), "task"),
            (Subject::Workspace(id), "workspace"),
        ] {
            assert_eq!(subject.resource(), resource);
            assert_eq!(subject.id(), id);
        }
    }
}
