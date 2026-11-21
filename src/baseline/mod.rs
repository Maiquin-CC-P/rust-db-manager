use crate::domain::error::DomainError;
use crate::domain::inventory::{AppliedMigrationRecord, InstanceMetadata};
use crate::domain::manifest::{
    compute_normalized_file_sha256, resolve_migration_plan, BaselineManifest, ClientManifest,
    MigrationManifest,
};
use crate::ports::DatabaseEnginePort;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct CatalogRegistry {
    pub catalog_root: PathBuf,
}

impl CatalogRegistry {
    pub fn new(catalog_root: impl Into<PathBuf>) -> Self {
        Self {
            catalog_root: catalog_root.into(),
        }
    }

    pub fn baseline_dir(&self, product_id: &str, version: &str) -> PathBuf {
        self.catalog_root
            .join("products")
            .join(product_id)
            .join("baselines")
            .join(version)
    }

    pub fn load_baseline(
        &self,
        product_id: &str,
        version: &str,
    ) -> Result<(BaselineManifest, PathBuf), DomainError> {
        let dir = self.baseline_dir(product_id, version);
        let manifest = BaselineManifest::load_from_dir(&dir)?;
        Ok((manifest, dir))
    }

    pub fn load_client(&self, client_id: &str) -> Result<(ClientManifest, PathBuf), DomainError> {
        let dir = self.catalog_root.join("clients").join(client_id);
        let manifest_path = dir.join("client.json");
        let content = fs::read_to_string(&manifest_path)
            .map_err(|_| DomainError::ManifestNotFound(manifest_path.display().to_string()))?;
        let manifest: ClientManifest = serde_json::from_str(&content)
            .map_err(|e| DomainError::DatabaseError(format!("Invalid client.json: {e}")))?;
        Ok((manifest, dir))
    }

    /// Loads all migration manifests (along with their base directory) across a product and all clients.
    pub fn load_all_migrations_for_product(
        &self,
        product_id: &str,
    ) -> Result<Vec<(MigrationManifest, PathBuf)>, DomainError> {
        let mut results = Vec::new();

        // 1. Product-scoped migrations
        let prod_mig_root = self
            .catalog_root
            .join("products")
            .join(product_id)
            .join("migrations");
        if prod_mig_root.exists() {
            collect_migrations_in_dir(&prod_mig_root, &mut results)?;
        }

        // 2. Client-scoped migrations across all registered clients
        let clients_root = self.catalog_root.join("clients");
        if clients_root.exists() {
            for entry in fs::read_dir(&clients_root)
                .map_err(|e| DomainError::DatabaseError(format!("Failed to read clients dir: {e}")))?
            {
                let entry = entry.map_err(|e| DomainError::DatabaseError(e.to_string()))?;
                let client_mig_dir = entry.path().join("migrations");
                if client_mig_dir.exists() {
                    collect_migrations_in_dir(&client_mig_dir, &mut results)?;
                }
            }
        }

        // Filter to matching product
        results.retain(|(m, _)| m.product_id.eq_ignore_ascii_case(product_id));
        Ok(results)
    }
}

fn collect_migrations_in_dir(
    dir: &Path,
    out: &mut Vec<(MigrationManifest, PathBuf)>,
) -> Result<(), DomainError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| DomainError::DatabaseError(format!("Failed to read '{}': {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let manifest_file = path.join("manifest.json");
            if manifest_file.exists() {
                let raw = fs::read_to_string(&manifest_file).map_err(|_| {
                    DomainError::ManifestNotFound(manifest_file.display().to_string())
                })?;
                let mig: MigrationManifest = serde_json::from_str(&raw).map_err(|e| {
                    DomainError::DatabaseError(format!(
                        "Invalid migration manifest '{}': {e}",
                        manifest_file.display()
                    ))
                })?;
                out.push((mig, path));
            }
        }
    }
    Ok(())
}

