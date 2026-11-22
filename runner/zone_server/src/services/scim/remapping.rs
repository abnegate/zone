use super::remapped::Remapped;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remapping {
    Remapped(Box<Remapped>),
    GroupMissing,
    RoleMissing,
    OwnerUnmappable,
    Exceeds,
}
