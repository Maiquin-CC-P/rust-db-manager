use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiffKind {
    MissingInTarget,
    ExtraInTarget,
    ModifiedDefinition,
    ModifiedStructure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DriftClassification {
    /// Diff is explained by an authorized migration belonging to the target client or product update
    ExplainedByMigration,
    /// Object or modification belongs to a migration scoped to a DIFFERENT client!
    CrossClientContamination,
    /// Object or modification is not present in any registered migration or history
    UnexplainedManualDrift,
    /// Target database is missing an object that belongs to a pending known migration
    PendingKnownMigration,
    /// Formatting/comment-only difference (semantic AST/tokens match)
    FormattingOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiffSeverity {
    Info,
    Notice,
    Warning,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectDifference {
    pub object_type: String,
    pub object_name: String,
    pub diff_kind: DiffKind,
    pub classification: DriftClassification,
    pub severity: DiffSeverity,
    pub attributed_migration_id: Option<String>,
    pub attributed_client_id: Option<String>,
    pub summary: String,
    pub details: Vec<String>,
    pub unified_diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub total_differences: usize,
    pub explained_changes: usize,
    pub cross_client_contaminations: usize,
    pub unexplained_manual_drifts: usize,
    pub pending_migrations: usize,
    pub formatting_only_diffs: usize,
    pub is_clean_for_deployment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub source_database: String,
    pub target_database: String,
    pub target_client_id: Option<String>,
    pub compared_at_utc: DateTime<Utc>,
    pub comparison_engine: String,
    pub summary: ComparisonSummary,
    pub differences: Vec<ObjectDifference>,
}
