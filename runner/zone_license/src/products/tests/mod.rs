use super::*;

const ENTERPRISE: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const TEAM: &str = "8a2b6c1d-4e3f-4a5b-9c6d-7e8f9a0b1c2d";
const YEARLY: &str = "0e1f2a3b-4c5d-4e6f-8a9b-0c1d2e3f4a5b";

#[test]
fn a_bare_id_issues_enterprise() {
    let products = Products::parse(ENTERPRISE).unwrap();
    assert_eq!(products.plan(ENTERPRISE), Some(Plan::Enterprise));
    assert_eq!(products.len(), 1);
}

#[test]
fn maps_each_listed_product_to_its_plan() {
    let products =
        Products::parse(&format!("{ENTERPRISE},{TEAM}=team,{YEARLY}=enterprise")).unwrap();
    assert_eq!(products.plan(ENTERPRISE), Some(Plan::Enterprise));
    assert_eq!(products.plan(TEAM), Some(Plan::Team));
    assert_eq!(products.plan(YEARLY), Some(Plan::Enterprise));
    assert_eq!(products.len(), 3);
    assert!(!products.is_empty());
}

#[test]
fn iterates_products_in_listed_order() {
    let products = Products::parse(&format!("{TEAM}=team,{ENTERPRISE}")).unwrap();
    let listed: Vec<(&str, Plan)> = products
        .iter()
        .map(|product| (product.id.as_str(), product.plan))
        .collect();
    assert_eq!(listed, [(TEAM, Plan::Team), (ENTERPRISE, Plan::Enterprise)]);
}

#[test]
fn tolerates_spaces_and_blank_entries() {
    let products = Products::parse(&format!(" {ENTERPRISE} , ,{TEAM} = team ,,")).unwrap();
    assert_eq!(products.plan(ENTERPRISE), Some(Plan::Enterprise));
    assert_eq!(products.plan(TEAM), Some(Plan::Team));
    assert_eq!(products.len(), 2);
}

#[test]
fn an_unlisted_product_has_no_plan() {
    let products = Products::parse(&format!("{TEAM}=team")).unwrap();
    assert_eq!(products.plan(ENTERPRISE), None);
    assert_eq!(products.plan(""), None);
}

#[test]
fn ids_match_exactly() {
    let products = Products::parse(TEAM).unwrap();
    assert_eq!(products.plan(&TEAM.to_uppercase()), None);
    assert_eq!(products.plan(&format!(" {TEAM}")), None);
}

#[test]
fn refuses_an_empty_list() {
    for list in ["", " ", ",", " , ,"] {
        assert_eq!(Products::parse(list), Err(ProductsError::Empty), "{list:?}");
    }
}

#[test]
fn refuses_an_unknown_plan_naming_the_entry() {
    assert_eq!(
        Products::parse(&format!("{ENTERPRISE}, {TEAM}=business")),
        Err(ProductsError::UnknownPlan {
            entry: format!("{TEAM}=business"),
            plan: "business".to_owned(),
        })
    );
}

#[test]
fn refuses_an_empty_id_naming_the_entry() {
    assert_eq!(
        Products::parse(&format!("{ENTERPRISE},=team")),
        Err(ProductsError::EmptyId {
            entry: "=team".to_owned(),
        })
    );
}

#[test]
fn refuses_the_same_id_twice_naming_the_entry() {
    for (list, entry) in [
        (format!("{TEAM},{TEAM}"), TEAM.to_owned()),
        (
            format!("{TEAM}=team, {TEAM} = enterprise"),
            format!("{TEAM} = enterprise"),
        ),
        (format!("{TEAM}=team,{TEAM}=team"), format!("{TEAM}=team")),
    ] {
        assert_eq!(
            Products::parse(&list),
            Err(ProductsError::Duplicate {
                entry,
                id: TEAM.to_owned(),
            }),
            "{list}"
        );
    }
}

#[test]
fn the_duplicate_error_names_the_entry_and_the_id() {
    let message = Products::parse(&format!("{TEAM},{TEAM}=team"))
        .unwrap_err()
        .to_string();
    assert!(message.contains(&format!("\"{TEAM}=team\"")), "{message}");
    assert!(message.contains("again"), "{message}");
}

#[test]
fn debug_shows_each_product_and_its_plan() {
    let shown = format!("{:?}", Products::parse(&format!("{TEAM}=team")).unwrap());
    assert!(shown.contains(TEAM), "{shown}");
    assert!(shown.contains("Team"), "{shown}");
}
