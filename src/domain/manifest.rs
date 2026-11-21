use crate::domain::error::DomainError;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    SqlServer,
    MySql,
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineKind::SqlServer => write!(f, "SQLSERVER"),
            EngineKind::MySql => write!(f, "MYSQL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineState {
    Draft,
    Building,
    PendingValidation,
    Validated,
    ValidationFailed,
    Deprecated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasterDataPolicy {
    SystemOwned,
    ClientExtensible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEntry {
    pub path: String,
    pub sha256: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub master_data_policy: Option<MasterDataPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineManifest {
    pub product_id: String,
    pub version: String,
    pub engine: EngineKind,
    pub state: BaselineState,
    pub description: String,
    pub schema_scripts: Vec<ScriptEntry>,
    pub master_data_scripts: Vec<ScriptEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationScope {
    Product,
    Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub migration_id: String,
    pub change_id: String,
    pub product_id: String,
    pub description: String,
    pub engine: EngineKind,
    pub scope: MigrationScope,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub up_scripts: Vec<ScriptEntry>,
    #[serde(default)]
    pub down_scripts: Vec<ScriptEntry>,
    #[serde(default)]
    pub affected_objects: Vec<String>,
    #[serde(default)]
    pub destructive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEnvironmentConfig {
    pub environment: String,
    pub database_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientManifest {
    pub client_id: String,
    pub client_name: String,
    pub product_id: String,
    pub baseline_version: String,
    #[serde(default)]
    pub approved_product_migrations: Vec<String>,
    #[serde(default)]
    pub approved_client_migrations: Vec<String>,
    #[serde(default)]
    pub environments: Vec<ClientEnvironmentConfig>,
}

/// Computes the deterministic SHA-256 checksum of a file after normalizing CRLF -> LF.
pub fn compute_normalized_file_sha256(path: &Path) -> Result<String, DomainError> {
    let raw = fs::read_to_string(path)
        .map_err(|_| DomainError::ScriptNotFound(path.display().to_string()))?;
    Ok(compute_normalized_str_sha256(&raw))
}

/// Computes the deterministic SHA-256 checksum of a string after normalizing CRLF -> LF.
pub fn compute_normalized_str_sha256(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

impl BaselineManifest {
    pub fn load_from_dir(baseline_dir: &Path) -> Result<Self, DomainError> {
        let manifest_path = baseline_dir.join("manifest.json");
        let content = fs::read_to_string(&manifest_path)
            .map_err(|_| DomainError::ManifestNotFound(manifest_path.display().to_string()))?;
        serde_json::from_str(&content)
            .map_err(|e| DomainError::DatabaseError(format!("Invalid baseline manifest JSON: {e}")))
    }

    /// Validates that every schema and master-data script exists and matches its declared SHA-256 (if provided).
    pub fn validate_scripts(&self, baseline_dir: &Path) -> Result<Vec<(PathBuf, String)>, DomainError> {
        let mut verified = Vec::new();
        for entry in self.schema_scripts.iter().chain(self.master_data_scripts.iter()) {
            let full_path = baseline_dir.join(&entry.path);
            let actual_hash = compute_normalized_file_sha256(&full_path)?;
            if let Some(expected) = &entry.sha256 {
                if !expected.eq_ignore_ascii_case(&actual_hash) {
                    return Err(DomainError::ChecksumMismatch {
                        path: entry.path.clone(),
                        expected: expected.clone(),
                        actual: actual_hash,
                    });
                }
            }
            verified.push((full_path, actual_hash));
        }
        Ok(verified)
    }
}

/// Validates and topologically sorts migrations for a target product and optional client.
/// Strictly blocks cross-client contamination.
pub fn resolve_migration_plan(
    target_product: &str,
    target_client: Option<&str>,
    available_migrations: &[MigrationManifest],
    required_migration_ids: &[String],
    allow_destructive: bool,
) -> Result<Vec<MigrationManifest>, DomainError> {
    let by_id: HashMap<&str, &MigrationManifest> = available_migrations
        .iter()
        .map(|m| (m.migration_id.as_str(), m))
        .collect();

    // Collect transitive closure of required migrations and verify scope/product rules
    let mut selected_ids: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = required_migration_ids.to_vec();

    while let Some(curr_id) = stack.pop() {
        if !selected_ids.insert(curr_id.clone()) {
            continue;
        }
        let mig = by_id.get(curr_id.as_str()).ok_or_else(|| DomainError::MissingDependency {
            migration_id: "target_plan".to_string(),
            missing_dep: curr_id.clone(),
        })?;

        // 1. Product guard
        if !mig.product_id.eq_ignore_ascii_case(target_product) {
            return Err(DomainError::ProductMismatch {
                migration_id: mig.migration_id.clone(),
                migration_product: mig.product_id.clone(),
                target_product: target_product.to_string(),
            });
        }

        // 2. Client isolation guard (CRITICAL: never allow Client B migration into Client A or pure baseline)
        if mig.scope == MigrationScope::Client {
            let mig_client = mig.client_id.as_deref().unwrap_or("<unspecified>");
            match target_client {
                Some(tc) if tc.eq_ignore_ascii_case(mig_client) => {}
                Some(tc) => {
                    return Err(DomainError::CrossClientContaminationBlocked {
                        migration_id: mig.migration_id.clone(),
                        migration_client: mig_client.to_string(),
                        target_client: tc.to_string(),
                    });
                }
                None => {
                    return Err(DomainError::CrossClientContaminationBlocked {
                        migration_id: mig.migration_id.clone(),
                        migration_client: mig_client.to_string(),
                        target_client: "PRODUCT_BASELINE (no client)".to_string(),
                    });
                }
            }
        }

        // 3. Destructive guard
        if mig.destructive && !allow_destructive {
            return Err(DomainError::DestructiveMigrationNotAllowed(
                mig.migration_id.clone(),
            ));
        }

        // 4. Queue dependencies
        for dep in &mig.dependencies {
            if !by_id.contains_key(dep.as_str()) {
                return Err(DomainError::MissingDependency {
                    migration_id: mig.migration_id.clone(),
                    missing_dep: dep.clone(),
                });
            }
            stack.push(dep.clone());
        }
    }

    // Build DAG for topological sorting
    let mut graph = DiGraph::<String, ()>::new();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    // Sort IDs deterministically before inserting nodes
    let mut sorted_selected: Vec<String> = selected_ids.into_iter().collect();
    sorted_selected.sort();

    for id in &sorted_selected {
        let idx = graph.add_node(id.clone());
        node_map.insert(id.clone(), idx);
    }

    for id in &sorted_selected {
        let mig = by_id[id.as_str()];
        let target_idx = node_map[id];
        for dep in &mig.dependencies {
            let dep_idx = node_map[dep];
            // Edge from dependency -> dependent
            graph.add_edge(dep_idx, target_idx, ());
        }
    }

    let sorted_indices = toposort(&graph, None).map_err(|cycle| {
        let cycle_node = &graph[cycle.node_id()];
        DomainError::DependencyCycle(cycle_node.clone())
    })?;

    let ordered_plan = sorted_indices
        .into_iter()
        .map(|idx| {
            let id = &graph[idx];
            by_id[id.as_str()].clone()
        })
        .collect();

    Ok(ordered_plan)
}
