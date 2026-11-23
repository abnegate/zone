use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admitted(Vec<Uuid>),
    Strangers(Vec<Uuid>),
}
