use crate::domain::diff::{DiffKind, DiffSeverity, DriftClassification, ObjectDifference};
use crate::domain::inventory::{ColumnInventory, DatabaseSchemaInventory, TableInventory};
use similar::{ChangeTag, TextDiff};

/// Compares two `DatabaseSchemaInventory` snapshots (Source Baseline vs Target Database)
/// and returns raw structural differences across Tables, Columns, Indexes, Foreign Keys,
/// Check Constraints, Views, Stored Procedures, Functions, and Triggers.
///
/// The resulting differences are initially marked as `UnexplainedManualDrift` and are then
/// enriched by `classifier::classify_differences` using the migration catalog and history.
pub fn compute_raw_schema_differences(
    source: &DatabaseSchemaInventory,
    target: &DatabaseSchemaInventory,
) -> Vec<ObjectDifference> {
    let mut diffs = Vec::new();
    let src_db = &source.database_name;
    let tgt_db = &target.database_name;

    // 1. Compare Tables
    for (table_key, src_table) in &source.tables {
        match target.tables.get(table_key) {
            None => {
                let col_names: Vec<String> =
                    src_table.columns.values().map(|c| c.name.clone()).collect();
                diffs.push(ObjectDifference {
                    object_type: "TABLE".to_string(),
                    object_name: src_table.full_name(),
                    diff_kind: DiffKind::MissingInTarget,
                    classification: DriftClassification::UnexplainedManualDrift,
                    severity: DiffSeverity::High,
                    attributed_migration_id: None,
                    attributed_client_id: None,
                    summary: format!(
                        "[ONLY IN SOURCE '{src_db}'] Table '{}' exists in Source '{src_db}', but is MISSING in Target '{tgt_db}'",
                        src_table.full_name(),
                    ),
                    details: vec![format!(
                        "Present in '{src_db}' ({} columns: {}) | Missing in '{tgt_db}'",
                        src_table.columns.len(),
                        col_names.join(", ")
                    )],
                    unified_diff: None,
                });
            }
            Some(tgt_table) => {
                compare_table_internals(src_db, tgt_db, src_table, tgt_table, &mut diffs);
            }
        }
    }

    // Extra tables in Target that do not exist in Source
    for (table_key, tgt_table) in &target.tables {
        if !source.tables.contains_key(table_key) {
            let col_names: Vec<String> =
                tgt_table.columns.values().map(|c| c.name.clone()).collect();
            diffs.push(ObjectDifference {
                object_type: "TABLE".to_string(),
                object_name: tgt_table.full_name(),
                diff_kind: DiffKind::ExtraInTarget,
                classification: DriftClassification::UnexplainedManualDrift,
                severity: DiffSeverity::High,
                attributed_migration_id: None,
                attributed_client_id: None,
                summary: format!(
                    "[ONLY IN TARGET '{tgt_db}'] Table '{}' exists in Target '{tgt_db}', but does NOT exist in Source '{src_db}'",
                    tgt_table.full_name(),
                ),
                details: vec![format!(
                    "Present in '{tgt_db}' ({} columns: {}) | Not in '{src_db}'",
                    tgt_table.columns.len(),
                    col_names.join(", ")
                )],
                unified_diff: None,
            });
        }
    }

    // 2. Compare Programmable Objects (Stored Procedures, Functions, Views, Triggers)
    for (obj_key, src_obj) in &source.programmable_objects {
        match target.programmable_objects.get(obj_key) {
            None => {
                diffs.push(ObjectDifference {
                    object_type: src_obj.kind.to_string(),
                    object_name: src_obj.full_name(),
                    diff_kind: DiffKind::MissingInTarget,
                    classification: DriftClassification::UnexplainedManualDrift,
                    severity: DiffSeverity::High,
                    attributed_migration_id: None,
                    attributed_client_id: None,
                    summary: format!(
                        "[ONLY IN SOURCE '{src_db}'] {} '{}' exists in Source '{src_db}', but is MISSING in Target '{tgt_db}'",
                        src_obj.kind,
                        src_obj.full_name(),
                    ),
                    details: vec![format!("Present in '{src_db}' (SHA-256: {}) | Missing in '{tgt_db}'", src_obj.semantic_sha256)],
                    unified_diff: Some(src_obj.raw_definition.clone()),
                });
            }
            Some(tgt_obj) => {
                if src_obj.raw_sha256 != tgt_obj.raw_sha256 {
                    let is_semantic_match = src_obj.semantic_sha256 == tgt_obj.semantic_sha256;
                    let udiff = build_unified_text_diff(
                        &src_obj.raw_definition,
                        &tgt_obj.raw_definition,
                        &format!("SOURCE ({src_db})/{}", src_obj.full_name()),
                        &format!("TARGET ({tgt_db})/{}", tgt_obj.full_name()),
                    );

                    if is_semantic_match {
                        diffs.push(ObjectDifference {
                            object_type: src_obj.kind.to_string(),
                            object_name: src_obj.full_name(),
                            diff_kind: DiffKind::ModifiedDefinition,
                            classification: DriftClassification::FormattingOnly,
                            severity: DiffSeverity::Info,
                            attributed_migration_id: None,
                            attributed_client_id: None,
                            summary: format!(
                                "[BOTH DBs - FORMATTING ONLY] {} '{}' has comment/whitespace differences between '{src_db}' and '{tgt_db}'",
                                src_obj.kind,
                                src_obj.full_name()
                            ),
                            details: vec![format!(
                                "Executable T-SQL matches in both '{src_db}' and '{tgt_db}' (SHA-256: {})",
                                src_obj.semantic_sha256
                            )],
                            unified_diff: Some(udiff),
                        });
                    } else {
                        diffs.push(ObjectDifference {
                            object_type: src_obj.kind.to_string(),
                            object_name: src_obj.full_name(),
                            diff_kind: DiffKind::ModifiedDefinition,
                            classification: DriftClassification::UnexplainedManualDrift,
                            severity: DiffSeverity::High,
                            attributed_migration_id: None,
                            attributed_client_id: None,
                            summary: format!(
                                "[MODIFIED IN BOTH] {} '{}' exists in both '{src_db}' and '{tgt_db}', but its SQL code differs",
                                src_obj.kind,
                                src_obj.full_name(),
                            ),
                            details: vec![
                                format!("Lines with '-' belong to SOURCE '{src_db}' (SHA-256: {})", &src_obj.semantic_sha256[..16]),
                                format!("Lines with '+' belong to TARGET '{tgt_db}' (SHA-256: {})", &tgt_obj.semantic_sha256[..16]),
                            ],
                            unified_diff: Some(udiff),
                        });
                    }
                }
            }
        }
    }

    // Extra Programmable Objects in Target
    for (obj_key, tgt_obj) in &target.programmable_objects {
        if !source.programmable_objects.contains_key(obj_key) {
            diffs.push(ObjectDifference {
                object_type: tgt_obj.kind.to_string(),
                object_name: tgt_obj.full_name(),
                diff_kind: DiffKind::ExtraInTarget,
                classification: DriftClassification::UnexplainedManualDrift,
                severity: DiffSeverity::High,
                attributed_migration_id: None,
                attributed_client_id: None,
                summary: format!(
                    "[ONLY IN TARGET '{tgt_db}'] {} '{}' exists in Target '{tgt_db}', but does NOT exist in Source '{src_db}'",
                    tgt_obj.kind,
                    tgt_obj.full_name(),
                ),
                details: vec![format!("Present in '{tgt_db}' (SHA-256: {}) | Not in '{src_db}'", tgt_obj.semantic_sha256)],
                unified_diff: Some(tgt_obj.raw_definition.clone()),
            });
        }
    }

    diffs
}

