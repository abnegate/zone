use serde::Deserialize;

/// The order a chat list comes back in. Every order then breaks ties by chat
/// id, so pages of one order never repeat or skip a chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    UpdatedDesc,
    UpdatedAsc,
    CreatedDesc,
    CreatedAsc,
    TitleAsc,
    TitleDesc,
}

impl Sort {
    /// The name the list query matches on; it is also the query parameter.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UpdatedDesc => "updated_desc",
            Self::UpdatedAsc => "updated_asc",
            Self::CreatedDesc => "created_desc",
            Self::CreatedAsc => "created_asc",
            Self::TitleAsc => "title_asc",
            Self::TitleDesc => "title_desc",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Sort;

    #[test]
    fn each_name_parses_back_to_its_sort() {
        for sort in [
            Sort::UpdatedDesc,
            Sort::UpdatedAsc,
            Sort::CreatedDesc,
            Sort::CreatedAsc,
            Sort::TitleAsc,
            Sort::TitleDesc,
        ] {
            let parsed: Sort = serde_json::from_value(serde_json::json!(sort.as_str()))
                .unwrap_or_else(|error| panic!("`{}` must parse: {error}", sort.as_str()));
            assert_eq!(parsed, sort);
        }
    }

    #[test]
    fn updated_descending_is_the_default() {
        assert_eq!(Sort::default(), Sort::UpdatedDesc);
    }
}
