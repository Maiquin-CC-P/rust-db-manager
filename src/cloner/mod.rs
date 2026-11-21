use crate::baseline::{build_database_from_baseline, CatalogRegistry};
use crate::comparator::sqlpackage_bridge::SqlPackageBridge;
use crate::config::ConnectionProfile;
use crate::domain::error::DomainError;
use crate::domain::inventory::InstanceMetadata;
use crate::ports::{CopyMode, DatabaseEnginePort};
use chrono::Utc;
use std::path::Path;
use uuid::Uuid;

pub struct CopyRequest<'a> {
    pub mode: CopyMode,
    pub source_db: Option<&'a str>,
    pub target_db: &'a str,
    pub catalog: Option<&'a CatalogRegistry>,
    pub product_id: Option<&'a str>,
    pub baseline_version: Option<&'a str>,
    pub client_id: Option<&'a str>,
    pub backup_dir: Option<&'a Path>,
    pub drop_if_exists: bool,
    pub allow_destructive: bool,
}

pub struct CopyReceipt {
    pub target_database: String,
    pub mode_used: String,
    pub provenance: String,
    pub artifact_path: Option<String>,
}

/// Executes a Database Copy operation using the selected modality:
/// 1. `CopyMode::Rebuild`: Clean deterministic rebuild from scripts (Baseline + Client Overlay).
/// 2. `CopyMode::BackupRestore`: Physical SQL Server `BACKUP ... WITH COPY_ONLY` + `RESTORE ... WITH MOVE`.
/// 3. `CopyMode::SchemaClone`: Live schema-only clone via `SqlPackage` DACPAC extraction and publish.
pub async fn copy_database(
    engine: &dyn DatabaseEnginePort,
    profile: &ConnectionProfile,
    req: CopyRequest<'_>,
) -> Result<CopyReceipt, DomainError> {
    match req.mode {
        CopyMode::Rebuild => {
            let catalog = req.catalog.ok_or_else(|| {
                DomainError::DatabaseError(
                    "Catalog path (--catalog) is required for '--mode rebuild'".to_string(),
                )
            })?;
            let product = req.product_id.ok_or_else(|| {
                DomainError::DatabaseError(
                    "Product ID (--product) is required for '--mode rebuild'".to_string(),
                )
            })?;
            let version = req.baseline_version.ok_or_else(|| {
                DomainError::DatabaseError(
                    "Baseline version (--version) is required for '--mode rebuild'".to_string(),
                )
            })?;

            let records = build_database_from_baseline(
                engine,
                catalog,
                product,
                version,
                req.client_id,
                req.target_db,
                req.drop_if_exists,
                req.allow_destructive,
            )
            .await?;

            Ok(CopyReceipt {
                target_database: req.target_db.to_string(),
                mode_used: "SCRIPT_REBUILD".to_string(),
                provenance: format!(
                    "Rebuilt from {}@{} (client: {}) with {} tracked scripts",
                    product,
                    version,
                    req.client_id.unwrap_or("none"),
                    records.len()
                ),
                artifact_path: None,
            })
        }
        CopyMode::BackupRestore => {
            let source_db = req.source_db.ok_or_else(|| {
                DomainError::DatabaseError(
                    "Source database (--source) is required for '--mode backup-restore'"
                        .to_string(),
                )
            })?;

            let backup_file = engine
                .physical_backup_restore_copy(source_db, req.target_db, req.backup_dir)
                .await?;

            // Stamp metadata in the cloned database so it is never confused with a clean baseline
            let meta = InstanceMetadata {
                instance_uid: Uuid::new_v4().to_string(),
                product_id: req.product_id.unwrap_or("CLONED_INSTANCE").to_string(),
                baseline_version: req.baseline_version.unwrap_or("UNKNOWN").to_string(),
                client_id: req.client_id.map(|s| s.to_string()),
                environment: "LOCAL_CLONE".to_string(),
                origin_mode: "BACKUP_RESTORE".to_string(),
                source_provenance: Some(format!("physical_clone_of:{source_db}")),
                trust_level: "CLONED_PHYSICAL_INSTANCE".to_string(),
                created_at_utc: Utc::now().to_rfc3339(),
            };
            engine.write_instance_metadata(req.target_db, &meta).await?;

            Ok(CopyReceipt {
                target_database: req.target_db.to_string(),
                mode_used: "BACKUP_RESTORE".to_string(),
                provenance: format!("Physical COPY_ONLY backup & restore from '{source_db}'"),
                artifact_path: Some(backup_file),
            })
        }
        CopyMode::SchemaClone => {
            let source_db = req.source_db.ok_or_else(|| {
                DomainError::DatabaseError(
                    "Source database (--source) is required for '--mode schema-clone'".to_string(),
                )
            })?;

            let bridge = SqlPackageBridge::detect().ok_or_else(|| {
                DomainError::DatabaseError(
                    "SqlPackage CLI was not found in PATH. Install it via 'dotnet tool install -g microsoft.sqlpackage' or use '--mode rebuild' / '--mode backup-restore'.".to_string(),
                )
            })?;

            let dacpac_path = bridge
                .clone_schema_via_dacpac(profile, source_db, req.target_db)
                .await?;

            let meta = InstanceMetadata {
                instance_uid: Uuid::new_v4().to_string(),
                product_id: req.product_id.unwrap_or("DACPAC_CLONE").to_string(),
                baseline_version: req.baseline_version.unwrap_or("UNKNOWN").to_string(),
                client_id: req.client_id.map(|s| s.to_string()),
                environment: "LOCAL_SCHEMA_CLONE".to_string(),
                origin_mode: "DACPAC_CLONE".to_string(),
                source_provenance: Some(format!("dacpac_schema_of:{source_db}")),
                trust_level: "SCHEMA_ONLY_CLONE".to_string(),
                created_at_utc: Utc::now().to_rfc3339(),
            };
            engine.write_instance_metadata(req.target_db, &meta).await?;

            Ok(CopyReceipt {
                target_database: req.target_db.to_string(),
                mode_used: "DACPAC_SCHEMA_CLONE".to_string(),
                provenance: format!("Schema-only DACPAC clone from '{source_db}'"),
                artifact_path: Some(dacpac_path.display().to_string()),
            })
        }
    }
}
