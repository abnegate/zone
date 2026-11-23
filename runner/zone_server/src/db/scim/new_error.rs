use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewError<'a> {
    pub organization_id: Uuid,
    pub token_id: Option<Uuid>,
    pub method: &'a str,
    pub path: &'a str,
    pub status: u16,
    pub scim_type: Option<&'a str>,
    pub detail: &'a str,
}
