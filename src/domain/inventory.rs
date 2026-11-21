use crate::domain::manifest::EngineKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceMetadata {
    pub instance_uid: String,
    pub product_id: String,
    pub baseline_version: String,
    pub client_id: Option<String>,
    pub environment: String,
    pub origin_mode: String,
    pub source_provenance: Option<String>,
    pub trust_level: String,
    pub created_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppliedMigrationRecord {
    pub migration_id: String,
    pub change_control_id: Option<String>,
    pub scope: String,
    pub client_id: Option<String>,
    pub script_path: String,
    pub checksum_sha256: String,
    pub executed_by: String,
    pub executed_at_utc: String,
    pub execution_ms: i64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnInventory {
    pub name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub max_length: i16,
    pub precision: u8,
    pub scale: u8,
    pub is_nullable: bool,
    pub is_identity: bool,
    pub default_definition: Option<String>,
    pub collation_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexInventory {
    pub name: String,
    pub index_type: String,
    pub is_unique: bool,
    pub is_primary_key: bool,
    pub key_columns: Vec<String>,
    pub included_columns: Vec<String>,
    pub filter_definition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForeignKeyInventory {
    pub name: String,
    pub parent_table: String,
    pub referenced_table: String,
    pub columns: Vec<(String, String)>, // (parent_col, referenced_col)
    pub delete_referential_action: String,
    pub update_referential_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckConstraintInventory {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableInventory {
    pub schema_name: String,
    pub table_name: String,
    pub columns: BTreeMap<String, ColumnInventory>,
    pub indexes: BTreeMap<String, IndexInventory>,
    pub foreign_keys: BTreeMap<String, ForeignKeyInventory>,
    pub check_constraints: BTreeMap<String, CheckConstraintInventory>,
}

impl TableInventory {
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.schema_name, self.table_name)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgrammableObjectKind {
    StoredProcedure,
    ScalarFunction,
    TableFunction,
    View,
    Trigger,
}

impl std::fmt::Display for ProgrammableObjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgrammableObjectKind::StoredProcedure => write!(f, "STORED_PROCEDURE"),
            ProgrammableObjectKind::ScalarFunction => write!(f, "SCALAR_FUNCTION"),
            ProgrammableObjectKind::TableFunction => write!(f, "TABLE_FUNCTION"),
            ProgrammableObjectKind::View => write!(f, "VIEW"),
            ProgrammableObjectKind::Trigger => write!(f, "TRIGGER"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgrammableObjectInventory {
    pub schema_name: String,
    pub object_name: String,
    pub kind: ProgrammableObjectKind,
    pub raw_definition: String,
    pub raw_sha256: String,
    pub semantic_sha256: String,
}

impl ProgrammableObjectInventory {
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.schema_name, self.object_name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSchemaInventory {
    pub database_name: String,
    pub engine: EngineKind,
    pub inspected_at_utc: DateTime<Utc>,
    pub instance_meta: Option<InstanceMetadata>,
    pub applied_migrations: Vec<AppliedMigrationRecord>,
    pub schemas: Vec<String>,
    /// Keyed by lowercase `schema.table_name`
    pub tables: BTreeMap<String, TableInventory>,
    /// Keyed by lowercase `schema.object_name`
    pub programmable_objects: BTreeMap<String, ProgrammableObjectInventory>,
}
