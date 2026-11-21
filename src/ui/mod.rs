use crate::baseline::{build_database_from_baseline, CatalogRegistry};
use crate::cloner::{copy_database, CopyRequest};
use crate::comparator::classifier::compare_and_classify;
use crate::config::{load_connection_profile, ConnectionProfile, ConnectionsConfigFile};
use crate::db::sqlserver::SqlServerAdapter;
use crate::domain::error::DomainError;
use crate::domain::manifest::{resolve_migration_plan, EngineKind, MigrationManifest};
use crate::ports::{CopyMode, DatabaseEnginePort};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tracing::info;

const INDEX_HTML: &str = include_str!("index.html");

#[derive(Clone)]
pub struct UiAppState {
    pub config_path: PathBuf,
    pub catalog_path: PathBuf,
}

fn build_adapter(profile: ConnectionProfile) -> Result<Arc<dyn DatabaseEnginePort>, DomainError> {
    match profile.engine {
        EngineKind::SqlServer => Ok(Arc::new(SqlServerAdapter::new(profile))),
        EngineKind::MySql => Err(DomainError::UnsupportedEngine(
            "MySQL adapter is scheduled for MVP 0.2".to_string(),
        )),
    }
}

pub async fn start_ui_server(
    config_path: PathBuf,
    catalog_path: PathBuf,
    host: &str,
    port: u16,
    launch_desktop_window: bool,
) -> anyhow::Result<()> {
    let state = UiAppState {
        config_path,
        catalog_path,
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(api_status))
        .route("/api/connection/test", get(api_connection_test))
        .route("/api/databases", get(api_list_databases))
        .route("/api/schema/inspect", get(api_schema_inspect))
        .route("/api/baseline/validate", post(api_baseline_validate))
        .route("/api/baseline/build", post(api_baseline_build))
        .route("/api/db/copy", post(api_db_copy))
        .route("/api/compare", post(api_compare))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let url = format!(
        "http://{}:{}",
        if host == "0.0.0.0" { "localhost" } else { host },
        port
    );

    info!("RustDB Manager Studio listening on {url}");
    println!("==============================================================");
    println!("  RustDB Manager Studio is running at: {url}");
    println!("==============================================================");

    if launch_desktop_window {
        open_native_app_window(&url);
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Launches a standalone Desktop App Window on Windows using Microsoft Edge / Chrome `--app` mode
/// (no address bar, no browser tabs, behaves like a native desktop window), falling back to the default browser.
fn open_native_app_window(url: &str) {
    #[cfg(windows)]
    {
        let app_arg = format!("--app={url}");
        // Try Microsoft Edge in native App Window mode first
        if Command::new("cmd")
            .args(["/C", "start", "", "msedge", &app_arg])
            .spawn()
            .is_ok()
        {
            return;
        }
        // Fallback to default browser
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn api_status(State(state): State<UiAppState>) -> impl IntoResponse {
    let mut profiles = Vec::new();
    if let Ok(raw) = fs::read_to_string(&state.config_path) {
        if let Ok(parsed) = toml::from_str::<ConnectionsConfigFile>(&raw) {
            profiles = parsed.profiles.keys().cloned().collect();
            profiles.sort();
        }
    }
    let default_profile = if profiles.iter().any(|p| p == "local_windows_auth") {
        "local_windows_auth".to_string()
    } else {
        profiles
            .first()
            .cloned()
            .unwrap_or_else(|| "local".to_string())
    };

    let mut products = Vec::new();
    let prod_dir = state.catalog_path.join("products");
    if let Ok(entries) = fs::read_dir(prod_dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    products.push(name.to_string());
                }
            }
        }
    }
    products.sort();

    let mut clients = Vec::new();
    let clients_dir = state.catalog_path.join("clients");
    if let Ok(entries) = fs::read_dir(clients_dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    clients.push(name.to_string());
                }
            }
        }
    }
    clients.sort();

    Json(json!({
        "profiles": profiles,
        "default_profile": default_profile,
        "products": products,
        "clients": clients
    }))
}