fn compare_table_internals(
    src_db: &str,
    tgt_db: &str,
    src: &TableInventory,
    tgt: &TableInventory,
    diffs: &mut Vec<ObjectDifference>,
) {
    let mut structural_notes = Vec::new();

    // A. Columns
    for (col_key, src_col) in &src.columns {
        match tgt.columns.get(col_key) {
            None => {
                structural_notes.push(format!(
                    "[ONLY IN SOURCE '{src_db}' | MISSING IN TARGET '{tgt_db}'] Column [{}] ({})",
                    src_col.name,
                    format_column_type(src_col)
                ));
            }
            Some(tgt_col) => {
                let col_diffs = compare_columns(src_db, tgt_db, src_col, tgt_col);
                structural_notes.extend(col_diffs);
            }
        }
    }

    for (col_key, tgt_col) in &tgt.columns {
        if !src.columns.contains_key(col_key) {
            structural_notes.push(format!(
                "[ONLY IN TARGET '{tgt_db}' | NOT IN SOURCE '{src_db}'] Column [{}] ({})",
                tgt_col.name,
                format_column_type(tgt_col)
            ));
        }
    }

    // B. Indexes & Primary Keys
    for (idx_key, src_idx) in &src.indexes {
        match tgt.indexes.get(idx_key) {
            None => {
                structural_notes.push(format!(
                    "[ONLY IN SOURCE '{src_db}' | MISSING IN TARGET '{tgt_db}'] Index [{}] (keys: {})",
                    src_idx.name,
                    src_idx.key_columns.join(", ")
                ));
            }
            Some(tgt_idx) => {
                if src_idx != tgt_idx {
                    structural_notes.push(format!(
                        "[DIFFERENT IN BOTH] Index [{}]: in '{src_db}' keys=({:?}, unique={}) vs in '{tgt_db}' keys=({:?}, unique={})",
                        src_idx.name,
                        src_idx.key_columns,
                        src_idx.is_unique,
                        tgt_idx.key_columns,
                        tgt_idx.is_unique
                    ));
                }
            }
        }
    }
    for (idx_key, tgt_idx) in &tgt.indexes {
        if !src.indexes.contains_key(idx_key) {
            structural_notes.push(format!(
                "[ONLY IN TARGET '{tgt_db}' | NOT IN SOURCE '{src_db}'] Index [{}] (keys: {})",
                tgt_idx.name,
                tgt_idx.key_columns.join(", ")
            ));
        }
    }

    // C. Foreign Keys
    for (fk_key, src_fk) in &src.foreign_keys {
        match tgt.foreign_keys.get(fk_key) {
            None => {
                structural_notes.push(format!(
                    "[ONLY IN SOURCE '{src_db}' | MISSING IN TARGET '{tgt_db}'] Foreign Key [{}] -> {}",
                    src_fk.name, src_fk.referenced_table
                ));
            }
            Some(tgt_fk) => {
                if src_fk != tgt_fk {
                    structural_notes.push(format!(
                        "[DIFFERENT IN BOTH] Foreign Key [{}]: in '{src_db}'={:?} vs in '{tgt_db}'={:?}",
                        src_fk.name, src_fk.columns, tgt_fk.columns
                    ));
                }
            }
        }
    }
    for (fk_key, tgt_fk) in &tgt.foreign_keys {
        if !src.foreign_keys.contains_key(fk_key) {
            structural_notes.push(format!(
                "[ONLY IN TARGET '{tgt_db}' | NOT IN SOURCE '{src_db}'] Foreign Key [{}] -> {}",
                tgt_fk.name, tgt_fk.referenced_table
            ));
        }
    }

    // D. Check Constraints
    for (cc_key, src_cc) in &src.check_constraints {
        match tgt.check_constraints.get(cc_key) {
            None => structural_notes.push(format!(
                "[ONLY IN SOURCE '{src_db}' | MISSING IN TARGET '{tgt_db}'] Check Constraint [{}]",
                src_cc.name
            )),
            Some(tgt_cc) if src_cc.definition != tgt_cc.definition => {
                structural_notes.push(format!(
                    "[DIFFERENT IN BOTH] Check Constraint [{}]: in '{src_db}'='{}' vs in '{tgt_db}'='{}'",
                    src_cc.name, src_cc.definition, tgt_cc.definition
                ));
            }
            _ => {}
        }
    }
    for (cc_key, tgt_cc) in &tgt.check_constraints {
        if !src.check_constraints.contains_key(cc_key) {
            structural_notes.push(format!(
                "[ONLY IN TARGET '{tgt_db}' | NOT IN SOURCE '{src_db}'] Check Constraint [{}]: {}",
                tgt_cc.name, tgt_cc.definition
            ));
        }
    }

    if !structural_notes.is_empty() {
        diffs.push(ObjectDifference {
            object_type: "TABLE".to_string(),
            object_name: src.full_name(),
            diff_kind: DiffKind::ModifiedStructure,
            classification: DriftClassification::UnexplainedManualDrift,
            severity: DiffSeverity::High,
            attributed_migration_id: None,
            attributed_client_id: None,
            summary: format!(
                "Table '{}' exists in both '{src_db}' and '{tgt_db}', with {} difference(s)",
                src.full_name(),
                structural_notes.len()
            ),
            details: structural_notes,
            unified_diff: None,
        });
    }
}

