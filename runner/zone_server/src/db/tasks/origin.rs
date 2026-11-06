use serde_json::Value;

/// What put a task on the board: a member, or an agent turn nobody typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Member,
    Reminder,
    Automation,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Reminder => "reminder",
            Self::Automation => "automation",
        }
    }

    /// The origin of a task an agent creates during the turn `metadata`
    /// opened: only a turn without a `source` is one a member typed.
    pub fn of_turn(metadata: Option<&Value>) -> Self {
        match metadata
            .and_then(Value::as_object)
            .and_then(|fields| fields.get("source"))
        {
            None => Self::Member,
            Some(source) if source == "reminder" => Self::Reminder,
            Some(_) => Self::Automation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_typed_turn_is_a_member() {
        assert_eq!(Origin::of_turn(None), Origin::Member);
        assert_eq!(Origin::of_turn(Some(&Value::Null)), Origin::Member);
        assert_eq!(
            Origin::of_turn(Some(&json!({"attachments": []}))),
            Origin::Member
        );
    }

    #[test]
    fn a_sourced_turn_is_not_a_member() {
        assert_eq!(
            Origin::of_turn(Some(&json!({"source": "reminder"}))),
            Origin::Reminder
        );
        assert_eq!(
            Origin::of_turn(Some(&json!({"source": "auto_project"}))),
            Origin::Automation
        );
        assert_eq!(
            Origin::of_turn(Some(&json!({"source": null}))),
            Origin::Automation
        );
    }
}
