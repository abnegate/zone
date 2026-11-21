use super::*;

const ID: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";

fn product(entry: &str) -> Result<Product, ProductsError> {
    entry.parse()
}

fn listed(plan: Plan) -> Result<Product, ProductsError> {
    Ok(Product {
        id: ID.to_owned(),
        plan,
    })
}

#[test]
fn a_bare_id_is_enterprise() {
    assert_eq!(product(ID), listed(Plan::Enterprise));
}

#[test]
fn an_id_can_name_the_team_plan() {
    assert_eq!(product(&format!("{ID}=team")), listed(Plan::Team));
}

#[test]
fn an_id_can_name_the_enterprise_plan() {
    assert_eq!(
        product(&format!("{ID}=enterprise")),
        listed(Plan::Enterprise)
    );
}

#[test]
fn spaces_around_the_id_and_the_plan_are_ignored() {
    assert_eq!(product(&format!("  {ID} = team ")), listed(Plan::Team));
    assert_eq!(product(&format!(" {ID} ")), listed(Plan::Enterprise));
}

#[test]
fn refuses_an_unknown_plan_naming_the_entry() {
    for plan in ["business", "Team", "ENTERPRISE", "", "team=team"] {
        let entry = format!("{ID}={plan}");
        assert_eq!(
            product(&entry),
            Err(ProductsError::UnknownPlan {
                entry: entry.clone(),
                plan: plan.to_owned(),
            }),
            "{entry}"
        );
    }
}

#[test]
fn refuses_an_empty_id_naming_the_entry() {
    for entry in ["=team", " = enterprise", "=", "=business"] {
        let error = product(entry).unwrap_err();
        assert!(
            matches!(&error, ProductsError::EmptyId { entry: named } if named == entry.trim()),
            "{entry:?}: {error:?}"
        );
    }
}

#[test]
fn each_error_names_its_entry() {
    let unknown = product(&format!("{ID}=business")).unwrap_err().to_string();
    assert!(unknown.contains(&format!("{ID}=business")), "{unknown}");
    assert!(unknown.contains("team or enterprise"), "{unknown}");
    let empty = product("=team").unwrap_err().to_string();
    assert!(empty.contains("\"=team\""), "{empty}");
}
