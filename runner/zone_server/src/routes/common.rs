//! Common response types and utilities for API routes

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;

/// Macro to define a response struct with automatic timestamps.
///
/// This macro generates a struct with the specified fields plus a flattened
/// `Timestamps` field that provides `created_at` and `updated_at`.
///
/// # Example
/// ```ignore
/// response_struct! {
///     /// My response documentation
///     pub struct MyResponse {
///         pub id: Uuid,
///         pub name: String,
///     }
/// }
/// ```
///
/// Expands to:
/// ```ignore
/// #[derive(Debug, Serialize)]
/// pub struct MyResponse {
///     pub id: Uuid,
///     pub name: String,
///     #[serde(flatten)]
///     pub timestamps: Timestamps,
/// }
/// ```
#[macro_export]
macro_rules! response_struct {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, serde::Serialize)]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $ty,
            )*
            #[serde(flatten)]
            pub timestamps: $crate::routes::common::Timestamps,
        }
    };
}

pub use response_struct;

/// Base timestamps that should be included in all entity responses.
/// Use `#[serde(flatten)]` to embed these fields in your response structs.
///
/// Includes `deleted_at` for soft delete support - only serialized when present.
///
/// # Example
/// ```ignore
/// #[derive(Serialize)]
/// struct MyResponse {
///     id: Uuid,
///     name: String,
///     #[serde(flatten)]
///     timestamps: Timestamps,
/// }
/// ```
#[derive(Debug, Clone, Serialize, Default)]
pub struct Timestamps {
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl Timestamps {
    /// Create timestamps from optional NaiveDateTime values
    pub fn from_naive(
        created_at: Option<NaiveDateTime>,
        updated_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            created_at: created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            updated_at: updated_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            deleted_at: None,
        }
    }

    /// Create timestamps from optional NaiveDateTime values including deleted_at
    pub fn from_naive_with_deleted(
        created_at: Option<NaiveDateTime>,
        updated_at: Option<NaiveDateTime>,
        deleted_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            created_at: created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            updated_at: updated_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
            deleted_at: deleted_at.map(|dt| dt.and_utc().to_rfc3339()),
        }
    }

    /// Create timestamps from DateTime<Utc> values
    pub fn from_utc(created_at: DateTime<Utc>, updated_at: DateTime<Utc>) -> Self {
        Self {
            created_at: created_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            deleted_at: None,
        }
    }

    /// Create timestamps from DateTime<Utc> values including deleted_at
    pub fn from_utc_with_deleted(
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            deleted_at: deleted_at.map(|dt| dt.to_rfc3339()),
        }
    }

    /// Create timestamps from optional DateTime<Utc> values
    pub fn from_utc_opt(
        created_at: Option<DateTime<Utc>>,
        updated_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            updated_at: updated_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            deleted_at: None,
        }
    }

    /// Create timestamps from optional DateTime<Utc> values including deleted_at
    pub fn from_utc_opt_with_deleted(
        created_at: Option<DateTime<Utc>>,
        updated_at: Option<DateTime<Utc>>,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            created_at: created_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            updated_at: updated_at.map(|dt| dt.to_rfc3339()).unwrap_or_default(),
            deleted_at: deleted_at.map(|dt| dt.to_rfc3339()),
        }
    }

    /// Create timestamps with current time for both fields
    pub fn now() -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Set deleted_at timestamp
    pub fn with_deleted(mut self, deleted_at: Option<DateTime<Utc>>) -> Self {
        self.deleted_at = deleted_at.map(|dt| dt.to_rfc3339());
        self
    }

    /// Set deleted_at timestamp from NaiveDateTime
    pub fn with_deleted_naive(mut self, deleted_at: Option<NaiveDateTime>) -> Self {
        self.deleted_at = deleted_at.map(|dt| dt.and_utc().to_rfc3339());
        self
    }
}

/// Standard error response format
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}
