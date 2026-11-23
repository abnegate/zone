use serde::Deserialize;

use crate::scim::attribute::MEMBERS;
use crate::scim::error::Error;
use crate::scim::filter::Filter;
use crate::scim::urn;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListQuery {
    pub filter: Option<String>,
    pub start_index: Option<i64>,
    pub count: Option<i64>,
    pub attributes: Option<String>,
    pub excluded_attributes: Option<String>,
}

impl ListQuery {
    pub fn parsed_filter(&self) -> Result<Option<Filter>, Error> {
        self.filter
            .as_deref()
            .filter(|filter| !filter.trim().is_empty())
            .map(Filter::parse)
            .transpose()
    }

    pub fn excludes_members(&self) -> bool {
        if names(self.excluded_attributes.as_deref()).any(is_members) {
            return true;
        }
        let mut requested = names(self.attributes.as_deref()).peekable();
        requested.peek().is_some() && !requested.any(is_members)
    }
}

fn names(list: Option<&str>) -> impl Iterator<Item = &str> {
    list.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn is_members(name: &str) -> bool {
    let name = urn::strip_core(name);
    name.eq_ignore_ascii_case(MEMBERS)
        || name
            .split_once('.')
            .is_some_and(|(base, _)| base.eq_ignore_ascii_case(MEMBERS))
}
