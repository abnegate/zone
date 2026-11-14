use serde::Deserialize;

const REQUESTED: [&str; 2] = ["1", "true"];

#[derive(Debug, Default, Deserialize)]
pub struct RefreshQuery {
    #[serde(default)]
    pub refresh: Option<String>,
}

impl RefreshQuery {
    pub fn requested(&self) -> bool {
        Self::asked(self.refresh.as_deref())
    }

    pub fn asked(refresh: Option<&str>) -> bool {
        refresh.is_some_and(|value| {
            REQUESTED
                .iter()
                .any(|requested| value.eq_ignore_ascii_case(requested))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refresh_asked_for_refreshes() {
        for (refresh, requested) in [
            (None, false),
            (Some("1"), true),
            (Some("true"), true),
            (Some("TRUE"), true),
            (Some("0"), false),
            (Some("false"), false),
            (Some("yes"), false),
            (Some(""), false),
        ] {
            let query = RefreshQuery {
                refresh: refresh.map(str::to_string),
            };
            assert_eq!(query.requested(), requested, "{refresh:?}");
            assert_eq!(RefreshQuery::asked(refresh), requested, "{refresh:?}");
        }
    }
}
