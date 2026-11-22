use crate::config::ConnectionProfile;
use crate::db::batch_splitter::split_tsql_batches;
use crate::db::history::ENSURE_TRACKING_TABLES_SQL;
use crate::domain::error::DomainError;
use crate::domain::inventory::{
    AppliedMigrationRecord, CheckConstraintInventory, ColumnInventory, DatabaseSchemaInventory,
    ForeignKeyInventory, IndexInventory, InstanceMetadata, ProgrammableObjectInventory,
    ProgrammableObjectKind, TableInventory,
};
use crate::domain::manifest::{compute_normalized_str_sha256, EngineKind};
use crate::domain::normalizer::compute_semantic_sql_hash;
use crate::ports::DatabaseEnginePort;
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;
use std::path::Path;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub type SqlServerClient = Client<Compat<TcpStream>>;

pub struct SqlServerAdapter {
    profile: ConnectionProfile,
}

impl SqlServerAdapter {
    pub fn new(profile: ConnectionProfile) -> Self {
        Self { profile }
    }

    async fn connect_to_database(&self, database_name: &str) -> Result<SqlServerClient, DomainError> {
        let mut config = Config::new();
        config.host(&self.profile.host);
        config.port(self.profile.port);
        config.database(database_name);

        if self.profile.trust_server_certificate {
            config.trust_cert();
            config.encryption(EncryptionLevel::Required);
        }

        if self.profile.integrated_auth {
            #[cfg(windows)]
            {
                config.authentication(AuthMethod::Integrated);
            }
            #[cfg(not(windows))]
            {
                return Err(DomainError::DatabaseError(
                    "Integrated Windows authentication requires Windows or Kerberos configuration"
                        .to_string(),
                ));
            }
        } else {
            let user = self.profile.user.as_deref().unwrap_or("sa");
            let password = self.profile.resolve_password()?.unwrap_or_default();
            config.authentication(AuthMethod::sql_server(user, password));
        }

        let tcp = TcpStream::connect(config.get_addr())
            .await
            .map_err(|e| DomainError::DatabaseError(format!("TCP connection failed: {e}")))?;
        tcp.set_nodelay(true)
            .map_err(|e| DomainError::DatabaseError(format!("Failed to set TCP_NODELAY: {e}")))?;

        Client::connect(config, tcp.compat_write())
            .await
            .map_err(|e| DomainError::DatabaseError(format!("TDS handshake failed: {e}")))
    }
}

fn sanitize_identifier(ident: &str) -> Result<String, DomainError> {
    if ident.is_empty()
        || !ident
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(DomainError::DatabaseError(format!(
            "Unsafe database identifier rejected: '{ident}'"
        )));
    }
    Ok(format!("[{}]", ident.replace(']', "]]")))
}

#[async_trait]
impl DatabaseEnginePort for SqlServerAdapter {
    async fn test_connection(&self) -> Result<String, DomainError> {
        let mut client = self.connect_to_database(&self.profile.database).await?;
        let stream = client
            .simple_query("SELECT @@VERSION AS server_version;")
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Query failed: {e}")))?;
        let row = stream
            .into_row()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Row fetch failed: {e}")))?
            .ok_or_else(|| DomainError::DatabaseError("Empty version response".to_string()))?;

        let version: &str = row.get("server_version").unwrap_or("Unknown SQL Server");
        Ok(version.lines().next().unwrap_or(version).to_string())
    }

    async fn list_databases(&self) -> Result<Vec<String>, DomainError> {
        let mut client = self.connect_to_database("master").await?;
        let rows = client
            .simple_query(
                "SELECT [name] FROM sys.databases WHERE database_id > 4 AND state_desc = 'ONLINE' ORDER BY [name];",
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to list databases: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read database list: {e}")))?;

        let mut dbs = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(name) = row.get::<&str, _>("name") {
                dbs.push(name.to_string());
            }
        }
        Ok(dbs)
    }

