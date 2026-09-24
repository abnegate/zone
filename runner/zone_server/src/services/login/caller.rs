//! Who is asking to start or finish a sign-in, and in which session.

use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub user: Uuid,
    pub email: String,
    pub session: Uuid,
}
