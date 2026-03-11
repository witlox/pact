//! Test fixtures and builders for pact types.
//!
//! Uses the builder pattern for constructing test data:
//! ```ignore
//! let config = ConfigEntryBuilder::new()
//!     .vcluster("ml-training")
//!     .author("admin@example.org")
//!     .build();
//! ```
