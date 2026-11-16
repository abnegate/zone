#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasswordAccess {
    Allowed,
    BreakGlass,
    Refused,
}

pub fn password_access(required: bool, admin: bool) -> PasswordAccess {
    match (required, admin) {
        (false, _) => PasswordAccess::Allowed,
        (true, true) => PasswordAccess::BreakGlass,
        (true, false) => PasswordAccess::Refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_enforcement_table() {
        for (required, admin, expected) in [
            (false, false, PasswordAccess::Allowed),
            (false, true, PasswordAccess::Allowed),
            (true, true, PasswordAccess::BreakGlass),
            (true, false, PasswordAccess::Refused),
        ] {
            assert_eq!(
                password_access(required, admin),
                expected,
                "required {required}, admin {admin}"
            );
        }
    }
}
