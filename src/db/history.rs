/// T-SQL DDL and queries for in-database metadata and migration history tracking.

pub const ENSURE_TRACKING_TABLES_SQL: &str = r#"
IF OBJECT_ID(N'[dbo].[__rustdb_instance_meta]', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[__rustdb_instance_meta] (
        [instance_uid]       NVARCHAR(64)     NOT NULL PRIMARY KEY,
        [product_id]         NVARCHAR(100)    NOT NULL,
        [baseline_version]   NVARCHAR(50)     NOT NULL,
        [client_id]          NVARCHAR(100)    NULL,
        [environment]        NVARCHAR(50)     NOT NULL,
        [origin_mode]        NVARCHAR(50)     NOT NULL,
        [source_provenance]  NVARCHAR(500)    NULL,
        [trust_level]        NVARCHAR(50)     NOT NULL,
        [created_at_utc]     NVARCHAR(64)     NOT NULL
    );
END;

IF OBJECT_ID(N'[dbo].[__rustdb_schema_history]', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[__rustdb_schema_history] (
        [execution_id]       INT IDENTITY(1,1) NOT NULL PRIMARY KEY,
        [migration_id]       NVARCHAR(150)     NOT NULL,
        [change_control_id]  NVARCHAR(100)     NULL,
        [scope]              NVARCHAR(20)      NOT NULL,
        [client_id]          NVARCHAR(100)     NULL,
        [script_path]        NVARCHAR(500)     NOT NULL,
        [checksum_sha256]    CHAR(64)          NOT NULL,
        [executed_by]        NVARCHAR(128)     NOT NULL,
        [executed_at_utc]    NVARCHAR(64)      NOT NULL,
        [execution_ms]       BIGINT            NOT NULL,
        [success]            BIT               NOT NULL
    );
END;
"#;