    async fn create_database(&self, db_name: &str, drop_if_exists: bool) -> Result<(), DomainError> {
        let quoted = sanitize_identifier(db_name)?;
        let mut client = self.connect_to_database("master").await?;

        if drop_if_exists {
            let drop_sql = format!(
                "IF DB_ID(N'{db_name}') IS NOT NULL BEGIN ALTER DATABASE {quoted} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE {quoted}; END;"
            );
            client
                .simple_query(drop_sql)
                .await
                .map_err(|e| DomainError::DatabaseError(format!("Failed to drop '{db_name}': {e}")))?
                .into_results()
                .await
                .map_err(|e| DomainError::DatabaseError(format!("Failed to drop '{db_name}': {e}")))?;
        }

        let create_sql = format!(
            "IF DB_ID(N'{db_name}') IS NULL BEGIN CREATE DATABASE {quoted}; END;"
        );
        client
            .simple_query(create_sql)
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to create '{db_name}': {e}")))?
            .into_results()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to create '{db_name}': {e}")))?;

        Ok(())
    }

    async fn ensure_tracking_tables(&self, db_name: &str) -> Result<(), DomainError> {
        let mut client = self.connect_to_database(db_name).await?;
        client
            .simple_query(ENSURE_TRACKING_TABLES_SQL)
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!(
                    "Failed to initialize tracking tables in '{db_name}': {e}"
                ))
            })?
            .into_results()
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!(
                    "Failed to initialize tracking tables in '{db_name}': {e}"
                ))
            })?;
        Ok(())
    }

    async fn write_instance_metadata(
        &self,
        db_name: &str,
        meta: &InstanceMetadata,
    ) -> Result<(), DomainError> {
        self.ensure_tracking_tables(db_name).await?;
        let mut client = self.connect_to_database(db_name).await?;

        client
            .execute(
                "DELETE FROM [dbo].[__rustdb_instance_meta];
                 INSERT INTO [dbo].[__rustdb_instance_meta]
                 ([instance_uid], [product_id], [baseline_version], [client_id], [environment], [origin_mode], [source_provenance], [trust_level], [created_at_utc])
                 VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8, @P9);",
                &[
                    &meta.instance_uid.as_str(),
                    &meta.product_id.as_str(),
                    &meta.baseline_version.as_str(),
                    &meta.client_id.as_deref(),
                    &meta.environment.as_str(),
                    &meta.origin_mode.as_str(),
                    &meta.source_provenance.as_deref(),
                    &meta.trust_level.as_str(),
                    &meta.created_at_utc.as_str(),
                ],
            )
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!(
                    "Failed to record instance metadata in '{db_name}': {e}"
                ))
            })?;

        Ok(())
    }

    async fn execute_tracked_script(
        &self,
        db_name: &str,
        script_sql: &str,
        record: &AppliedMigrationRecord,
    ) -> Result<(), DomainError> {
        self.ensure_tracking_tables(db_name).await?;
        let mut client = self.connect_to_database(db_name).await?;

        let batches = split_tsql_batches(script_sql);
        let start = std::time::Instant::now();

        for (idx, batch) in batches.iter().enumerate() {
            client
                .simple_query(batch.as_str())
                .await
                .map_err(|e| {
                    DomainError::DatabaseError(format!(
                        "Error in script '{}' (batch #{}): {e}",
                        record.script_path,
                        idx + 1
                    ))
                })?
                .into_results()
                .await
                .map_err(|e| {
                    DomainError::DatabaseError(format!(
                        "Execution error in script '{}' (batch #{}): {e}",
                        record.script_path,
                        idx + 1
                    ))
                })?;
        }

        let elapsed_ms = start.elapsed().as_millis() as i64;

        client
            .execute(
                "INSERT INTO [dbo].[__rustdb_schema_history]
                 ([migration_id], [change_control_id], [scope], [client_id], [script_path], [checksum_sha256], [executed_by], [executed_at_utc], [execution_ms], [success])
                 VALUES (@P1, @P2, @P3, @P4, @P5, @P6, SUSER_SNAME(), @P7, @P8, 1);",
                &[
                    &record.migration_id.as_str(),
                    &record.change_control_id.as_deref(),
                    &record.scope.as_str(),
                    &record.client_id.as_deref(),
                    &record.script_path.as_str(),
                    &record.checksum_sha256.as_str(),
                    &record.executed_at_utc.as_str(),
                    &elapsed_ms,
                ],
            )
            .await
            .map_err(|e| {
                DomainError::DatabaseError(format!(
                    "Failed to record schema history for '{}': {e}",
                    record.migration_id
                ))
            })?;

        Ok(())
    }

    async fn physical_backup_restore_copy(
        &self,
        source_db: &str,
        target_db: &str,
        backup_dir: Option<&Path>,
    ) -> Result<String, DomainError> {
        let _ = sanitize_identifier(source_db)?;
        let quoted_target = sanitize_identifier(target_db)?;
        let mut client = self.connect_to_database("master").await?;

        // 1. Discover SQL Server default backup, data, and log paths
        let path_rows = client
            .simple_query(
                "SELECT
                    CAST(SERVERPROPERTY('InstanceDefaultBackupPath') AS NVARCHAR(500)) AS backup_path,
                    CAST(SERVERPROPERTY('InstanceDefaultDataPath') AS NVARCHAR(500)) AS data_path,
                    CAST(SERVERPROPERTY('InstanceDefaultLogPath') AS NVARCHAR(500)) AS log_path;",
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to query instance paths: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read instance paths: {e}")))?;

        let path_row = path_rows
            .first()
            .ok_or_else(|| DomainError::DatabaseError("No instance paths returned".to_string()))?;

        let default_backup_dir = backup_dir
            .map(|p| p.display().to_string())
            .or_else(|| path_row.get::<&str, _>("backup_path").map(|s| s.to_string()))
            .unwrap_or_else(|| "C:\\Temp".to_string());
        let data_dir = path_row
            .get::<&str, _>("data_path")
            .unwrap_or(&default_backup_dir)
            .trim_end_matches(['\\', '/'])
            .to_string();
        let log_dir = path_row
            .get::<&str, _>("log_path")
            .unwrap_or(&data_dir)
            .trim_end_matches(['\\', '/'])
            .to_string();

        let separator = if data_dir.contains('\\') { '\\' } else { '/' };

        // Check if `default_backup_dir` actually exists on the SQL Server host via sys.dm_os_file_exists.
        // On some installations (like MSSQL16.MIKI_VS), `MSSQL\Backup` does not exist and the service
        // account lacks permission to create directories in `MSSQL\`, whereas `data_dir` (`MSSQL\DATA`)
        // always exists and is writable by the SQL Server service account.
        let mut effective_backup_dir = default_backup_dir.trim_end_matches(['\\', '/']).to_string();
        let dir_check_sql = format!(
            "SELECT file_is_a_directory FROM sys.dm_os_file_exists(N'{}');",
            effective_backup_dir.replace('\'', "''")
        );
        if let Ok(stream) = client.simple_query(dir_check_sql).await {
            if let Ok(rows) = stream.into_first_result().await {
                let exists = rows
                    .first()
                    .and_then(|r| {
                        r.try_get::<i32, _>("file_is_a_directory")
                            .ok()
                            .flatten()
                            .map(|v| v == 1)
                            .or_else(|| {
                                r.try_get::<bool, _>("file_is_a_directory")
                                    .ok()
                                    .flatten()
                            })
                    })
                    .unwrap_or(false);
                if !exists {
                    effective_backup_dir = data_dir.clone();
                }
            }
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_file = format!(
            "{}{}{}_copy_{}.bak",
            effective_backup_dir,
            separator,
            source_db,
            timestamp
        );

        // 2. Execute COPY_ONLY backup
        let backup_sql = format!(
            "BACKUP DATABASE [{source_db}] TO DISK = N'{backup_file}' WITH COPY_ONLY, INIT, STATS = 25;"
        );
        client
            .simple_query(backup_sql)
            .await
            .map_err(|e| DomainError::DatabaseError(format!("BACKUP DATABASE failed: {e}")))?
            .into_results()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("BACKUP DATABASE failed: {e}")))?;

        // 3. Query logical file names from the source database
        let files_sql = format!(
            "SELECT [name] AS logical_name, [type_desc] FROM [{source_db}].sys.database_files;"
        );
        let file_rows = client
            .simple_query(files_sql)
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to query database files: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read database files: {e}")))?;

        let mut move_clauses = Vec::new();
        for (idx, frow) in file_rows.iter().enumerate() {
            let logical_name = frow.get::<&str, _>("logical_name").unwrap_or("data");
            let type_desc = frow.get::<&str, _>("type_desc").unwrap_or("ROWS");
            let target_path = if type_desc.eq_ignore_ascii_case("LOG") {
                format!("{log_dir}{separator}{target_db}_{idx}_log.ldf")
            } else {
                let ext = if idx == 0 { "mdf" } else { "ndf" };
                format!("{data_dir}{separator}{target_db}_{idx}.{ext}")
            };
            move_clauses.push(format!("MOVE N'{logical_name}' TO N'{target_path}'"));
        }

        // 4. Drop target if exists and restore WITH MOVE
        let drop_target_sql = format!(
            "IF DB_ID(N'{target_db}') IS NOT NULL BEGIN ALTER DATABASE {quoted_target} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE {quoted_target}; END;"
        );
        client
            .simple_query(drop_target_sql)
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to prepare target DB: {e}")))?
            .into_results()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to prepare target DB: {e}")))?;

        let restore_sql = format!(
            "RESTORE DATABASE {quoted_target} FROM DISK = N'{backup_file}' WITH {}, REPLACE, RECOVERY;",
            move_clauses.join(", ")
        );
        client
            .simple_query(restore_sql)
            .await
            .map_err(|e| DomainError::DatabaseError(format!("RESTORE DATABASE failed: {e}")))?
            .into_results()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("RESTORE DATABASE failed: {e}")))?;

        Ok(backup_file)
    }

    async fn inspect_schema(&self, db_name: &str) -> Result<DatabaseSchemaInventory, DomainError> {
        let mut client = self.connect_to_database(db_name).await?;

        // 1. Schemas
        let schema_rows = client
            .simple_query(
                "SELECT [name] FROM sys.schemas WHERE principal_id = 1 AND [name] NOT IN ('sys', 'INFORMATION_SCHEMA', 'guest') ORDER BY [name];",
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect schemas: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read schemas: {e}")))?;

        let schemas: Vec<String> = schema_rows
            .iter()
            .filter_map(|r| r.get::<&str, _>("name").map(|s| s.to_string()))
            .collect();

        // 2. Tables & Columns (excluding internal tracking tables)
        let col_rows = client
            .simple_query(
                r#"
                SELECT
                    s.name AS schema_name,
                    t.name AS table_name,
                    c.name AS column_name,
                    CAST(c.column_id AS INT) AS ordinal_position,
                    ty.name AS data_type,
                    c.max_length,
                    c.precision,
                    c.scale,
                    c.is_nullable,
                    c.is_identity,
                    dc.definition AS default_definition,
                    c.collation_name
                FROM sys.tables t
                INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
                INNER JOIN sys.columns c ON t.object_id = c.object_id
                INNER JOIN sys.types ty ON c.user_type_id = ty.user_type_id
                LEFT JOIN sys.default_constraints dc ON c.default_object_id = dc.object_id
                WHERE t.is_ms_shipped = 0
                  AND t.name NOT IN ('__rustdb_instance_meta', '__rustdb_schema_history', 'sysdiagrams')
                ORDER BY s.name, t.name, c.column_id;
                "#,
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect columns: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read columns: {e}")))?;

        let mut tables: BTreeMap<String, TableInventory> = BTreeMap::new();

        for row in col_rows {
            let schema_name = row.get::<&str, _>("schema_name").unwrap_or("dbo").to_string();
            let table_name = row.get::<&str, _>("table_name").unwrap_or("").to_string();
            let key = format!("{}.{}", schema_name.to_lowercase(), table_name.to_lowercase());

            let table_entry = tables.entry(key).or_insert_with(|| TableInventory {
                schema_name: schema_name.clone(),
                table_name: table_name.clone(),
                columns: BTreeMap::new(),
                indexes: BTreeMap::new(),
                foreign_keys: BTreeMap::new(),
                check_constraints: BTreeMap::new(),
            });

            let col_name = row.get::<&str, _>("column_name").unwrap_or("").to_string();
            let col = ColumnInventory {
                name: col_name.clone(),
                ordinal_position: row.get::<i32, _>("ordinal_position").unwrap_or(0),
                data_type: row.get::<&str, _>("data_type").unwrap_or("").to_string(),
                max_length: row.get::<i16, _>("max_length").unwrap_or(0),
                precision: row.get::<u8, _>("precision").unwrap_or(0),
                scale: row.get::<u8, _>("scale").unwrap_or(0),
                is_nullable: row.get::<bool, _>("is_nullable").unwrap_or(false),
                is_identity: row.get::<bool, _>("is_identity").unwrap_or(false),
                default_definition: row
                    .get::<&str, _>("default_definition")
                    .map(|s| s.to_string()),
                collation_name: row.get::<&str, _>("collation_name").map(|s| s.to_string()),
            };
            table_entry.columns.insert(col_name.to_lowercase(), col);
        }

        // 3. Indexes & Primary Keys
        let idx_rows = client
            .simple_query(
                r#"
                SELECT
                    s.name AS schema_name,
                    t.name AS table_name,
                    i.name AS index_name,
                    i.type_desc AS index_type,
                    i.is_unique,
                    i.is_primary_key,
                    i.filter_definition,
                    c.name AS column_name,
                    ic.is_included_column,
                    ic.key_ordinal
                FROM sys.indexes i
                INNER JOIN sys.tables t ON i.object_id = t.object_id
                INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
                INNER JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id
                INNER JOIN sys.columns c ON ic.object_id = c.object_id AND ic.column_id = c.column_id
                WHERE t.is_ms_shipped = 0
                  AND i.name IS NOT NULL
                  AND t.name NOT IN ('__rustdb_instance_meta', '__rustdb_schema_history', 'sysdiagrams')
                ORDER BY s.name, t.name, i.name, ic.key_ordinal, ic.index_column_id;
                "#,
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect indexes: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read indexes: {e}")))?;

        for row in idx_rows {
            let schema_name = row.get::<&str, _>("schema_name").unwrap_or("dbo");
            let table_name = row.get::<&str, _>("table_name").unwrap_or("");
            let table_key = format!("{}.{}", schema_name.to_lowercase(), table_name.to_lowercase());

            if let Some(table) = tables.get_mut(&table_key) {
                let idx_name = row.get::<&str, _>("index_name").unwrap_or("").to_string();
                let col_name = row.get::<&str, _>("column_name").unwrap_or("").to_string();
                let is_included = row.get::<bool, _>("is_included_column").unwrap_or(false);

                let idx_entry = table
                    .indexes
                    .entry(idx_name.to_lowercase())
                    .or_insert_with(|| IndexInventory {
                        name: idx_name,
                        index_type: row.get::<&str, _>("index_type").unwrap_or("").to_string(),
                        is_unique: row.get::<bool, _>("is_unique").unwrap_or(false),
                        is_primary_key: row.get::<bool, _>("is_primary_key").unwrap_or(false),
                        key_columns: Vec::new(),
                        included_columns: Vec::new(),
                        filter_definition: row
                            .get::<&str, _>("filter_definition")
                            .map(|s| s.to_string()),
                    });

                if is_included {
                    idx_entry.included_columns.push(col_name);
                } else {
                    idx_entry.key_columns.push(col_name);
                }
            }
        }

        // 4. Foreign Keys
        let fk_rows = client
            .simple_query(
                r#"
                SELECT
                    s.name AS schema_name,
                    tp.name AS parent_table,
                    fk.name AS fk_name,
                    sr.name AS ref_schema_name,
                    tr.name AS referenced_table,
                    cp.name AS parent_column,
                    cr.name AS referenced_column,
                    fk.delete_referential_action_desc,
                    fk.update_referential_action_desc
                FROM sys.foreign_keys fk
                INNER JOIN sys.tables tp ON fk.parent_object_id = tp.object_id
                INNER JOIN sys.schemas s ON tp.schema_id = s.schema_id
                INNER JOIN sys.tables tr ON fk.referenced_object_id = tr.object_id
                INNER JOIN sys.schemas sr ON tr.schema_id = sr.schema_id
                INNER JOIN sys.foreign_key_columns fkc ON fk.object_id = fkc.constraint_object_id
                INNER JOIN sys.columns cp ON fkc.parent_object_id = cp.object_id AND fkc.parent_column_id = cp.column_id
                INNER JOIN sys.columns cr ON fkc.referenced_object_id = cr.object_id AND fkc.referenced_column_id = cr.column_id
                ORDER BY s.name, tp.name, fk.name, fkc.constraint_column_id;
                "#,
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect foreign keys: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read foreign keys: {e}")))?;

        for row in fk_rows {
            let schema_name = row.get::<&str, _>("schema_name").unwrap_or("dbo");
            let parent_table = row.get::<&str, _>("parent_table").unwrap_or("");
            let table_key = format!("{}.{}", schema_name.to_lowercase(), parent_table.to_lowercase());

            if let Some(table) = tables.get_mut(&table_key) {
                let fk_name = row.get::<&str, _>("fk_name").unwrap_or("").to_string();
                let ref_schema = row.get::<&str, _>("ref_schema_name").unwrap_or("dbo");
                let ref_table = row.get::<&str, _>("referenced_table").unwrap_or("");
                let parent_col = row.get::<&str, _>("parent_column").unwrap_or("").to_string();
                let ref_col = row.get::<&str, _>("referenced_column").unwrap_or("").to_string();

                let fk_entry = table
                    .foreign_keys
                    .entry(fk_name.to_lowercase())
                    .or_insert_with(|| ForeignKeyInventory {
                        name: fk_name,
                        parent_table: format!("{schema_name}.{parent_table}"),
                        referenced_table: format!("{ref_schema}.{ref_table}"),
                        columns: Vec::new(),
                        delete_referential_action: row
                            .get::<&str, _>("delete_referential_action_desc")
                            .unwrap_or("NO_ACTION")
                            .to_string(),
                        update_referential_action: row
                            .get::<&str, _>("update_referential_action_desc")
                            .unwrap_or("NO_ACTION")
                            .to_string(),
                    });

                fk_entry.columns.push((parent_col, ref_col));
            }
        }

        // 5. Check Constraints
        let cc_rows = client
            .simple_query(
                r#"
                SELECT
                    s.name AS schema_name,
                    t.name AS table_name,
                    cc.name AS constraint_name,
                    cc.definition
                FROM sys.check_constraints cc
                INNER JOIN sys.tables t ON cc.parent_object_id = t.object_id
                INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
                WHERE t.is_ms_shipped = 0
                ORDER BY s.name, t.name, cc.name;
                "#,
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect check constraints: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read check constraints: {e}")))?;

        for row in cc_rows {
            let schema_name = row.get::<&str, _>("schema_name").unwrap_or("dbo");
            let table_name = row.get::<&str, _>("table_name").unwrap_or("");
            let table_key = format!("{}.{}", schema_name.to_lowercase(), table_name.to_lowercase());
            if let Some(table) = tables.get_mut(&table_key) {
                let cc_name = row.get::<&str, _>("constraint_name").unwrap_or("").to_string();
                let definition = row.get::<&str, _>("definition").unwrap_or("").to_string();
                table.check_constraints.insert(
                    cc_name.to_lowercase(),
                    CheckConstraintInventory {
                        name: cc_name,
                        definition,
                    },
                );
            }
        }

        // 6. Programmable Objects (Stored Procedures, Functions, Views, Triggers)
        let prog_rows = client
            .simple_query(
                r#"
                SELECT
                    s.name AS schema_name,
                    o.name AS object_name,
                    RTRIM(o.type) AS object_type,
                    m.definition
                FROM sys.sql_modules m
                INNER JOIN sys.objects o ON m.object_id = o.object_id
                INNER JOIN sys.schemas s ON o.schema_id = s.schema_id
                WHERE o.is_ms_shipped = 0
                  AND RTRIM(o.type) IN ('P', 'FN', 'IF', 'TF', 'V', 'TR')
                ORDER BY s.name, o.name;
                "#,
            )
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to inspect programmable objects: {e}")))?
            .into_first_result()
            .await
            .map_err(|e| DomainError::DatabaseError(format!("Failed to read programmable objects: {e}")))?;

        let mut programmable_objects = BTreeMap::new();
        for row in prog_rows {
            let schema_name = row.get::<&str, _>("schema_name").unwrap_or("dbo").to_string();
            let object_name = row.get::<&str, _>("object_name").unwrap_or("").to_string();
            let obj_type = row.get::<&str, _>("object_type").unwrap_or("P");
            let raw_definition = row.get::<&str, _>("definition").unwrap_or("").to_string();

            let kind = match obj_type {
                "P" => ProgrammableObjectKind::StoredProcedure,
                "FN" => ProgrammableObjectKind::ScalarFunction,
                "IF" | "TF" => ProgrammableObjectKind::TableFunction,
                "V" => ProgrammableObjectKind::View,
                "TR" => ProgrammableObjectKind::Trigger,
                _ => ProgrammableObjectKind::StoredProcedure,
            };

            let raw_sha256 = compute_normalized_str_sha256(&raw_definition);
            let semantic_sha256 = compute_semantic_sql_hash(&raw_definition);
            let key = format!("{}.{}", schema_name.to_lowercase(), object_name.to_lowercase());

            programmable_objects.insert(
                key,
                ProgrammableObjectInventory {
                    schema_name,
                    object_name,
                    kind,
                    raw_definition,
                    raw_sha256,
                    semantic_sha256,
                },
            );
        }

        // 7. Read Tracking Metadata & Applied Migrations if present
        let mut instance_meta = None;
        {
            let meta_rows = client
                .simple_query(
                    "IF OBJECT_ID(N'[dbo].[__rustdb_instance_meta]', N'U') IS NOT NULL
                     SELECT TOP 1 [instance_uid], [product_id], [baseline_version], [client_id], [environment], [origin_mode], [source_provenance], [trust_level], [created_at_utc]
                     FROM [dbo].[__rustdb_instance_meta];",
                )
                .await;

            if let Ok(stream) = meta_rows {
                if let Ok(results) = stream.into_first_result().await {
                    if let Some(row) = results.first() {
                        instance_meta = Some(InstanceMetadata {
                            instance_uid: row.get::<&str, _>("instance_uid").unwrap_or("").to_string(),
                            product_id: row.get::<&str, _>("product_id").unwrap_or("").to_string(),
                            baseline_version: row
                                .get::<&str, _>("baseline_version")
                                .unwrap_or("")
                                .to_string(),
                            client_id: row.get::<&str, _>("client_id").map(|s| s.to_string()),
                            environment: row.get::<&str, _>("environment").unwrap_or("").to_string(),
                            origin_mode: row.get::<&str, _>("origin_mode").unwrap_or("").to_string(),
                            source_provenance: row
                                .get::<&str, _>("source_provenance")
                                .map(|s| s.to_string()),
                            trust_level: row.get::<&str, _>("trust_level").unwrap_or("").to_string(),
                            created_at_utc: row
                                .get::<&str, _>("created_at_utc")
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                }
            }
        }

        let mut applied_migrations = Vec::new();
        {
            let hist_rows = client
                .simple_query(
                    "IF OBJECT_ID(N'[dbo].[__rustdb_schema_history]', N'U') IS NOT NULL
                     SELECT [migration_id], [change_control_id], [scope], [client_id], [script_path], [checksum_sha256], [executed_by], [executed_at_utc], [execution_ms], [success]
                     FROM [dbo].[__rustdb_schema_history]
                     ORDER BY [execution_id];",
                )
                .await;

            if let Ok(stream) = hist_rows {
                if let Ok(results) = stream.into_first_result().await {
                    for row in results {
                        applied_migrations.push(AppliedMigrationRecord {
                            migration_id: row.get::<&str, _>("migration_id").unwrap_or("").to_string(),
                            change_control_id: row
                                .get::<&str, _>("change_control_id")
                                .map(|s| s.to_string()),
                            scope: row.get::<&str, _>("scope").unwrap_or("").to_string(),
                            client_id: row.get::<&str, _>("client_id").map(|s| s.to_string()),
                            script_path: row.get::<&str, _>("script_path").unwrap_or("").to_string(),
                            checksum_sha256: row
                                .get::<&str, _>("checksum_sha256")
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                            executed_by: row.get::<&str, _>("executed_by").unwrap_or("").to_string(),
                            executed_at_utc: row
                                .get::<&str, _>("executed_at_utc")
                                .unwrap_or("")
                                .to_string(),
                            execution_ms: row.get::<i64, _>("execution_ms").unwrap_or(0),
                            success: row.get::<bool, _>("success").unwrap_or(true),
                        });
                    }
                }
            }
        }

        Ok(DatabaseSchemaInventory {
            database_name: db_name.to_string(),
            engine: EngineKind::SqlServer,
            inspected_at_utc: Utc::now(),
            instance_meta,
            applied_migrations,
            schemas,
            tables,
            programmable_objects,
        })
    }
}
