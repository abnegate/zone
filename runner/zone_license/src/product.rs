use std::str::FromStr;

use zone_enterprise::Plan;

use crate::products_error::ProductsError;

pub const PLAN_SEPARATOR: char = '=';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: String,
    pub plan: Plan,
}

impl FromStr for Product {
    type Err = ProductsError;

    fn from_str(entry: &str) -> Result<Self, Self::Err> {
        let entry = entry.trim();
        let (id, plan) = match entry.split_once(PLAN_SEPARATOR) {
            Some((id, plan)) => (id.trim(), Some(plan.trim())),
            None => (entry, None),
        };
        if id.is_empty() {
            return Err(ProductsError::EmptyId {
                entry: entry.to_owned(),
            });
        }
        let plan = match plan {
            Some(plan) => plan.parse().map_err(|_| ProductsError::UnknownPlan {
                entry: entry.to_owned(),
                plan: plan.to_owned(),
            })?,
            None => Plan::Enterprise,
        };
        Ok(Self {
            id: id.to_owned(),
            plan,
        })
    }
}

#[cfg(test)]
mod tests;
