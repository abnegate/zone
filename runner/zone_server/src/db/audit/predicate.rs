use super::scope::Scope;

pub fn scoped(column: &str, scope: Scope, parameter: u8) -> String {
    match scope {
        Scope::Server => format!("{column} IS NULL AND ${parameter}::uuid IS NULL"),
        Scope::Organization(_) => format!("{column} = ${parameter}"),
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn the_server_scope_matches_rows_without_an_organization() {
        assert_eq!(
            scoped("destination.organization_id", Scope::Server, 1),
            "destination.organization_id IS NULL AND $1::uuid IS NULL"
        );
    }

    #[test]
    fn an_organization_scope_compares_against_its_parameter() {
        assert_eq!(
            scoped("organization_id", Scope::Organization(Uuid::nil()), 2),
            "organization_id = $2"
        );
    }
}
