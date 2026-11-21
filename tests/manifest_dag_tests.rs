use rustdb_manager::baseline::CatalogRegistry;
use rustdb_manager::domain::error::DomainError;
use rustdb_manager::domain::manifest::{resolve_migration_plan, MigrationManifest};
use std::path::Path;

#[test]
fn validates_demo_catalog_baseline_and_client_a_dag() {
    let registry = CatalogRegistry::new(Path::new("tests/fixtures/catalog"));
    let (baseline, baseline_dir) = registry
        .load_baseline("sistema-erp", "v1")
        .expect("Failed to load sistema-erp@v1 baseline");
    let verified = baseline
        .validate_scripts(&baseline_dir)
        .expect("Failed to validate baseline scripts");
    assert_eq!(verified.len(), 3);

    let (client_a, _) = registry
        .load_client("cliente-a")
        .expect("Failed to load cliente-a manifest");
    let all_migs: Vec<MigrationManifest> = registry
        .load_all_migrations_for_product("sistema-erp")
        .expect("Failed to load product migrations")
        .into_iter()
        .map(|(m, _)| m)
        .collect();

    let mut required = client_a.approved_product_migrations.clone();
    required.extend(client_a.approved_client_migrations.clone());

    let plan = resolve_migration_plan(
        "sistema-erp",
        Some("cliente-a"),
        &all_migs,
        &required,
        false,
    )
    .expect("Failed to resolve Client A migration DAG");

    let ordered_ids: Vec<&str> = plan.iter().map(|m| m.migration_id.as_str()).collect();
    assert_eq!(
        ordered_ids,
        vec![
            "20261121_BASE_001",
            "20261121_CC_A001",
            "20261121_CC_A002"
        ]
    );
}

#[test]
fn blocks_cross_client_contamination_in_migration_plan() {
    let registry = CatalogRegistry::new(Path::new("tests/fixtures/catalog"));
    let all_migs: Vec<MigrationManifest> = registry
        .load_all_migrations_for_product("sistema-erp")
        .expect("Failed to load product migrations")
        .into_iter()
        .map(|(m, _)| m)
        .collect();

    // Attempt to schedule Client B's Digiflow migration (20261121_CC_B001) on Client A
    let tainted_request = vec![
        "20261121_BASE_001".to_string(),
        "20261121_CC_B001".to_string(),
    ];

    let result = resolve_migration_plan(
        "sistema-erp",
        Some("cliente-a"),
        &all_migs,
        &tainted_request,
        false,
    );

    match result {
        Err(DomainError::CrossClientContaminationBlocked {
            migration_id,
            migration_client,
            target_client,
        }) => {
            assert_eq!(migration_id, "20261121_CC_B001");
            assert_eq!(migration_client, "cliente-b");
            assert_eq!(target_client, "cliente-a");
        }
        other => panic!("Expected CrossClientContaminationBlocked, got: {other:?}"),
    }
}
