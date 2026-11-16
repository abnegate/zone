use serde_json::{Value, json};
use uuid::Uuid;

/// The account a single sign-on lands in, and how it got there: `provisioned` for an account the
/// sign-in created, `linked` for an existing account the identity was first linked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settlement {
    pub user: Uuid,
    pub provisioned: bool,
    pub linked: bool,
    pub reclaimed: bool,
}

impl Settlement {
    pub fn signed_in(user: Uuid) -> Self {
        Self {
            user,
            provisioned: false,
            linked: false,
            reclaimed: false,
        }
    }

    pub fn linked(user: Uuid, reclaimed: bool) -> Self {
        Self {
            linked: true,
            reclaimed,
            ..Self::signed_in(user)
        }
    }

    pub fn provisioned(user: Uuid) -> Self {
        Self {
            provisioned: true,
            ..Self::signed_in(user)
        }
    }

    pub fn recorded(&self, provider_id: Uuid, subject: &str) -> Value {
        json!({
            "provider_id": provider_id,
            "subject": subject,
            "provisioned": self.provisioned,
            "linked": self.linked,
            "reclaimed": self.reclaimed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audit_names_how_the_account_was_reached() {
        let user = Uuid::from_u128(1);
        let provider = Uuid::from_u128(2);
        let cases = [
            (Settlement::signed_in(user), false, false, false),
            (Settlement::linked(user, false), false, true, false),
            (Settlement::linked(user, true), false, true, true),
            (Settlement::provisioned(user), true, false, false),
        ];
        for (settlement, provisioned, linked, reclaimed) in cases {
            assert_eq!(settlement.user, user);
            assert_eq!(
                settlement.recorded(provider, "sub-1"),
                json!({
                    "provider_id": provider,
                    "subject": "sub-1",
                    "provisioned": provisioned,
                    "linked": linked,
                    "reclaimed": reclaimed,
                })
            );
        }
    }
}
