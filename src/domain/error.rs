use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Manifest file not found: {0}")]
    ManifestNotFound(String),

    #[error("Script file not found: {0}")]
    ScriptNotFound(String),

    #[error("Checksum mismatch for script '{path}': expected {expected}, actual {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("Cross-client contamination blocked: migration '{migration_id}' belongs to client '{migration_client}' and cannot be applied to target client '{target_client}'")]
    CrossClientContaminationBlocked {
        migration_id: String,
        migration_client: String,
        target_client: String,
    },

    #[error("Product mismatch: migration '{migration_id}' is for product '{migration_product}', but target product is '{target_product}'")]
    ProductMismatch {
        migration_id: String,
        migration_product: String,
        target_product: String,
    },

    #[error("Dependency cycle detected in migrations graph involving: {0}")]
    DependencyCycle(String),

    #[error("Missing prerequisite migration '{missing_dep}' required by '{migration_id}'")]
    MissingDependency {
        migration_id: String,
        missing_dep: String,
    },

    #[error("Destructive migration '{0}' requires explicit '--allow-destructive' confirmation")]
    DestructiveMigrationNotAllowed(String),

    #[error("Database operation failed: {0}")]
    DatabaseError(String),

    #[error("Unsupported database engine for this operation: {0}")]
    UnsupportedEngine(String),
}