/// Validates and builds a clean database instance from a Product Baseline and optional Client Profile.
pub async fn build_database_from_baseline(
    engine: &dyn DatabaseEnginePort,
    registry: &CatalogRegistry,
    product_id: &str,
    version: &str,
    client_id: Option<&str>,
    target_database: &str,
    drop_if_exists: bool,
    allow_destructive: bool,
) -> Result<Vec<AppliedMigrationRecord>, DomainError> {
    let (baseline, baseline_dir) = registry.load_baseline(product_id, version)?;
    let _verified_baseline_scripts = baseline.validate_scripts(&baseline_dir)?;

    // Resolve client overlay if a client_id was requested
    let mut planned_migrations: Vec<(MigrationManifest, PathBuf)> = Vec::new();
    if let Some(cid) = client_id {
        let (client_manifest, _) = registry.load_client(cid)?;
        if !client_manifest.product_id.eq_ignore_ascii_case(product_id) {
            return Err(DomainError::ProductMismatch {
                migration_id: format!("client:{cid}"),
                migration_product: client_manifest.product_id,
                target_product: product_id.to_string(),
            });
        }

        let all_migs_with_paths = registry.load_all_migrations_for_product(product_id)?;
        let all_migs: Vec<MigrationManifest> =
            all_migs_with_paths.iter().map(|(m, _)| m.clone()).collect();

        let mut required_ids = client_manifest.approved_product_migrations.clone();
        required_ids.extend(client_manifest.approved_client_migrations.clone());

        let ordered_migs = resolve_migration_plan(
            product_id,
            Some(&client_manifest.client_id),
            &all_migs,
            &required_ids,
            allow_destructive,
        )?;

        for om in ordered_migs {
            let dir = all_migs_with_paths
                .iter()
                .find(|(m, _)| m.migration_id == om.migration_id)
                .map(|(_, d)| d.clone())
                .unwrap();

            // Pre-validate script existence and hashes before touching the database
            for script in &om.up_scripts {
                let script_path = dir.join(&script.path);
                let actual_hash = compute_normalized_file_sha256(&script_path)?;
                if let Some(expected) = &script.sha256 {
                    if !expected.eq_ignore_ascii_case(&actual_hash) {
                        return Err(DomainError::ChecksumMismatch {
                            path: script_path.display().to_string(),
                            expected: expected.clone(),
                            actual: actual_hash,
                        });
                    }
                }
            }
            planned_migrations.push((om, dir));
        }
    }

    // 1. Create or recreate target database
    engine
        .create_database(target_database, drop_if_exists)
        .await?;

    // 2. Write instance metadata
    let meta = InstanceMetadata {
        instance_uid: Uuid::new_v4().to_string(),
        product_id: baseline.product_id.clone(),
        baseline_version: baseline.version.clone(),
        client_id: client_id.map(|s| s.to_string()),
        environment: "LOCAL_REBUILD".to_string(),
        origin_mode: "SCRIPT_REBUILD".to_string(),
        source_provenance: Some(format!(
            "baseline:{}@{}{}",
            baseline.product_id,
            baseline.version,
            client_id
                .map(|c| format!("+client:{c}"))
                .unwrap_or_default()
        )),
        trust_level: "VALIDATED_REBUILD".to_string(),
        created_at_utc: Utc::now().to_rfc3339(),
    };
    engine
        .write_instance_metadata(target_database, &meta)
        .await?;

    let mut applied_records = Vec::new();

    // 3. Execute Baseline Schema Scripts
    for (idx, entry) in baseline.schema_scripts.iter().enumerate() {
        let full_path = baseline_dir.join(&entry.path);
        let sql = fs::read_to_string(&full_path)
            .map_err(|_| DomainError::ScriptNotFound(full_path.display().to_string()))?;
        let hash = compute_normalized_file_sha256(&full_path)?;

        let record = AppliedMigrationRecord {
            migration_id: format!("BASELINE_{}_SCHEMA_{:03}", baseline.version, idx + 1),
            change_control_id: Some(format!("BASE-{}", baseline.version)),
            scope: "BASELINE_SCHEMA".to_string(),
            client_id: None,
            script_path: entry.path.clone(),
            checksum_sha256: hash,
            executed_by: "rustdb".to_string(),
            executed_at_utc: Utc::now().to_rfc3339(),
            execution_ms: 0,
            success: true,
        };
        engine
            .execute_tracked_script(target_database, &sql, &record)
            .await?;
        applied_records.push(record);
    }

    // 4. Execute Baseline Master-Data Scripts
    for (idx, entry) in baseline.master_data_scripts.iter().enumerate() {
        let full_path = baseline_dir.join(&entry.path);
        let sql = fs::read_to_string(&full_path)
            .map_err(|_| DomainError::ScriptNotFound(full_path.display().to_string()))?;
        let hash = compute_normalized_file_sha256(&full_path)?;

        let record = AppliedMigrationRecord {
            migration_id: format!("BASELINE_{}_MASTERDATA_{:03}", baseline.version, idx + 1),
            change_control_id: Some(format!("BASE-{}", baseline.version)),
            scope: "BASELINE_MASTER_DATA".to_string(),
            client_id: None,
            script_path: entry.path.clone(),
            checksum_sha256: hash,
            executed_by: "rustdb".to_string(),
            executed_at_utc: Utc::now().to_rfc3339(),
            execution_ms: 0,
            success: true,
        };
        engine
            .execute_tracked_script(target_database, &sql, &record)
            .await?;
        applied_records.push(record);
    }

    // 5. Execute Resolved Product & Client Migrations in Topological Order
    for (mig, mig_dir) in planned_migrations {
        for entry in &mig.up_scripts {
            let full_path = mig_dir.join(&entry.path);
            let sql = fs::read_to_string(&full_path)
                .map_err(|_| DomainError::ScriptNotFound(full_path.display().to_string()))?;
            let hash = compute_normalized_file_sha256(&full_path)?;

            let record = AppliedMigrationRecord {
                migration_id: mig.migration_id.clone(),
                change_control_id: Some(mig.change_id.clone()),
                scope: format!("{:?}", mig.scope).to_uppercase(),
                client_id: mig.client_id.clone(),
                script_path: entry.path.clone(),
                checksum_sha256: hash,
                executed_by: "rustdb".to_string(),
                executed_at_utc: Utc::now().to_rfc3339(),
                execution_ms: 0,
                success: true,
            };
            engine
                .execute_tracked_script(target_database, &sql, &record)
                .await?;
            applied_records.push(record);
        }
    }

    Ok(applied_records)
}
