//! Storage module.
//!
//! SQLite via `rusqlite` (bundled) — [`database`] runs
//! `migrations/001_initial.sql` and does row CRUD against the types in
//! [`crate::models`]; [`files`] manages on-disk audio files; [`search`] queries
//! the FTS5 index (`transcript_fts`, kept in sync by the external-content
//! triggers). See docs/PRD.md §4.3.

pub mod database;
pub mod files;
pub mod search;

pub use database::Database;
pub use files::FileStore;
pub use search::SearchHit;

/// Errors surfaced by the storage layer.
///
/// SQLite and JSON failures are wrapped transparently; everything else is a
/// domain error (missing row, unknown enum string persisted in the DB, I/O on
/// the recordings directory).
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json (de)serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    /// A lookup by primary key found no row.
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },

    /// A column held a value that doesn't map to a known enum variant. This
    /// means the DB and [`crate::models`] have drifted out of sync.
    #[error("invalid `{column}` value stored in database: {value:?}")]
    InvalidEnum {
        column: &'static str,
        value: String,
    },
}

/// Convenience alias for storage results.
pub type Result<T> = std::result::Result<T, StorageError>;
