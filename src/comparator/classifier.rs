use crate::comparator::structural_differ::compute_raw_schema_differences;
use crate::domain::diff::{
    ComparisonReport, ComparisonSummary, DiffKind, DiffSeverity, DriftClassification,
};
use crate::domain::inventory::DatabaseSchemaInventory;
use crate::domain::manifest::{MigrationManifest, MigrationScope};
use chrono::Utc;
use std::collections::HashSet;

/// Compares `source` (typically `PROD_BASE` or a reconstructed baseline) against `target`
/// (e.g., `CLIENTE_A_DB`) and classifies every structural difference using the registered
/// migration catalog and the target instance's applied migration history:
///
/// - `ExplainedByMigration`: Change matches an authorized migration for this client/product.
/// - `CrossClientContamination`: Change matches a migration belonging to ANOTHER client (`CLIENTE_B`)!
/// - `PendingKnownMigration`: Object is missing in target because a valid migration hasn't been applied yet.
/// - `UnexplainedManualDrift`: Undocumented change applied manually in the database.
pub fn compare_and_classify(
    source: &DatabaseSchemaInventory,
    target: &DatabaseSchemaInventory,
    target_client_id: Option<&str>,
    all_product_migrations: &[MigrationManifest],
    comparison_engine_label: &str,
) -> ComparisonReport {
    let mut differences = compute_raw_schema_differences(source, target);

    let applied_ids_in_target: HashSet<String> = target
        .applied_migrations
        .iter()
        .filter(|r| r.success)
        .map(|r| r.migration_id.to_lowercase())
        .collect();

    for diff in &mut differences {
        if diff.classification == DriftClassification::FormattingOnly {
            continue;
        }

        let norm_obj_name = normalize_object_identifier(&diff.object_name);

        // Find all registered migrations in the catalog that declare affecting this object
        let matching_migrations: Vec<&MigrationManifest> = all_product_migrations
            .iter()
            .filter(|m| {
                m.affected_objects
                    .iter()
                    .any(|ao| normalize_object_identifier(ao) == norm_obj_name)
            })
            .collect();

        if matching_migrations.is_empty() {
            // No migration in the entire repository touches this object -> Undocumented Manual Drift
            diff.classification = DriftClassification::UnexplainedManualDrift;
            diff.severity = DiffSeverity::High;
            diff.details.push(
                "DRIFT ALERT: No migration in the catalog declares affecting this object. Likely manual modification in DB.".to_string(),
            );
            continue;
        }

        // Check if any matching migration belongs to a DIFFERENT client (Cross-Client Contamination!)
        let mut classified = false;
        for mig in &matching_migrations {
            match mig.scope {
                MigrationScope::Client => {
                    let mig_client = mig.client_id.as_deref().unwrap_or("");
                    let is_for_target_client = target_client_id
                        .map(|tc| tc.eq_ignore_ascii_case(mig_client))
                        .unwrap_or(false);

                    if !is_for_target_client {
                        // CRITICAL: An object from Client B is present in Client A (or in pure baseline)!
                        diff.classification = DriftClassification::CrossClientContamination;
                        diff.severity = DiffSeverity::Critical;
                        diff.attributed_migration_id = Some(mig.migration_id.clone());
                        diff.attributed_client_id = mig.client_id.clone();
                        diff.details.push(format!(
                            "CONTAMINATION DETECTED: Object belongs to migration '{}' ({}) scoped exclusively to client '{}', which is invalid for target client '{}'!",
                            mig.migration_id,
                            mig.change_id,
                            mig_client,
                            target_client_id.unwrap_or("BASELINE")
                        ));
                        classified = true;
                        break;
                    } else {
                        // Migration belongs to the target client
                        if diff.diff_kind == DiffKind::MissingInTarget
                            && !applied_ids_in_target.contains(&mig.migration_id.to_lowercase())
                        {
                            diff.classification = DriftClassification::PendingKnownMigration;
                            diff.severity = DiffSeverity::Notice;
                            diff.attributed_migration_id = Some(mig.migration_id.clone());
                            diff.attributed_client_id = mig.client_id.clone();
                            diff.details.push(format!(
                                "Pending client migration '{}' ({})",
                                mig.migration_id, mig.change_id
                            ));
                        } else {
                            diff.classification = DriftClassification::ExplainedByMigration;
                            diff.severity = DiffSeverity::Info;
                            diff.attributed_migration_id = Some(mig.migration_id.clone());
                            diff.attributed_client_id = mig.client_id.clone();
                            diff.details.push(format!(
                                "Explained by authorized client migration '{}' ({}): {}",
                                mig.migration_id, mig.change_id, mig.description
                            ));
                        }
                        classified = true;
                        break;
                    }
                }
                MigrationScope::Product => {
                    if diff.diff_kind == DiffKind::MissingInTarget
                        && !applied_ids_in_target.contains(&mig.migration_id.to_lowercase())
                    {
                        diff.classification = DriftClassification::PendingKnownMigration;
                        diff.severity = DiffSeverity::Notice;
                        diff.attributed_migration_id = Some(mig.migration_id.clone());
                        diff.details.push(format!(
                            "Pending common product migration '{}' ({})",
                            mig.migration_id, mig.change_id
                        ));
                    } else {
                        diff.classification = DriftClassification::ExplainedByMigration;
                        diff.severity = DiffSeverity::Info;
                        diff.attributed_migration_id = Some(mig.migration_id.clone());
                        diff.details.push(format!(
                            "Explained by common product migration '{}' ({}): {}",
                            mig.migration_id, mig.change_id, mig.description
                        ));
                    }
                    classified = true;
                }
            }
        }

        if !classified {
            diff.classification = DriftClassification::UnexplainedManualDrift;
            diff.severity = DiffSeverity::High;
        }
    }

    // Sort differences so Critical / High appear first
    differences.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.object_name.cmp(&b.object_name))
    });

    let explained_changes = differences
        .iter()
        .filter(|d| d.classification == DriftClassification::ExplainedByMigration)
        .count();
    let cross_client_contaminations = differences
        .iter()
        .filter(|d| d.classification == DriftClassification::CrossClientContamination)
        .count();
    let unexplained_manual_drifts = differences
        .iter()
        .filter(|d| d.classification == DriftClassification::UnexplainedManualDrift)
        .count();
    let pending_migrations = differences
        .iter()
        .filter(|d| d.classification == DriftClassification::PendingKnownMigration)
        .count();
    let formatting_only_diffs = differences
        .iter()
        .filter(|d| d.classification == DriftClassification::FormattingOnly)
        .count();

    let is_clean_for_deployment =
        cross_client_contaminations == 0 && unexplained_manual_drifts == 0;

    ComparisonReport {
        source_database: source.database_name.clone(),
        target_database: target.database_name.clone(),
        target_client_id: target_client_id.map(|s| s.to_string()),
        compared_at_utc: Utc::now(),
        comparison_engine: comparison_engine_label.to_string(),
        summary: ComparisonSummary {
            total_differences: differences.len(),
            explained_changes,
            cross_client_contaminations,
            unexplained_manual_drifts,
            pending_migrations,
            formatting_only_diffs,
            is_clean_for_deployment,
        },
        differences,
    }
}

fn normalize_object_identifier(ident: &str) -> String {
    let cleaned = ident.replace(['[', ']'], "").trim().to_lowercase();
    if cleaned.contains('.') {
        cleaned
    } else {
        format!("dbo.{cleaned}")
    }
}
