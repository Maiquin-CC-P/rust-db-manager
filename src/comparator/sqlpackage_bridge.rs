use crate::config::ConnectionProfile;
use crate::domain::error::DomainError;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tokio::process::Command as AsyncCommand;

/// Hybrid Layer 2 Adapter for Microsoft's official `SqlPackage` (DacFx) CLI.
///
/// Used when `--engine dacpac` or `--mode schema-clone` is requested.
/// When `SqlPackage` is not installed on the machine, Layer 1 (`sys.*` native Rust inspector)
/// still operates with zero external dependencies.
pub struct SqlPackageBridge {
    pub binary_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DacpacOperationAlert {
    pub operation_type: String,
    pub item_value: String,
    pub item_type: String,
}

impl SqlPackageBridge {
    /// Detects if `SqlPackage` (or `sqlpackage.exe`) is available in PATH or standard installation folders.
    pub fn detect() -> Option<Self> {
        let candidates = ["SqlPackage", "sqlpackage", "sqlpackage.exe"];
        for bin in candidates {
            if let Ok(output) = Command::new(bin).arg("/Version").output() {
                if output.status.success() {
                    return Some(Self {
                        binary_path: PathBuf::from(bin),
                    });
                }
            }
        }

        // Check standard dotnet global tool path on Windows/Linux/macOS
        if let Ok(home) = env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
            let dotnet_tool = PathBuf::from(home)
                .join(".dotnet")
                .join("tools")
                .join(if cfg!(windows) {
                    "SqlPackage.exe"
                } else {
                    "SqlPackage"
                });
            if dotnet_tool.exists() {
                return Some(Self {
                    binary_path: dotnet_tool,
                });
            }
        }

        None
    }

    fn build_connection_string(
        profile: &ConnectionProfile,
        db_name: &str,
    ) -> Result<String, DomainError> {
        let trust = if profile.trust_server_certificate {
            "True"
        } else {
            "False"
        };
        if profile.integrated_auth {
            Ok(format!(
                "Server={},{};Database={};Integrated Security=True;Encrypt=True;TrustServerCertificate={};",
                profile.host, profile.port, db_name, trust
            ))
        } else {
            let user = profile.user.as_deref().unwrap_or("sa");
            let pwd = profile.resolve_password()?.unwrap_or_default();
            Ok(format!(
                "Server={},{};Database={};User Id={};Password={};Encrypt=True;TrustServerCertificate={};",
                profile.host, profile.port, db_name, user, pwd, trust
            ))
        }
    }

    /// Extracts a live SQL Server database schema into a `.dacpac` artifact.
    pub async fn extract_dacpac(
        &self,
        profile: &ConnectionProfile,
        db_name: &str,
        output_dacpac: &PathBuf,
    ) -> Result<(), DomainError> {
        let conn_str = Self::build_connection_string(profile, db_name)?;
        let output = AsyncCommand::new(&self.binary_path)
            .arg("/Action:Extract")
            .arg(format!("/SourceConnectionString:{conn_str}"))
            .arg(format!("/TargetFile:{}", output_dacpac.display()))
            .output()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to execute SqlPackage: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(DomainError::DatabaseError(format!(
                "SqlPackage Extract failed: {stdout} {stderr}"
            )));
        }
        Ok(())
    }

    /// Clones a database structure (schema-only) by extracting a `.dacpac` from `source_db`
    /// and publishing it to `target_db`.
    pub async fn clone_schema_via_dacpac(
        &self,
        profile: &ConnectionProfile,
        source_db: &str,
        target_db: &str,
    ) -> Result<PathBuf, DomainError> {
        let temp_dir = env::temp_dir();
        let dacpac_path = temp_dir.join(format!("{source_db}_schema.dacpac"));
        self.extract_dacpac(profile, source_db, &dacpac_path)
            .await?;

        let target_conn = Self::build_connection_string(profile, target_db)?;
        let output = AsyncCommand::new(&self.binary_path)
            .arg("/Action:Publish")
            .arg(format!("/SourceFile:{}", dacpac_path.display()))
            .arg(format!("/TargetConnectionString:{target_conn}"))
            .arg("/p:CreateNewDatabase=True")
            .output()
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!("Failed to run SqlPackage Publish: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(DomainError::DatabaseError(format!(
                "SqlPackage Publish failed: {stdout} {stderr}"
            )));
        }

        Ok(dacpac_path)
    }

    /// Generates an XML `DeployReport` comparing `source_db` against `target_db`.
    pub async fn generate_deploy_report_xml(
        &self,
        profile: &ConnectionProfile,
        source_db: &str,
        target_db: &str,
    ) -> Result<String, DomainError> {
        let temp_dir = env::temp_dir();
        let source_dacpac = temp_dir.join(format!("{source_db}_cmp.dacpac"));
        let report_xml_path = temp_dir.join(format!("{source_db}_vs_{target_db}_report.xml"));

        self.extract_dacpac(profile, source_db, &source_dacpac)
            .await?;
        let target_conn = Self::build_connection_string(profile, target_db)?;

        let output = AsyncCommand::new(&self.binary_path)
            .arg("/Action:DeployReport")
            .arg(format!("/SourceFile:{}", source_dacpac.display()))
            .arg(format!("/TargetConnectionString:{target_conn}"))
            .arg(format!("/OutputPath:{}", report_xml_path.display()))
            .arg("/p:IgnorePermissions=True")
            .arg("/p:IgnoreWhitespace=True")
            .arg("/p:DropObjectsNotInSource=True")
            .output()
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!("Failed to run SqlPackage DeployReport: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(DomainError::DatabaseError(format!(
                "SqlPackage DeployReport failed: {stdout} {stderr}"
            )));
        }

        fs::read_to_string(&report_xml_path)
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read DeployReport XML: {e}")))
    }
}
