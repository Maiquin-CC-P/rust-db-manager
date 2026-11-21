use anyhow::{Context, Result};
use clap::Parser;
use rustdb_manager::baseline::{build_database_from_baseline, CatalogRegistry};
use rustdb_manager::cli::{
    BaselineCommands, Cli, Commands, CompareEngineArg, ConnectionCommands, CopyModeArg, DbCommands,
    ReportFormatArg, SchemaCommands,
};
use rustdb_manager::cloner::{copy_database, CopyRequest};
use rustdb_manager::comparator::classifier::compare_and_classify;
use rustdb_manager::comparator::sqlpackage_bridge::SqlPackageBridge;
use rustdb_manager::config::{load_connection_profile, ConnectionProfile};
use rustdb_manager::db::sqlserver::SqlServerAdapter;
use rustdb_manager::domain::error::DomainError;
use rustdb_manager::domain::manifest::{resolve_migration_plan, EngineKind, MigrationManifest};
use rustdb_manager::ports::{CopyMode, DatabaseEnginePort};
use rustdb_manager::report::{render_report_json, render_report_markdown};
use std::fs;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn build_engine_adapter(
    profile: ConnectionProfile,
) -> Result<Arc<dyn DatabaseEnginePort>, DomainError> {
    match profile.engine {
        EngineKind::SqlServer => Ok(Arc::new(SqlServerAdapter::new(profile))),
        EngineKind::MySql => Err(DomainError::UnsupportedEngine(
            "MySQL adapter is scheduled for MVP 0.2 (Hexagonal Port 'DatabaseEnginePort' is ready)."
                .to_string(),
        )),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Connection { action } => match action {
            ConnectionCommands::Test { profile } => {
                let prof = load_connection_profile(&cli.config, &profile)?;
                let engine = build_engine_adapter(prof)?;
                let version = engine.test_connection().await?;
                println!("Connection OK [profile: {profile}] -> {version}");
            }
            ConnectionCommands::ListDbs { profile } => {
                let prof = load_connection_profile(&cli.config, &profile)?;
                let engine = build_engine_adapter(prof)?;
                let dbs = engine.list_databases().await?;
                println!("Online user databases ({}):", dbs.len());
                for db in dbs {
                    println!("  - {db}");
                }
            }
        },

        Commands::Schema { action } => match action {
            SchemaCommands::Inspect {
                profile,
                database,
                output,
            } => {
                let prof = load_connection_profile(&cli.config, &profile)?;
                let engine = build_engine_adapter(prof)?;
                info!("Inspecting schema of database '{database}'...");
                let inventory = engine.inspect_schema(&database).await?;
                let json = serde_json::to_string_pretty(&inventory)?;

                if let Some(out_path) = output {
                    if let Some(parent) = out_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&out_path, &json)?;
                    println!(
                        "Schema inventory exported to '{}' ({} tables, {} programmable objects).",
                        out_path.display(),
                        inventory.tables.len(),
                        inventory.programmable_objects.len()
                    );
                } else {
                    println!("{json}");
                }
            }
        },

        Commands::Baseline { action } => match action {
            BaselineCommands::Validate {
                catalog,
                product,
                version,
                client,
            } => {
                let registry = CatalogRegistry::new(&catalog);
                let (baseline, baseline_dir) = registry.load_baseline(&product, &version)?;
                let verified = baseline.validate_scripts(&baseline_dir)?;

                println!(
                    "Baseline '{}@{}' (state: {:?}) validated successfully ({} scripts verified):",
                    baseline.product_id,
                    baseline.version,
                    baseline.state,
                    verified.len()
                );
                for (path, hash) in &verified {
                    println!("  [OK] {} (sha256: {}...)", path.display(), &hash[..12]);
                }

                if let Some(cid) = client {
                    let (client_manifest, _) = registry.load_client(&cid)?;
                    let all_migs_with_paths = registry.load_all_migrations_for_product(&product)?;
                    let all_migs: Vec<MigrationManifest> =
                        all_migs_with_paths.iter().map(|(m, _)| m.clone()).collect();

                    let mut required_ids = client_manifest.approved_product_migrations.clone();
                    required_ids.extend(client_manifest.approved_client_migrations.clone());

                    let plan = resolve_migration_plan(
                        &product,
                        Some(&client_manifest.client_id),
                        &all_migs,
                        &required_ids,
                        true,
                    )?;
                    println!(
                        "\nClient overlay '{}' migration DAG validated ({} ordered migration(s)):",
                        client_manifest.client_id,
                        plan.len()
                    );
                    for (idx, m) in plan.iter().enumerate() {
                        println!(
                            "  {}. {} ({}) [scope: {:?}, client: {}]",
                            idx + 1,
                            m.migration_id,
                            m.change_id,
                            m.scope,
                            m.client_id.as_deref().unwrap_or("ALL")
                        );
                    }
                }
            }

            BaselineCommands::Build {
                catalog,
                product,
                version,
                client,
                profile,
                database,
                drop_if_exists,
                allow_destructive,
            } => {
                let prof = load_connection_profile(&cli.config, &profile)?;
                let engine = build_engine_adapter(prof)?;
                let registry = CatalogRegistry::new(&catalog);

                info!(
                    "Building database '{}' from baseline '{}@{}' (client: {:?})...",
                    database, product, version, client
                );
                let applied = build_database_from_baseline(
                    engine.as_ref(),
                    &registry,
                    &product,
                    &version,
                    client.as_deref(),
                    &database,
                    drop_if_exists,
                    allow_destructive,
                )
                .await?;

                println!(
                    "Database '{}' built and tracked successfully ({} script(s) applied).",
                    database,
                    applied.len()
                );
            }
        },

        Commands::Db { action } => match action {
            DbCommands::Copy {
                profile,
                mode,
                source,
                target,
                catalog,
                product,
                version,
                client,
                backup_dir,
                drop_if_exists,
                allow_destructive,
            } => {
                let prof = load_connection_profile(&cli.config, &profile)?;
                let engine = build_engine_adapter(prof.clone())?;
                let registry = CatalogRegistry::new(&catalog);

                let copy_mode = match mode {
                    CopyModeArg::Rebuild => CopyMode::Rebuild,
                    CopyModeArg::BackupRestore => CopyMode::BackupRestore,
                    CopyModeArg::SchemaClone => CopyMode::SchemaClone,
                };

                let receipt = copy_database(
                    engine.as_ref(),
                    &prof,
                    CopyRequest {
                        mode: copy_mode,
                        source_db: source.as_deref(),
                        target_db: &target,
                        catalog: Some(&registry),
                        product_id: product.as_deref(),
                        baseline_version: version.as_deref(),
                        client_id: client.as_deref(),
                        backup_dir: backup_dir.as_deref(),
                        drop_if_exists,
                        allow_destructive,
                    },
                )
                .await?;

                println!(
                    "Database copy completed:\n  - Target DB: {}\n  - Mode: {}\n  - Provenance: {}",
                    receipt.target_database, receipt.mode_used, receipt.provenance
                );
                if let Some(artifact) = receipt.artifact_path {
                    println!("  - Artifact: {artifact}");
                }
            }
        },

        Commands::Compare {
            profile,
            source,
            target,
            catalog,
            product,
            client,
            engine: cmp_engine,
            format,
            output,
        } => {
            let prof = load_connection_profile(&cli.config, &profile)?;
            let db_engine = build_engine_adapter(prof.clone())?;
            let registry = CatalogRegistry::new(&catalog);

            info!("Inspecting source database '{source}'...");
            let src_inv = db_engine.inspect_schema(&source).await?;

            info!("Inspecting target database '{target}'...");
            let tgt_inv = db_engine.inspect_schema(&target).await?;

            let all_migrations: Vec<MigrationManifest> = registry
                .load_all_migrations_for_product(&product)
                .unwrap_or_default()
                .into_iter()
                .map(|(m, _)| m)
                .collect();

            let mut engine_label = "Native Catalog Inspector (sys.* + T-SQL Semantic Hash)".to_string();
            if matches!(cmp_engine, CompareEngineArg::Hybrid) {
                if let Some(bridge) = SqlPackageBridge::detect() {
                    info!(
                        "SqlPackage detected at '{}'; running hybrid DACPAC DeployReport check...",
                        bridge.binary_path.display()
                    );
                    let _xml = bridge
                        .generate_deploy_report_xml(&prof, &source, &target)
                        .await
                        .context("SqlPackage hybrid DeployReport failed")?;
                    engine_label =
                        "Hybrid (Native sys.* Catalog + Microsoft SqlPackage DACPAC)".to_string();
                } else {
                    info!("SqlPackage not found in PATH; using Native Catalog Inspector Layer 1.");
                }
            }

            let target_client = client
                .or_else(|| tgt_inv.instance_meta.as_ref().and_then(|m| m.client_id.clone()));

            let report = compare_and_classify(
                &src_inv,
                &tgt_inv,
                target_client.as_deref(),
                &all_migrations,
                &engine_label,
            );

            let rendered = match format {
                ReportFormatArg::Markdown => render_report_markdown(&report),
                ReportFormatArg::Json => render_report_json(&report)?,
            };

            if let Some(out_path) = output {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_path, &rendered)?;
                println!(
                    "Comparison report written to '{}' (Total diffs: {}, Contaminations: {}, Manual drifts: {}).",
                    out_path.display(),
                    report.summary.total_differences,
                    report.summary.cross_client_contaminations,
                    report.summary.unexplained_manual_drifts
                );
            } else {
                println!("{rendered}");
            }
        }
    }

    Ok(())
}
