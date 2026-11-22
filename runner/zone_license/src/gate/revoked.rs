use std::collections::HashSet;

use uuid::Uuid;
use zone_enterprise::License;

use crate::gate::unparsable::Unparsable;

pub const SEPARATOR: char = ',';

const LICENSE_EXPECTED: &str = "a license ID, which is a UUID";
const SUBSCRIPTION_EXPECTED: &str = "a subscription ID, which is a UUID";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revoked {
    licenses: HashSet<Uuid>,
    subscriptions: HashSet<Uuid>,
}

impl Revoked {
    pub fn new(licenses: HashSet<Uuid>, subscriptions: HashSet<Uuid>) -> Self {
        Self {
            licenses,
            subscriptions,
        }
    }

    pub fn parse_licenses(list: &str) -> Result<HashSet<Uuid>, Unparsable> {
        parse(list, LICENSE_EXPECTED)
    }

    pub fn parse_subscriptions(list: &str) -> Result<HashSet<Uuid>, Unparsable> {
        parse(list, SUBSCRIPTION_EXPECTED)
    }

    pub fn covers(&self, license: &License) -> bool {
        self.licenses.contains(&license.id())
            || license
                .subscription()
                .and_then(|subscription| Uuid::try_parse(subscription).ok())
                .is_some_and(|subscription| self.subscriptions.contains(&subscription))
    }

    pub fn licenses(&self) -> &HashSet<Uuid> {
        &self.licenses
    }

    pub fn subscriptions(&self) -> &HashSet<Uuid> {
        &self.subscriptions
    }

    pub fn is_empty(&self) -> bool {
        self.licenses.is_empty() && self.subscriptions.is_empty()
    }
}

fn parse(list: &str, expected: &'static str) -> Result<HashSet<Uuid>, Unparsable> {
    list.split(SEPARATOR)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            Uuid::try_parse(entry).map_err(|_| Unparsable {
                entry: entry.to_owned(),
                expected,
            })
        })
        .collect()
}
