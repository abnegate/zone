use serde::Serialize;
use zone_enterprise::{Edition, Status};

use super::entry::Entry;
use super::role::Role;

#[derive(Debug, Serialize)]
pub struct Listing {
    pub entitled: bool,
    pub editable: bool,
    pub edition: Edition,
    pub status: Status,
    pub catalogue: Vec<Entry>,
    pub roles: Vec<Role>,
}