async fn api_connection_test(
    State(state): State<UiAppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let profile_name = params
        .get("profile")
        .map(|s| s.as_str())
        .unwrap_or("local_windows_auth");
    let prof = load_connection_profile(&state.config_path, profile_name).map_err(err_json)?;
    let engine = build_adapter(prof).map_err(err_json)?;
    let version = engine.test_connection().await.map_err(err_json)?;
    Ok(Json(json!({ "server_version": version })))
}

async fn api_list_databases(
    State(state): State<UiAppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let profile_name = params
        .get("profile")
        .map(|s| s.as_str())
        .unwrap_or("local_windows_auth");
    let prof = load_connection_profile(&state.config_path, profile_name).map_err(err_json)?;
    let engine = build_adapter(prof).map_err(err_json)?;
    let dbs = engine.list_databases().await.map_err(err_json)?;
    Ok(Json(json!({ "databases": dbs })))
}

async fn api_schema_inspect(
    State(state): State<UiAppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let profile_name = params
        .get("profile")
        .map(|s| s.as_str())
        .unwrap_or("local_windows_auth");
    let database = params.get("database").cloned().unwrap_or_default();
    let prof = load_connection_profile(&state.config_path, profile_name).map_err(err_json)?;
    let engine = build_adapter(prof).map_err(err_json)?;
    let inv = engine.inspect_schema(&database).await.map_err(err_json)?;
    Ok(Json(serde_json::to_value(inv).unwrap()))
}

#[derive(Deserialize)]
struct ValidateBaselinePayload {
    product: String,
    version: String,
    client: Option<String>,
}

async fn api_baseline_validate(
    State(state): State<UiAppState>,
    Json(payload): Json<ValidateBaselinePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let registry = CatalogRegistry::new(&state.catalog_path);
    let (baseline, baseline_dir) = registry
        .load_baseline(&payload.product, &payload.version)
        .map_err(err_json)?;
    let verified = baseline.validate_scripts(&baseline_dir).map_err(err_json)?;

    let mut log = format!(
        "Baseline '{}@{}' validated ({} scripts):\n",
        baseline.product_id,
        baseline.version,
        verified.len()
    );
    for (p, h) in &verified {
        log.push_str(&format!("  [OK] {} (sha256: {}...)\n", p.display(), &h[..16]));
    }

    if let Some(cid) = payload.client.as_deref().filter(|s| !s.is_empty()) {
        let (client_manifest, _) = registry.load_client(cid).map_err(err_json)?;
        let all_migs: Vec<MigrationManifest> = registry
            .load_all_migrations_for_product(&payload.product)
            .map_err(err_json)?
            .into_iter()
            .map(|(m, _)| m)
            .collect();

        let mut req_ids = client_manifest.approved_product_migrations.clone();
        req_ids.extend(client_manifest.approved_client_migrations.clone());

        let plan = resolve_migration_plan(
            &payload.product,
            Some(&client_manifest.client_id),
            &all_migs,
            &req_ids,
            true,
        )
        .map_err(err_json)?;

        log.push_str(&format!(
            "\nClient '{}' DAG resolved ({} migration(s)):\n",
            cid,
            plan.len()
        ));
        for (i, m) in plan.iter().enumerate() {
            log.push_str(&format!(
                "  {}. {} ({}) [scope: {:?}]\n",
                i + 1,
                m.migration_id,
                m.change_id,
                m.scope
            ));
        }
    }

    Ok(Json(json!({ "log": log })))
}

#[derive(Deserialize)]
struct BuildBaselinePayload {
    profile: String,
    product: String,
    version: String,
    client: Option<String>,
    database: String,
    drop_if_exists: bool,
}

