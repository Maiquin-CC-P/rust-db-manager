use crate::domain::error::DomainError;
use crate::domain::inventory::{
    AppliedMigrationRecord, DatabaseSchemaInventory, InstanceMetadata,
};
use async_trait::async_trait;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMode {
    /// Clean deterministic rebuild from product baseline scripts + optional client migrations
    Rebuild,
    /// Physical SQL Server `BACKUP DATABASE ... WITH COPY_ONLY` + `RESTORE ... WITH MOVE`
    BackupRestore,
    /// Schema-only clone via `SqlPackage` DACPAC extract & publish
    SchemaClone,
}

/// Hexagonal Port for interacting with any Database Engine (SQL Server in MVP 0.1, MySQL in MVP 0.2).
#[async_trait]
pub trait DatabaseEnginePort: Send + Sync {
    /// Verifies connectivity and returns server version string.
    async fn test_connection(&self) -> Result<String, DomainError>;

    /// Lists all user databases on the connected instance.
    async fn list_databases(&self) -> Result<Vec<String>, DomainError>;

    /// Creates a new empty database (optionally dropping it first if `drop_if_exists` is true).
    async fn create_database(&self, db_name: &str, drop_if_exists: bool) -> Result<(), DomainError>;

    /// Bootstraps `[__rustdb_instance_meta]` and `[__rustdb_schema_history]` inside `db_name`.
    async fn ensure_tracking_tables(&self, db_name: &str) -> Result<(), DomainError>;

    /// Writes or updates the instance metadata record.
    async fn write_instance_metadata(
        &self,
        db_name: &str,
        meta: &InstanceMetadata,
    ) -> Result<(), DomainError>;

    /// Executes a SQL script (splitting on `GO` batches for SQL Server) inside `db_name`
    /// and records the execution in `[__rustdb_schema_history]`.
    async fn execute_tracked_script(
        &self,
        db_name: &str,
        script_sql: &str,
        record: &AppliedMigrationRecord,
    ) -> Result<(), DomainError>;

    /// Physical clone on the same instance via `BACKUP DATABASE ... WITH COPY_ONLY`
    /// and `RESTORE DATABASE ... WITH MOVE`.
    async fn physical_backup_restore_copy(
        &self,
        source_db: &str,
        target_db: &str,
        backup_dir: Option<&Path>,
    ) -> Result<String, DomainError>;

    /// Inspects the full schema inventory (Tables, Columns, Indexes, FKs, Constraints, Views, SPs, Functions, Triggers)
    /// and tracking history in read-only mode.
    async fn inspect_schema(&self, db_name: &str) -> Result<DatabaseSchemaInventory, DomainError>;
}
