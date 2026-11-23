#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent { code: Option<String> },
}