fn compare_columns(
    src_db: &str,
    tgt_db: &str,
    src: &ColumnInventory,
    tgt: &ColumnInventory,
) -> Vec<String> {
    let mut notes = Vec::new();
    if !src.data_type.eq_ignore_ascii_case(&tgt.data_type)
        || src.max_length != tgt.max_length
        || src.precision != tgt.precision
        || src.scale != tgt.scale
    {
        notes.push(format!(
            "[DIFFERENT TYPE] Column [{}]: in '{src_db}' is {} vs in '{tgt_db}' is {}",
            src.name,
            format_column_type(src),
            format_column_type(tgt)
        ));
    }
    if src.is_nullable != tgt.is_nullable {
        notes.push(format!(
            "[DIFFERENT NULLABILITY] Column [{}]: in '{src_db}' is_nullable={} vs in '{tgt_db}' is_nullable={}",
            src.name, src.is_nullable, tgt.is_nullable
        ));
    }
    if src.default_definition != tgt.default_definition {
        notes.push(format!(
            "[DIFFERENT DEFAULT] Column [{}]: in '{src_db}' default={:?} vs in '{tgt_db}' default={:?}",
            src.name, src.default_definition, tgt.default_definition
        ));
    }
    notes
}

fn format_column_type(col: &ColumnInventory) -> String {
    format!(
        "{}(len={}, prec={}, scale={}, null={})",
        col.data_type, col.max_length, col.precision, col.scale, col.is_nullable
    )
}

fn build_unified_text_diff(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    let old_norm = old.replace("\r\n", "\n");
    let new_norm = new.replace("\r\n", "\n");
    let diff = TextDiff::from_lines(&old_norm, &new_norm);
    let mut out = format!("--- {old_label}\n+++ {new_label}\n");
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        out.push_str(sign);
        out.push_str(change.value());
        if !change.value().ends_with('\n') {
            out.push('\n');
        }
    }
    out
}
