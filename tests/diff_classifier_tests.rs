use chrono::Utc;
use rustdb_manager::baseline::CatalogRegistry;
use rustdb_manager::comparator::classifier::compare_and_classify;
use rustdb_manager::domain::diff::DriftClassification;
use rustdb_manager::domain::inventory::{
    AppliedMigrationRecord, ColumnInventory, DatabaseSchemaInventory, ProgrammableObjectInventory,
    ProgrammableObjectKind, TableInventory,
};
use rustdb_manager::domain::manifest::{compute_normalized_str_sha256, EngineKind, MigrationManifest};
use rustdb_manager::domain::normalizer::compute_semantic_sql_hash;
use std::collections::BTreeMap;
use std::path::Path;

fn sample_column(name: &str, data_type: &str) -> ColumnInventory {
    ColumnInventory {
        name: name.to_string(),
        ordinal_position: 1,
        data_type: data_type.to_string(),
        max_length: 100,
        precision: 0,
        scale: 0,
        is_nullable: false,
        is_identity: false,
        default_definition: None,
        collation_name: None,
    }
}

fn sample_table(schema: &str, name: &str, cols: &[(&str, &str)]) -> TableInventory {
    let mut columns = BTreeMap::new();
    for (c_name, c_type) in cols {
        columns.insert(c_name.to_lowercase(), sample_column(c_name, c_type));
    }
    TableInventory {
        schema_name: schema.to_string(),
        table_name: name.to_string(),
        columns,
        indexes: BTreeMap::new(),
        foreign_keys: BTreeMap::new(),
        check_constraints: BTreeMap::new(),
    }
}

fn sample_sp(schema: &str, name: &str, sql: &str) -> ProgrammableObjectInventory {
    ProgrammableObjectInventory {
        schema_name: schema.to_string(),
        object_name: name.to_string(),
        kind: ProgrammableObjectKind::StoredProcedure,
        raw_definition: sql.to_string(),
        raw_sha256: compute_normalized_str_sha256(sql),
        semantic_sha256: compute_semantic_sql_hash(sql),
    }
}

#[test]
fn detects_explained_changes_cross_client_contamination_and_manual_drift() {
    let registry = CatalogRegistry::new(Path::new("tests/fixtures/catalog"));
    let all_migrations: Vec<MigrationManifest> = registry
        .load_all_migrations_for_product("sistema-erp")
        .unwrap()
        .into_iter()
        .map(|(m, _)| m)
        .collect();

    // 1. Build Source Baseline Inventory (DEMO_PROD_BASE)
    let mut src_tables = BTreeMap::new();
    src_tables.insert(
        "dbo.customers".to_string(),
        sample_table(
            "dbo",
            "Customers",
            &[("CustomerId", "int"), ("TaxId", "nvarchar")],
        ),
    );
    let mut src_sps = BTreeMap::new();
    src_sps.insert(
        "dbo.usp_issueinvoice".to_string(),
        sample_sp(
            "dbo",
            "usp_IssueInvoice",
            "CREATE PROCEDURE [dbo].[usp_IssueInvoice] AS BEGIN SELECT 1; END",
        ),
    );

    let source_inv = DatabaseSchemaInventory {
        database_name: "DEMO_PROD_BASE".to_string(),
        engine: EngineKind::SqlServer,
        inspected_at_utc: Utc::now(),
        instance_meta: None,
        applied_migrations: Vec::new(),
        schemas: vec!["dbo".to_string()],
        tables: src_tables,
        programmable_objects: src_sps,
    };

    // 2. Build Target Inventory (DEMO_CLIENTE_A) with:
    // - Legitimate Client A change on dbo.Customers (+LoyaltyTier from CC-A001)
    // - Legitimate Client A change on dbo.usp_IssueInvoice (from CC-A002)
    // - CONTAMINATION: dbo.DigiflowEndpoints table belonging to Client B (CC-B001)!
    // - MANUAL DRIFT: dbo.TmpManualHotfix table not registered in any migration!
    let mut tgt_tables = BTreeMap::new();
    tgt_tables.insert(
        "dbo.customers".to_string(),
        sample_table(
            "dbo",
            "Customers",
            &[
                ("CustomerId", "int"),
                ("TaxId", "nvarchar"),
                ("LoyaltyTier", "nvarchar"),
            ],
        ),
    );
    tgt_tables.insert(
        "dbo.digiflowendpoints".to_string(),
        sample_table("dbo", "DigiflowEndpoints", &[("EndpointId", "int")]),
    );
    tgt_tables.insert(
        "dbo.tmpmanualhotfix".to_string(),
        sample_table("dbo", "TmpManualHotfix", &[("HotfixId", "int")]),
    );

    let mut tgt_sps = BTreeMap::new();
    tgt_sps.insert(
        "dbo.usp_issueinvoice".to_string(),
        sample_sp(
            "dbo",
            "usp_IssueInvoice",
            "CREATE PROCEDURE [dbo].[usp_IssueInvoice] AS BEGIN INSERT INTO AuditLog VALUES ('A'); END",
        ),
    );

    let target_inv = DatabaseSchemaInventory {
        database_name: "DEMO_CLIENTE_A".to_string(),
        engine: EngineKind::SqlServer,
        inspected_at_utc: Utc::now(),
        instance_meta: None,
        applied_migrations: vec![
            AppliedMigrationRecord {
                migration_id: "20261121_CC_A001".to_string(),
                change_control_id: Some("CC-A001".to_string()),
                scope: "CLIENT".to_string(),
                client_id: Some("cliente-a".to_string()),
                script_path: "up/001_add_loyalty_column.sql".to_string(),
                checksum_sha256: "abc".to_string(),
                executed_by: "sa".to_string(),
                executed_at_utc: Utc::now().to_rfc3339(),
                execution_ms: 12,
                success: true,
            },
            AppliedMigrationRecord {
                migration_id: "20261121_CC_A002".to_string(),
                change_control_id: Some("CC-A002".to_string()),
                scope: "CLIENT".to_string(),
                client_id: Some("cliente-a".to_string()),
                script_path: "up/001_alter_usp_issue_invoice.sql".to_string(),
                checksum_sha256: "def".to_string(),
                executed_by: "sa".to_string(),
                executed_at_utc: Utc::now().to_rfc3339(),
                execution_ms: 15,
                success: true,
            },
        ],
        schemas: vec!["dbo".to_string()],
        tables: tgt_tables,
        programmable_objects: tgt_sps,
    };

    let report = compare_and_classify(
        &source_inv,
        &target_inv,
        Some("cliente-a"),
        &all_migrations,
        "UnitTestEngine",
    );

    assert_eq!(report.summary.total_differences, 4);
    assert_eq!(report.summary.explained_changes, 2);
    assert_eq!(report.summary.cross_client_contaminations, 1);
    assert_eq!(report.summary.unexplained_manual_drifts, 1);
    assert!(!report.summary.is_clean_for_deployment);

    let contamination = report
        .differences
        .iter()
        .find(|d| d.classification == DriftClassification::CrossClientContamination)
        .expect("Should detect Client B DigiflowEndpoints contamination");
    assert_eq!(contamination.object_name, "dbo.DigiflowEndpoints");
    assert_eq!(
        contamination.attributed_client_id.as_deref(),
        Some("cliente-b")
    );
}
