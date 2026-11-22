use crate::scim::list_query::ListQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    start_index: u64,
    count: u64,
}

impl Page {
    pub const DEFAULT_COUNT: u64 = 100;
    pub const MAX_RESULTS: u64 = 1000;

    pub fn offset(&self) -> i64 {
        i64::try_from(self.start_index - 1).unwrap_or(i64::MAX)
    }

    pub fn limit(&self) -> i64 {
        i64::try_from(self.count).unwrap_or(i64::MAX)
    }

    pub fn start_index(&self) -> u64 {
        self.start_index
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

impl From<&ListQuery> for Page {
    fn from(query: &ListQuery) -> Self {
        let start_index = query
            .start_index
            .map_or(1, |start_index| start_index.max(1).unsigned_abs());
        let count = query.count.map_or(Self::DEFAULT_COUNT, |count| {
            count.max(0).unsigned_abs().min(Self::MAX_RESULTS)
        });
        Self { start_index, count }
    }
}
