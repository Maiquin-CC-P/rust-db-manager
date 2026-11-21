use crate::domain::error::DomainError;
use crate::domain::manifest::EngineKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub engine: EngineKind,
    pub host: String,
    pub port: u16,
    #[serde(default = "default_database")]
    pub database: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default = "default_true")]
    pub trust_server_certificate: bool,
    #[serde(default)]
    pub integrated_auth: bool,
}

fn default_database() -> String {
    "master".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsConfigFile {
    pub profiles: HashMap<String, ConnectionProfile>,
}

impl ConnectionProfile {
    /// Resolves the password exclusively from the declared environment variable (`password_env`).
    pub fn resolve_password(&self) -> Result<Option<String>, DomainError> {
        if self.integrated_auth {
            return Ok(None);
        }
        if let Some(env_var) = &self.password_env {
            match env::var(env_var) {
                Ok(val) => Ok(Some(val)),
                Err(_) => Err(DomainError::DatabaseError(format!(
                    "Missing required environment variable '{env_var}' for SQL authentication"
                ))),
            }
        } else {
            Ok(None)
        }
    }
}

pub fn load_connection_profile(
    config_path: &Path,
    profile_name: &str,
) -> Result<ConnectionProfile, DomainError> {
    let content = fs::read_to_string(config_path).map_err(|_| {
        DomainError::ManifestNotFound(format!(
            "Connection config file not found at '{}'",
            config_path.display()
        ))
    })?;
    let parsed: ConnectionsConfigFile = toml::from_str(&content).map_err(|e| {
        DomainError::DatabaseError(format!("Invalid connections TOML format: {e}"))
    })?;
    parsed.profiles.get(profile_name).cloned().ok_or_else(|| {
        DomainError::DatabaseError(format!(
            "Connection profile '{profile_name}' not found in '{}'",
            config_path.display()
        ))
    })
}