async fn api_baseline_build(
    State(state): State<UiAppState>,
    Json(payload): Json<BuildBaselinePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let prof = load_connection_profile(&state.config_path, &payload.profile).map_err(err_json)?;
    let engine = build_adapter(prof).map_err(err_json)?;
    let registry = CatalogRegistry::new(&state.catalog_path);

    let applied = build_database_from_baseline(
        engine.as_ref(),
        &registry,
        &payload.product,
        &payload.version,
        payload.client.as_deref().filter(|s| !s.is_empty()),
        &payload.database,
        payload.drop_if_exists,
        true,
    )
    .await
    .map_err(err_json)?;

    let mut log = format!(
        "Built database '{}' with {} tracked script(s):\n",
        payload.database,
        applied.len()
    );
    for r in &applied {
        log.push_str(&format!(
            "  [APPLIED] {} ({}) -> {}\n",
            r.migration_id, r.scope, r.script_path
        ));
    }

    Ok(Json(json!({
        "message": format!("Database '{}' built successfully ({} scripts applied).", payload.database, applied.len()),
        "log": log
    })))
}

#[derive(Deserialize)]
struct CopyDbPayload {
    profile: String,
    mode: String,
    source: String,
    target: String,
    client: Option<String>,
}

async fn api_db_copy(
    State(state): State<UiAppState>,
    Json(payload): Json<CopyDbPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let prof = load_connection_profile(&state.config_path, &payload.profile).map_err(err_json)?;
    let engine = build_adapter(prof.clone()).map_err(err_json)?;
    let registry = CatalogRegistry::new(&state.catalog_path);

    let mode = match payload.mode.as_str() {
        "schema-clone" => CopyMode::SchemaClone,
        _ => CopyMode::BackupRestore,
    };

    let receipt = copy_database(
        engine.as_ref(),
        &prof,
        CopyRequest {
            mode,
            source_db: Some(&payload.source),
            target_db: &payload.target,
            catalog: Some(&registry),
            product_id: None,
            baseline_version: None,
            client_id: payload.client.as_deref().filter(|s| !s.is_empty()),
            backup_dir: None,
            drop_if_exists: true,
            allow_destructive: false,
        },
    )
    .await
    .map_err(err_json)?;

    Ok(Json(json!({
        "message": format!("Database '{}' copied to '{}'", payload.source, receipt.target_database),
        "mode_used": receipt.mode_used,
        "provenance": receipt.provenance,
        "artifact_path": receipt.artifact_path
    })))
}

#[derive(Deserialize, Serialize)]
struct ComparePayload {
    profile: String,
    source: String,
    target: String,
    product: String,
    client: Option<String>,
}

async fn api_compare(
    State(state): State<UiAppState>,
    Json(payload): Json<ComparePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let prof = load_connection_profile(&state.config_path, &payload.profile).map_err(err_json)?;
    let engine = build_adapter(prof).map_err(err_json)?;
    let registry = CatalogRegistry::new(&state.catalog_path);

    let src_inv = engine
        .inspect_schema(&payload.source)
        .await
        .map_err(err_json)?;
    let tgt_inv = engine
        .inspect_schema(&payload.target)
        .await
        .map_err(err_json)?;

    let all_migrations: Vec<MigrationManifest> = registry
        .load_all_migrations_for_product(&payload.product)
        .unwrap_or_default()
        .into_iter()
        .map(|(m, _)| m)
        .collect();

    let target_client = payload
        .client
        .filter(|s| !s.is_empty())
        .or_else(|| tgt_inv.instance_meta.as_ref().and_then(|m| m.client_id.clone()));

    let report = compare_and_classify(
        &src_inv,
        &tgt_inv,
        target_client.as_deref(),
        &all_migrations,
        "Native Catalog Inspector (sys.* + Semantic SHA-256)",
    );

    Ok(Json(serde_json::to_value(report).unwrap()))
}

fn err_json(err: DomainError) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": err.to_string() })),
    )
}
