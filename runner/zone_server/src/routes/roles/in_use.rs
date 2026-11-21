use serde::Serialize;

/// The refusal to delete a role that active members hold, with how many hold it.
#[derive(Debug, Serialize)]
pub struct InUse {
    pub error: &'static str,
    pub code: &'static str,
    pub members: i64,
}
