use std::fmt;

/// A table, named by its schema and its name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, sqlx::FromRow)]
pub struct Table {
    schema: String,
    name: String,
}

impl Table {
    /// The table `name` in `schema`.
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
        }
    }

    /// The schema the table is in.
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The table's name within its schema.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for Table {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.schema, self.name)
    }
}
