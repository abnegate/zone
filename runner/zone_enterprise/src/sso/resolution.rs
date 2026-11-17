use uuid::Uuid;

use crate::sso::account::Account;
use crate::sso::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    SignIn(Uuid),
    Link { user: Uuid, reclaim: bool },
    Provision,
}

pub fn resolve(
    by_subject: Option<Account>,
    by_email: &[Account],
    linked: bool,
    require_verified_email: bool,
) -> Result<Resolution, Error> {
    if let Some(account) = by_subject {
        return if account.active {
            Ok(Resolution::SignIn(account.id))
        } else {
            Err(Error::AccountDisabled)
        };
    }
    match by_email {
        [] => Ok(Resolution::Provision),
        [account] => {
            if linked || !require_verified_email {
                return Err(Error::EmailConflict);
            }
            if !account.active {
                return Err(Error::AccountDisabled);
            }
            Ok(Resolution::Link {
                user: account.id,
                reclaim: !account.email_verified && !account.admin,
            })
        }
        _ => Err(Error::EmailConflict),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: u128) -> Account {
        Account {
            id: Uuid::from_u128(id),
            active: true,
            email_verified: true,
            admin: false,
        }
    }

    fn unverified(id: u128) -> Account {
        Account {
            email_verified: false,
            ..account(id)
        }
    }

    fn admin(id: u128) -> Account {
        Account {
            admin: true,
            ..account(id)
        }
    }

    fn inactive(id: u128) -> Account {
        Account {
            active: false,
            ..account(id)
        }
    }

    #[test]
    fn a_known_subject_signs_in_whatever_the_email() {
        for by_email in [vec![], vec![account(2)], vec![account(2), account(3)]] {
            assert_eq!(
                resolve(Some(account(1)), &by_email, true, false),
                Ok(Resolution::SignIn(Uuid::from_u128(1)))
            );
        }
    }

    #[test]
    fn an_inactive_subject_is_refused() {
        assert_eq!(
            resolve(Some(inactive(1)), &[], false, true),
            Err(Error::AccountDisabled)
        );
    }

    #[test]
    fn no_match_provisions() {
        for require_verified_email in [true, false] {
            assert_eq!(
                resolve(None, &[], false, require_verified_email),
                Ok(Resolution::Provision)
            );
        }
    }

    #[test]
    fn a_verified_local_account_links_without_reclaim() {
        assert_eq!(
            resolve(None, &[account(2)], false, true),
            Ok(Resolution::Link {
                user: Uuid::from_u128(2),
                reclaim: false
            })
        );
    }

    #[test]
    fn an_unverified_local_account_is_reclaimed() {
        assert_eq!(
            resolve(None, &[unverified(2)], false, true),
            Ok(Resolution::Link {
                user: Uuid::from_u128(2),
                reclaim: true
            })
        );
    }

    #[test]
    fn an_existing_account_is_never_linked_when_verification_is_off() {
        for (kind, local) in [
            ("verified", account(2)),
            ("unverified", unverified(2)),
            ("admin", admin(2)),
            ("inactive", inactive(2)),
        ] {
            assert_eq!(
                resolve(None, &[local], false, false),
                Err(Error::EmailConflict),
                "{kind} account"
            );
        }
    }

    #[test]
    fn a_known_subject_still_signs_in_when_verification_is_off() {
        assert_eq!(
            resolve(Some(account(1)), &[account(2)], true, false),
            Ok(Resolution::SignIn(Uuid::from_u128(1)))
        );
    }

    #[test]
    fn an_admin_links_when_verification_is_on() {
        assert_eq!(
            resolve(None, &[admin(2)], false, true),
            Ok(Resolution::Link {
                user: Uuid::from_u128(2),
                reclaim: false
            })
        );
    }

    #[test]
    fn an_unverified_admin_is_never_reclaimed() {
        let local = Account {
            email_verified: false,
            ..admin(2)
        };
        assert_eq!(
            resolve(None, &[local], false, true),
            Ok(Resolution::Link {
                user: Uuid::from_u128(2),
                reclaim: false
            })
        );
    }

    #[test]
    fn an_account_linked_through_any_provider_conflicts() {
        assert_eq!(
            resolve(None, &[account(2)], true, true),
            Err(Error::EmailConflict)
        );
    }

    #[test]
    fn an_ambiguous_email_conflicts() {
        assert_eq!(
            resolve(None, &[account(2), account(3)], false, true),
            Err(Error::EmailConflict)
        );
    }

    #[test]
    fn an_inactive_email_match_is_refused() {
        assert_eq!(
            resolve(None, &[inactive(2)], false, true),
            Err(Error::AccountDisabled)
        );
    }
}
