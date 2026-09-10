//! Web search through a SearXNG instance.
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use zone_search::{SearxngClient, TimeRange, WebSearchConfig};
//!
//! let client = SearxngClient::new(WebSearchConfig::from_env())?;
//! let results = client
//!     .search("rust release notes", Some(TimeRange::Week))
//!     .await?;
//! # let _ = results;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod config;
pub mod observe;
pub mod time_range;

pub use client::{SearxngClient, needs_web_search};
pub use config::{DEFAULT_SEARXNG_QUERY_URL, WebSearchConfig};
pub use observe::{SearchObserver, observe_searches};
pub use time_range::TimeRange;
