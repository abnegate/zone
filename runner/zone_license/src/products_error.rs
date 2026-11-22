#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProductsError {
    #[error("it must list at least one Polar product ID, separated by commas")]
    Empty,
    #[error("the entry {entry:?} names the plan {plan:?}; use team or enterprise")]
    UnknownPlan { entry: String, plan: String },
    #[error("the entry {entry:?} has no product ID before the =")]
    EmptyId { entry: String },
    #[error("the entry {entry:?} lists the product ID {id:?} again")]
    Duplicate { entry: String, id: String },
}
