use uuid::Uuid;

use super::credential::Credential;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant<'a> {
    SignIn(Credential<'a>),
    Refresh(Uuid),
}
