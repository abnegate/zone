use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::edition::Edition;
use crate::entitlement::Feature;
use crate::license::License;
use crate::status::{Status, status};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pause {
    Unlicensed,
    Lapsed,
    Unverified,
}

impl Pause {
    pub const ALL: [Self; 3] = [Self::Unlicensed, Self::Lapsed, Self::Unverified];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unlicensed => "unlicensed",
            Self::Lapsed => "lapsed",
            Self::Unverified => "unverified",
        }
    }
}

impl fmt::Display for Pause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Standing {
    pub edition: Edition,
    pub status: Status,
    pub export: bool,
    pub streaming: bool,
    pub retention: bool,
    pub applies: bool,
    pub pause: Option<Pause>,
}

impl Standing {
    /// `applies` deliberately ignores the status: a lapse keeps deleting what the company committed
    /// to delete, and only removing the license, or moving to a plan without retention, stops it.
    pub fn of(license: Option<&License>, unverified: bool, now: DateTime<Utc>) -> Self {
        let entitled = |feature: Feature| !unverified && feature.entitled(license, now);
        let status = status(license, now);
        Self {
            edition: Edition::from(license),
            status,
            export: entitled(Feature::AuditExport),
            streaming: entitled(Feature::AuditStreaming),
            retention: entitled(Feature::AuditRetention),
            applies: unverified || Feature::AuditRetention.enforced(license),
            pause: pause(license, unverified, status),
        }
    }
}

fn pause(license: Option<&License>, unverified: bool, status: Status) -> Option<Pause> {
    if unverified {
        return Some(Pause::Unverified);
    }
    if !Feature::AuditStreaming.enforced(license) {
        return Some(Pause::Unlicensed);
    }
    if status == Status::Lapsed {
        return Some(Pause::Lapsed);
    }
    None
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::fixtures;
    use crate::payload::Payload;
    use crate::plan::Plan;

    fn license() -> License {
        fixtures::license(&fixtures::payload())
    }

    fn team() -> License {
        fixtures::license(&Payload {
            plan: Some(Plan::Team),
            ..fixtures::payload()
        })
    }

    fn active(license: &License) -> DateTime<Utc> {
        license.issued_at()
    }

    fn grace(license: &License) -> DateTime<Utc> {
        license.expires_at()
    }

    fn lapsed(license: &License) -> DateTime<Utc> {
        license.grace_ends_at() + TimeDelta::days(1)
    }

    #[test]
    fn community_pauses_everything_and_applies_nothing() {
        let now = fixtures::payload().issued_at;
        assert_eq!(
            Standing::of(None, false, now),
            Standing {
                edition: Edition::Community,
                status: Status::Community,
                export: false,
                streaming: false,
                retention: false,
                applies: false,
                pause: Some(Pause::Unlicensed),
            }
        );
    }

    #[test]
    fn active_enterprise_is_entitled_to_everything() {
        let enterprise = license();
        assert_eq!(
            Standing::of(Some(&enterprise), false, active(&enterprise)),
            Standing {
                edition: Edition::Enterprise,
                status: Status::Active,
                export: true,
                streaming: true,
                retention: true,
                applies: true,
                pause: None,
            }
        );
    }

    #[test]
    fn enterprise_in_grace_keeps_everything() {
        let enterprise = license();
        assert_eq!(
            Standing::of(Some(&enterprise), false, grace(&enterprise)),
            Standing {
                edition: Edition::Enterprise,
                status: Status::Grace,
                export: true,
                streaming: true,
                retention: true,
                applies: true,
                pause: None,
            }
        );
    }

    #[test]
    fn lapsed_enterprise_pauses_streaming_and_export_but_keeps_applying_retention() {
        let enterprise = license();
        assert_eq!(
            Standing::of(Some(&enterprise), false, lapsed(&enterprise)),
            Standing {
                edition: Edition::Enterprise,
                status: Status::Lapsed,
                export: false,
                streaming: false,
                retention: false,
                applies: true,
                pause: Some(Pause::Lapsed),
            }
        );
    }

    #[test]
    fn an_unverified_key_applies_retention_and_is_entitled_to_nothing() {
        let now = fixtures::payload().issued_at;
        assert_eq!(
            Standing::of(None, true, now),
            Standing {
                edition: Edition::Community,
                status: Status::Community,
                export: false,
                streaming: false,
                retention: false,
                applies: true,
                pause: Some(Pause::Unverified),
            }
        );
    }

    #[test]
    fn unverified_takes_precedence_over_a_license() {
        let enterprise = license();
        let standing = Standing::of(Some(&enterprise), true, active(&enterprise));
        assert_eq!(standing.pause, Some(Pause::Unverified));
        assert!(!standing.export && !standing.streaming && !standing.retention);
        assert!(standing.applies);
    }

    #[test]
    fn team_is_unlicensed_for_audit_whatever_its_status() {
        let team = team();
        for (now, status) in [
            (active(&team), Status::Active),
            (grace(&team), Status::Grace),
            (lapsed(&team), Status::Lapsed),
        ] {
            assert_eq!(
                Standing::of(Some(&team), false, now),
                Standing {
                    edition: Edition::Team,
                    status,
                    export: false,
                    streaming: false,
                    retention: false,
                    applies: false,
                    pause: Some(Pause::Unlicensed),
                },
                "while {status:?}"
            );
        }
    }

    #[test]
    fn an_unverified_key_over_a_team_license_still_applies_retention() {
        let team = team();
        let standing = Standing::of(Some(&team), true, active(&team));
        assert_eq!(standing.pause, Some(Pause::Unverified));
        assert!(standing.applies);
        assert!(!standing.export && !standing.streaming && !standing.retention);
    }

    #[test]
    fn serializes_its_wire_shape() {
        let enterprise = license();
        assert_eq!(
            serde_json::to_value(Standing::of(Some(&enterprise), false, lapsed(&enterprise)))
                .unwrap(),
            json!({
                "edition": "enterprise",
                "status": "lapsed",
                "export": false,
                "streaming": false,
                "retention": false,
                "applies": true,
                "pause": "lapsed",
            })
        );
        assert_eq!(
            serde_json::to_value(Standing::of(Some(&enterprise), false, active(&enterprise)))
                .unwrap()["pause"],
            json!(null)
        );
    }

    #[test]
    fn pauses_are_the_wire_contract() {
        let values: Vec<&str> = Pause::ALL.iter().map(|pause| pause.as_str()).collect();
        assert_eq!(values, ["unlicensed", "lapsed", "unverified"]);
        for pause in Pause::ALL {
            assert_eq!(serde_json::to_value(pause).unwrap(), pause.as_str());
            assert_eq!(pause.to_string(), pause.as_str());
        }
    }
}
