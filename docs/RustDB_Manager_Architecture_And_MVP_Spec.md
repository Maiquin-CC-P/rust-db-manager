# RustDB Manager — Technical Specification, Architecture & MVP 0.1 Execution Plan

**Document Type:** Architecture Specification & Viability Deliverable (Section 15 Resolution)  
**Status:** Approved Baseline for Implementation  
**Target Engine:** Microsoft SQL Server (2016 / 2019 / 2022+)  
**Core Stack:** Rust (CLI Orchestrator, Manifest & DAG Engine, Catalog Inspector, Copy/Rebuild Runner) + Microsoft `SqlPackage` (DacFx CLI for Deep AST Schema Comparison)

---

## 1. Problem Diagnosis & Viability Verdict

### 1.1 Root Cause Analysis
The problem described in `RustDB_Manager_Problema_Solucion_Viabilidad.md` is not a lack of SQL migration runners, but **Multi-Tenant Database Fork Drift** combined with **Environment Contamination**:
1. **Single Mutable Working Database:** Developers reuse a local or staging database that received scripts for Client B (`CC-B001`) and attempt to revert it via fragile `DOWN` scripts or manual changes when returning to Client A.
2. **Untracked Scope Boundaries:** Standard migration tools (Flyway Community, DbUp) assume a single linear history (`V1 -> V2 -> V3`). They lack native first-class primitives to enforce that a migration with `scope = client(B)` must **never** execute on an instance belonging to `client(A)`.
3. **Opaque Drift vs. Legitimate Customization:** Traditional schema comparators report raw differences between two databases but cannot distinguish whether an extra table or modified Stored Procedure is:
   - An expected client customization (`EXPLAINED_BY_CLIENT_MIGRATION`),
   - Cross-client contamination (`CROSS_CLIENT_CONTAMINATION`), or
   - An undocumented hotfix applied directly in production (`UNEXPLAINED_MANUAL_DRIFT`).

### 1.2 Viability Verdict
**VIABLE WITH ARCHITECTURAL CONSTRAINTS (Hybrid Approach Mandatory)**

- **What MUST be built in Rust:** The Domain Orchestrator, Product/Client Manifest & DAG Dependency Engine, SHA-256 Integrity Validator, T-SQL Batch Runner (`GO` splitter), Database Cloner (`BACKUP`/`RESTORE WITH MOVE` + Script Rebuilder), Native Catalog Inspector (`sys.*` fast inventory), and the **Context-Aware Diff Classifier**.
- **What MUST NOT be built from scratch in Rust:** A full T-SQL AST parser and semantic DDL comparator. SQL Server's T-SQL grammar is massive and context-dependent. RustDB Manager will integrate a dual-layer comparator:
  - **Layer 1 (Native Rust Catalog Inspector):** Queries SQL Server system views (`sys.tables`, `sys.columns`, `sys.sql_modules`, `sys.indexes`, `sys.foreign_keys`, etc.) for zero-dependency fast inventory and hash normalization.
  - **Layer 2 (Microsoft `SqlPackage` / DacFx CLI Bridge):** Invoked programmatically by Rust (`std::process::Command`) to extract `.dacpac` snapshots and produce semantic `DeployReport.xml` diffs when deep AST comparison is required.

---

## 2. Comparison of Alternatives

| Approach | Pros | Cons | Verdict |
|---|---|---|---|
| **1. Pure Off-the-Shelf (Flyway / Liquibase / DbUp)** | Mature execution engines; zero custom code for basic linear migrations. | Poor native modeling for `Base Product + Per-Client Overlays`; no built-in cross-client contamination detection; enterprise drift detection requires expensive per-seat licenses. | **Insufficient alone** |
| **2. Pure Custom Rust (100% from scratch, including T-SQL parser)** | Single binary, no external tools. | Months wasted writing a T-SQL parser/differ that will still miss edge cases in Stored Procedures, Triggers, Defaults, and Indexes. High maintenance risk. | **Not Recommended** |
| **3. Hybrid Rust Orchestrator + Native `sys.*` + `SqlPackage` Bridge** | Fast, safe Rust CLI enforces client isolation, builds baselines, clones DBs, tracks history, and classifies drift, while delegating deep T-SQL AST diffing to Microsoft's official engine when needed. | Requires `SqlPackage` installed on machines running deep DACPAC comparisons (Layer 1 `sys.*` inspection still works everywhere without external dependencies). | **RECOMMENDED** |

---

## 3. Recommended Architecture

### 3.1 High-Level Component Diagram

```text
+-----------------------------------------------------------------------------------+
|                                  rustdb (Rust CLI)                                |
+-----------------------------------------------------------------------------------+
         |                       |                        |                   |
         v                       v                        v                   v
+-----------------+   +--------------------+   +-------------------+   +------------+
| Manifest & DAG  |   | Database & Clone   |   | Migration Runner  |   | Comparator |
|     Engine      |   |      Manager       |   | & History Tracker |   |   Engine   |
+-----------------+   +--------------------+   +-------------------+   +------------+
  - Product Base        - CREATE DATABASE        - T-SQL GO Parser       - Layer 1:
  - Client Overlays     - Rebuild from Scripts   - Transaction Control     Native sys.*
  - SHA-256 Hashes      - BACKUP / RESTORE       - [__rustdb_history]      Catalog Diff
  - Scope Guard           WITH COPY_ONLY &       - [__rustdb_meta]       - Layer 2:
    (Client A != B)       WITH MOVE (.mdf/.ldf)                            SqlPackage
                                                                           DACPAC XML
                                                                         - Drift &
                                                                           Contamination
                                                                           Classifier
```

### 3.2 Verified Rust Technology Stack

> **CRITICAL SECURITY NOTE ON `tiberius`:**  
> Upstream `tiberius` (0.12.x) has stalled maintenance and unpatched RustSec advisories in its TLS chain (`RUSTSEC-2026-0098`, `RUSTSEC-2026-0099`, `RUSTSEC-2026-0104`).  
> **Mandate:** Use **`tiberius-ng`** (the actively maintained community continuation with TDS 8.0 support and patched TLS advisories) or **`odbc-api`** backed by the official Microsoft ODBC Driver for SQL Server.

| Component | Crate / Tool | Justification |
|---|---|---|
| **Language & Edition** | Rust (2024 / 2021 Edition) | Strong type safety for domain invariants (Scope, Hashes, DAGs). |
| **CLI Framework** | `clap` (v4, derive) | Subcommands, environment variable binding, clear help generation. |
| **Async Runtime** | `tokio` (v1, full) | Async I/O for database connections and subprocess execution. |
| **SQL Server Driver** | `tiberius-ng` + `tokio-util` | Patched TDS driver supporting SQL Auth, Windows/Integrated Auth, and TLS. |
| **Serialization** | `serde`, `serde_json`, `toml`, `quick-xml` | JSON/TOML for manifests and configs; `quick-xml` for parsing `SqlPackage` `DeployReport.xml`. |
| **Cryptography** | `sha2` + `hex` | Deterministic SHA-256 hashing of normalized SQL scripts and deployment packages. |
| **Graph & Dependencies** | `petgraph` | Topological sorting and cycle detection for baseline and client migration dependencies. |
| **Error Handling** | `thiserror` + `anyhow` | Structured domain errors in library modules; rich contextual diagnostics in CLI. |
| **Observability** | `tracing` + `tracing-subscriber` | Structured audit logs for every SQL batch and administrative operation. |

---

## 4. Versioning, Isolation & Data Model

### 4.1 Core Domain Entities

1. **Product Baseline (`BaselineManifest`):**
   - Represents an immutable, validated foundation of a product (e.g., `sistema-erp@v1.0.0`).
   - Contains ordered schema creation scripts, programmable objects, and system-owned master data scripts, each locked with a SHA-256 hash.
2. **Client Profile (`ClientManifest`):**
   - Binds a client (`client_id: "CLIENTE_A"`) to a specific product and base version (`base_version: "v1.0.0"`).
   - Lists the allowed target environments (`DEV`, `UAT`, `PROD`) and explicitly authorized client migration IDs.
3. **Migration Unit (`MigrationManifest`):**
   - Identified by an immutable `migration_id` (e.g., `20261121_CC_A001`), never by a mutable sequential file prefix alone.
   - Declares `scope`: `"product"` (applies to all clients on compatible base version) or `"client"` (requires explicit `client_id`).
   - Declares `dependencies`: list of prerequisite `migration_id`s (forming a Directed Acyclic Graph).
   - Declares `affected_objects`: explicit list of `[schema].[object_name]` touched by this migration (used by the Comparator to explain differences).

### 4.2 In-Database Tracking Tables

Every managed SQL Server database stores its own self-identifying metadata in two dedicated system tables created by RustDB Manager:

#### Table 1: `[dbo].[__rustdb_instance_meta]`
Prevents executing scripts on the wrong database or mixing client backups.
```sql
CREATE TABLE [dbo].[__rustdb_instance_meta] (
    [instance_uid]       UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    [product_id]         NVARCHAR(100)    NOT NULL,
    [baseline_version]   NVARCHAR(50)     NOT NULL,
    [client_id]          NVARCHAR(100)    NULL, -- NULL for pure product baseline DBs
    [environment]        NVARCHAR(50)     NOT NULL, -- LOCAL_REBUILD, DEV, UAT, PROD
    [origin_mode]        NVARCHAR(50)     NOT NULL, -- SCRIPT_REBUILD, BACKUP_RESTORE, DACPAC_CLONE
    [source_ provenance] NVARCHAR(500)    NULL,
    [trust_level]        NVARCHAR(50)     NOT NULL, -- DRAFT, RECONSTRUCTED, VALIDATED_BASELINE, CLONED_UNVERIFIED
    [created_at_utc]     DATETIME2        NOT NULL DEFAULT SYSUTCDATETIME()
);
```

#### Table 2: `[dbo].[__rustdb_schema_history]`
Tracks every baseline script and migration applied to the instance.
```sql
CREATE TABLE [dbo].[__rustdb_schema_history] (
    [execution_id]       INT IDENTITY(1,1) NOT NULL PRIMARY KEY,
    [migration_id]       NVARCHAR(150)     NOT NULL UNIQUE,
    [change_control_id]  NVARCHAR(100)     NULL, -- e.g., CC-001, CC-A002
    [scope]              NVARCHAR(20)      NOT NULL, -- BASELINE, PRODUCT, CLIENT
    [client_id]          NVARCHAR(100)     NULL,
    [script_path]        NVARCHAR(500)     NOT NULL,
    [checksum_sha256]    CHAR(64)          NOT NULL,
    [executed_by]        NVARCHAR(128)     NOT NULL DEFAULT SUSER_SNAME(),
    [executed_at_utc]    DATETIME2         NOT NULL DEFAULT SYSUTCDATETIME(),
    [execution_ms]       BIGINT            NOT NULL,
    [success]            BIT               NOT NULL
);
```

### 4.3 Elimination of `DOWN` Scripts for Client Switching
- **Architectural Rule:** `DOWN` scripts must **NEVER** be used to convert a database from Client B back to Client A.
- **Why:** Destructive DDL (`DROP COLUMN`, `DROP TABLE`, altered data types) and DML changes cannot be deterministically reversed without risking silent corruption or orphaned metadata.
- **How Client Switching Works in RustDB Manager:** Switching from Client B to Client A means provisioning or targeting an isolated database (`ERP_CLIENTE_A_DEV`) built deterministically from `Baseline(v1) + Migrations(Client A)` or restored from a verified Client A backup.

---

## 5. Database Copy & Cloning Strategy

RustDB Manager provides three distinct, non-interchangeable copy mechanisms via `rustdb db copy` and `rustdb baseline build`:

### Mode 1: Deterministic Clean Rebuild (`--mode rebuild`)
- **How it works:**
  1. Drops (if `--force-recreate` on non-PROD) or creates a new empty database `[TARGET_DB]`.
  2. Initializes `[__rustdb_instance_meta]` with `origin_mode = 'SCRIPT_REBUILD'`.
  3. Executes all scripts from the validated Product Baseline (`schema/` then `master-data/`) in dependency order.
  4. If `--client <CLIENT_ID>` is specified, resolves the DAG of product migrations + client-specific migrations for that client and applies them in order.
- **Use case:** Creating a 100% clean development or test database for Client A without needing a multi-gigabyte backup from production and with zero risk of Client B contamination.

### Mode 2: Physical Clone via Backup & Restore (`--mode backup-restore`)
- **How it works:**
  1. Connects to the SQL Server instance.
  2. Executes `BACKUP DATABASE [@source_db] TO DISK = @backup_path WITH COPY_ONLY, INIT, COMPRESSION;` (using `COPY_ONLY` so it never disrupts the production differential/log backup chain).
  3. Queries `RESTORE FILELISTONLY FROM DISK = @backup_path` and `SERVERPROPERTY('InstanceDefaultDataPath')` / `SERVERPROPERTY('InstanceDefaultLogPath')`.
  4. Executes `RESTORE DATABASE [@target_db] FROM DISK = @backup_path WITH MOVE @logical_data TO @new_mdf_path, MOVE @logical_log TO @new_ldf_path, RECOVERY;`.
  5. Updates `[__rustdb_instance_meta]` in `@target_db` to record that it is a physical clone (`origin_mode = 'BACKUP_RESTORE'`, `source_provenance = @source_db`).
- **Use case:** Testing a migration package against a realistic snapshot of an existing database (including transactional data) on the same or local SQL Server instance.

### Mode 3: Schema-Only Live Clone (`--mode schema-clone`)
- **How it works:**
  1. Invokes `SqlPackage /Action:Extract` on `@source_db` to produce a temporary `.dacpac` artifact (schema only, or schema + selected seed tables).
  2. Invokes `SqlPackage /Action:Publish` onto a newly created `@target_db`.
  3. Stamps `[__rustdb_instance_meta]` with `origin_mode = 'DACPAC_CLONE'`.
- **Use case:** Cloning the exact current structure of a remote or local database without copying gigabytes of sensitive transactional data.

---

## 6. Structure Comparison & Drift Classification Strategy

When running `rustdb compare --source <DB_OR_BASELINE> --target <TARGET_DB> [--client <CLIENT_ID>]`, the Comparator Engine operates in three stages:

### Stage 1: Native Catalog Inspection (Layer 1 — Pure Rust, Zero External Dependencies)
Rust queries SQL Server catalog views in read-only mode to extract a normalized canonical model (`DatabaseSchemaInventory`):
- **Schemas:** `sys.schemas`
- **Tables & Columns:** `sys.tables`, `sys.columns`, `sys.types`, `sys.default_constraints`, `sys.identity_columns`, `sys.computed_columns`
- **Keys & Constraints:** `sys.key_constraints` (PK/UQ), `sys.foreign_keys`, `sys.foreign_key_columns`, `sys.check_constraints`
- **Indexes:** `sys.indexes`, `sys.index_columns`
- **Programmable Objects:** `sys.objects` + `sys.sql_modules` for Stored Procedures (`P`), Scalar/Table Functions (`FN`, `IF`, `TF`), Views (`V`), and Triggers (`TR`).
  - *Normalization:* Before hashing programmable object definitions (`definition` column in `sys.sql_modules`), Rust normalizes line endings (`\r\n` -> `\n`), trims leading/trailing whitespace, normalizes `CREATE OR ALTER` headers, and computes both a **raw SHA-256** and a **token-normalized SHA-256** (stripping comments and redundant whitespace).

### Stage 2: Optional Deep AST Diff via `SqlPackage` (Layer 2 — Hybrid Adapter)
If `--engine dacpac` is passed (or `SqlPackage` is detected in `PATH`), Rust extracts `.dacpac` files and runs `SqlPackage /Action:DeployReport` to catch subtle T-SQL AST nuances that catalog text normalization might flag.

### Stage 3: Context-Aware Drift Classification (The Core Rust Value)
Every detected difference (`MissingObject`, `ExtraObject`, `ModifiedDefinition`, `ColumnMismatch`) is cross-referenced against the repository's `MigrationManifest`s and the target database's `[__rustdb_schema_history]`:

| Classification Code | Meaning | Severity |
|---|---|---|
| `MATCH_BASELINE` | Object matches the declared Product Baseline version exactly. | `INFO` |
| `EXPLAINED_CLIENT_CHANGE` | Object differs from Baseline, but the difference is accounted for by an applied migration belonging to this client (`CC-A001`). | `OK` |
| `PENDING_KNOWN_MIGRATION` | Target DB is missing a change that exists in a valid pending migration for this client/product. | `NOTICE` |
| `CROSS_CLIENT_CONTAMINATION` | Target DB (Client A) contains a table, column, or SP definition belonging to another client's migration (`CC-B001`). | `CRITICAL` |
| `UNEXPLAINED_MANUAL_DRIFT` | Object was created or modified directly in the database without any matching migration or hash in `[__rustdb_schema_history]`. | `HIGH` |

---

## 7. Repository & Workspace Structure

```text
rust-db-manager/
├── Cargo.toml
├── README.md
├── docs/
│   ├── RustDB_Manager_Problema_Solucion_Viabilidad.md
│   └── RustDB_Manager_Architecture_And_MVP_Spec.md
├── config/
│   └── connections.example.toml
├── catalog/                           # Sample / Demo Product & Client Registry
│   ├── products/
│   │   └── sistema-erp/
│   │       ├── baselines/
│   │       │   └── v1/
│   │       │       ├── manifest.json
│   │       │       ├── schema/
│   │       │       │   ├── 001_tables.sql
│   │       │       │   ├── 002_views_and_sps.sql
│   │       │       └── master-data/
│   │       │           └── 001_seed_catalogs.sql
│   │       └── migrations/
│   └── clients/
│       ├── cliente-a/
│       │   ├── client.json
│       │   └── migrations/
│       │       └── 20261121_CC_A001/
│       │           ├── manifest.json
│       │           └── up.sql
│       └── cliente-b/
│           ├── client.json
│           └── migrations/
│               └── 20261121_CC_B001/
│                   ├── manifest.json
│                   └── up.sql
├── src/
│   ├── main.rs                        # CLI entry point
│   ├── lib.rs                         # Library root
│   ├── cli/                           # clap command definitions
│   │   └── mod.rs
│   ├── config/                        # Connection profiles & env vars
│   │   └── mod.rs
│   ├── domain/                        # Core types: Product, Client, Migration, Inventory, Diff
│   │   ├── mod.rs
│   │   ├── manifest.rs
│   │   ├── inventory.rs
│   │   └── diff.rs
│   ├── db/                            # SQL Server connection, T-SQL batch splitter, runner
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   ├── batch_splitter.rs          # Handles GO separators safely
│   │   └── history.rs                 # __rustdb_instance_meta & __rustdb_schema_history
│   ├── baseline/                      # Baseline validation & clean DB rebuild
│   │   └── mod.rs
│   ├── cloner/                        # Copy DB via Rebuild, Backup/Restore, or DACPAC
│   │   └── mod.rs
│   ├── comparator/                    # Layer 1 (sys.*) + Layer 2 (SqlPackage) + Classifier
│   │   ├── mod.rs
│   │   ├── catalog_inspector.rs
│   │   ├── sqlpackage_bridge.rs
│   │   └── classifier.rs
│   └── report/                        # Markdown & JSON report generators
│       └── mod.rs
└── tests/
    ├── batch_splitter_tests.rs
    ├── manifest_dag_tests.rs
    └── diff_classifier_tests.rs
```

---

## 8. Detailed Answers to the 36 Evaluation Questions (Section 14)

### A. General Viability (Q1–Q5)
1. **Technically viable in Rust?** Yes. Rust excels at deterministic file hashing, DAG validation, fast CLI tooling, and orchestrating SQL Server connections via `tiberius-ng`.
2. **What to build in Rust vs. delegate?** Build manifest validation, client isolation rules, `GO` batch execution, `BACKUP/RESTORE` orchestration, `sys.*` catalog inventory, and drift classification in Rust. Delegate deep AST schema diffing/DACPAC extraction to `SqlPackage` when installed.
3. **Value over Flyway/Liquibase/DbUp/DACPAC?** Yes. DACPAC is state-based and overwrites or fails when multiple clients diverge from a single base project; Flyway/DbUp are linear runners without first-class multi-client overlay guards or cross-client contamination reports.
4. **Own solution, integration, or hybrid?** **Hybrid.** Rust owns the domain model and control plane; SQL Server system catalogs and `SqlPackage` provide the inspection mechanics.
5. **Complexity and maintenance?** Low-to-moderate if scoped to the hybrid model (approx. 3,500–5,000 LOC in Rust for the complete core CLI).

### B. Database Versioning (Q6–Q11)
6. **Representing base version and migrations:** An immutable `BaselineManifest` (`v1`) containing ordered DDL + seed scripts with SHA-256 hashes, followed by a DAG of `MigrationManifest` units.
7. **Separating common vs. client migrations:** Every migration manifest declares `scope: "product" | "client"` and an optional `client_id`. The DAG planner rejects any plan where a migration with `client_id = "B"` is targeted at an instance registered to `client_id = "A"`.
8. **Tracking state per client and environment:** Dual tracking: Git holds the desired state (`client.json` per client), while each physical database holds actual state in `[dbo].[__rustdb_instance_meta]` and `[dbo].[__rustdb_schema_history]`.
9. **Out-of-order migrations and divergent branches:** Migrations use unique IDs (`20261121_CC_A001`) and explicit `dependencies: [...]` rather than relying on sequential integers (`018_...`), allowing parallel branches to merge without renumbering files.
10. **Detecting modified applied scripts:** Comparing the SHA-256 stored in `[__rustdb_schema_history].[checksum_sha256]` against the current file on disk. Any mismatch halts execution with a `ChecksumMismatchError`.
11. **Handling manual changes in a DB:** Detected by `rustdb compare` as `UNEXPLAINED_MANUAL_DRIFT`. Must be resolved either by formalizing the change into a new tracked migration or creating a corrective forward migration.

### C. Copies & Reconstruction (Q12–Q16)
12. **Best copy mechanism in SQL Server:** Both are required for different jobs: **Rebuild from scripts** for clean dev instances and client switching; **Backup/Restore (`WITH COPY_ONLY`, `WITH MOVE`)** for testing migrations against realistic client states.
13. **New instance with structure + master data only:** Use `rustdb baseline build` (or `rustdb db copy --mode rebuild`), which executes only `schema/` and `master-data/` scripts plus approved migrations.
14. **Preventing master data from overwriting client edits:** Classify master data tables in the manifest as `system_owned` (full `MERGE`) vs. `client_extensible` (insert-only via `MERGE ... WHEN NOT MATCHED THEN INSERT`, never overwriting existing rows).
15. **Verifying a reconstruction matches expected version:** Immediately after `baseline build`, automatically run the `CatalogInspector` and compare the resulting object inventory hash against the expected baseline inventory hash.
16. **Recording provenance and trust level:** Written directly into `[dbo].[__rustdb_instance_meta]` (`origin_mode`, `source_provenance`, `trust_level`) and emitted in a sidecar JSON receipt.

### D. Structure Comparison (Q17–Q21)
17. **Reliable comparison tools for SQL Server:** SQL Server catalog views (`sys.*`) for structural metadata and module definitions + Microsoft `SqlPackage` (DacFx) for semantic AST comparison.
18. **Can `SqlPackage`/DACPAC be used from Rust?** Yes, `SqlPackage` is a standalone cross-platform CLI installable via `dotnet tool install -g microsoft.sqlpackage` and invocable via `tokio::process::Command` with XML output (`DeployReport.xml`).
19. **What a basic comparator misses:** Partition schemes, column collations, index fill factors, `INCLUDE` columns on non-clustered indexes, extended properties, and whitespace/comment differences inside Stored Procedures. (Our Layer 1 catalog query explicitly includes collations, `is_included_column`, filter definitions, and normalized SQL module hashes; Layer 2 `SqlPackage` covers the rest).
20. **Detecting differences in SPs, functions, views, triggers:** Querying `sys.sql_modules.definition` joined with `sys.objects`, stripping comments and normalizing whitespace before computing SHA-256 and generating unified line diffs (`similar` crate in Rust).
21. **Distinguishing expected customization vs. accidental drift:** Cross-referencing the changed object's `[schema].[name]` against the `affected_objects` declared in the client's applied migrations (`EXPLAINED_CLIENT_CHANGE`) vs. other clients' migrations (`CROSS_CLIENT_CONTAMINATION`) vs. unknown (`UNEXPLAINED_MANUAL_DRIFT`).

### E. Reversion & Recovery (Q22–Q26)
22. **Keep `DOWN` scripts for all migrations?** No. Make `DOWN` optional documentation only. Rely on pre-deployment `BACKUP DATABASE ... WITH COPY_ONLY` and forward corrective migrations.
23. **Irreversible DDL/DML in SQL Server:** `DROP TABLE`, `DROP COLUMN`, `TRUNCATE TABLE`, narrowing data types (`NVARCHAR(MAX)` -> `NVARCHAR(50)`), and lossy `UPDATE`/`DELETE` statements.
24. **Handling destructive migrations:** Require `"destructive": true` in the migration manifest and an explicit `--allow-destructive` confirmation flag during deployment.
25. **Pre-deployment backup and validation:** Before applying any package to UAT/PROD, take an automatic `COPY_ONLY` backup (or verify a recent backup timestamp), verify `[__rustdb_instance_meta]` matches the target `client_id`, and run a pre-flight drift check.
26. **Handling partial execution failures:** Each migration script executes inside a T-SQL transaction (`BEGIN TRY / BEGIN TRAN ... COMMIT TRAN / END TRY BEGIN CATCH ... ROLLBACK TRAN`). If a multi-migration deployment fails on migration #3, migrations #1 and #2 remain recorded in `[__rustdb_schema_history]`, and #3 rolls back cleanly so execution can resume after fixing #3.

### F. Security (Q27–Q30)
27. **Minimum permissions:**
   - *Inspect/Compare (Read-Only):* `VIEW DEFINITION` + `SELECT` on `[dbo].[__rustdb_*]`.
   - *Deploy Migrations:* `db_ddladmin`, `db_datareader`, `db_datawriter`.
   - *Create/Clone Databases:* `dbcreator` (or `BACKUP DATABASE` / `RESTORE` permissions on the instance).
28. **Storing credentials:** Never store passwords in `connections.toml` or Git. Reference environment variables (`password_env = "RUSTDB_SQL_PASSWORD"`) or Windows Integrated Authentication.
29. **Preventing sensitive data exposure when cloning:** Default developer workflows to `--mode rebuild` (zero transactional data). Require explicit `--include-data` flag and audit logging when using `--mode backup-restore`.
30. **Audit trail per operation:** Record execution ID, timestamp UTC, SQL login (`SUSER_SNAME()`), workstation (`HOST_NAME()`), migration ID, SHA-256 checksum, duration, and status.

### G. Design & Roadmap (Q31–Q36)
31. **Minimum data model:** `BaselineManifest`, `ClientManifest`, `MigrationManifest`, `DatabaseSchemaInventory`, `ComparisonReport`, plus the two SQL tracking tables (`__rustdb_instance_meta`, `__rustdb_schema_history`).
32. **Recommended repository structure:** See Section 7 above.
33. **Smallest valuable MVP:** **MVP 0.1 + 0.1.5 (Copy/Rebuild + Compare + Client Contamination Detection)**—detailed in Section 9 below.
34. **Essential automated tests:** Unit tests for T-SQL `GO` batch splitting (ignoring `GO` inside comments/strings), SHA-256 manifest verification, DAG cycle/scope validation, and drift/contamination classification; plus integration tests against a local SQL Server instance.
35. **Features to postpone:** GUI (Tauri/React), multi-engine (MySQL/PostgreSQL), automatic migration script generation from raw diffs, and automated `DOWN` execution.
36. **Project-killing risks:** Attempting to write a custom T-SQL AST parser from scratch, or allowing teams to bypass the manifest repository and keep editing production stored procedures manually without drift checks.

---

## 9. MVP 0.1 Execution Plan (Actionable Spec)

To directly satisfy your two core requirements (**1. Copying/Rebuilding a DB** and **2. Comparing Structure with Client Drift Detection**), MVP 0.1 will deliver a working Rust CLI (`rustdb`) with the following commands and capabilities:

### 9.1 CLI Command Specification

```bash
# 1. Test connection to SQL Server profile
rustdb connection test --profile local

# 2. Validate baseline & client manifests (hashes, DAG dependencies, client scope rules)
rustdb baseline validate --catalog ./catalog --product sistema-erp --version v1

# 3. Build / Copy a clean database from baseline (+ optional client overlay)
rustdb baseline build \
  --catalog ./catalog \
  --product sistema-erp \
  --version v1 \
  --client cliente-a \
  --profile local \
  --database DEMO_CLIENTE_A

# 4. Copy an existing database (Mode A: Physical Backup/Restore WITH MOVE, Mode B: Clean Rebuild)
rustdb db copy \
  --profile local \
  --source DEMO_PROD_BASE \
  --target DEMO_CLONE_TEST \
  --mode backup-restore

# 5. Inspect a live database and export its full schema inventory (JSON)
rustdb schema inspect \
  --profile local \
  --database DEMO_CLIENTE_A \
  --output ./reports/inventory_cliente_a.json

# 6. Compare two databases (or a baseline vs. target DB) and classify drift/contamination
rustdb compare \
  --profile local \
  --source DEMO_PROD_BASE \
  --target DEMO_CLIENTE_A \
  --catalog ./catalog \
  --client cliente-a \
  --format markdown \
  --output ./reports/diff_cliente_a.md
```

### 9.2 Implementation Phases & Tasks

#### Phase 1: Project Scaffold & Core Domain (`src/domain/`, `src/config/`)
- [ ] Initialize Cargo project (`cargo init`) with verified dependencies (`clap`, `tokio`, `tiberius-ng`, `tokio-util`, `serde`, `serde_json`, `toml`, `sha2`, `hex`, `petgraph`, `similar`, `thiserror`, `anyhow`, `tracing`).
- [ ] Implement connection profile loader (`config/connections.example.toml` + environment variable secret resolution).
- [ ] Implement `BaselineManifest`, `ClientManifest`, and `MigrationManifest` structs with SHA-256 file verification and DAG dependency resolution (`petgraph`).
- [ ] Implement the **Cross-Client Scope Guard** (unit-tested rule ensuring a migration for `CLIENTE_B` cannot be scheduled for `CLIENTE_A`).

#### Phase 2: T-SQL Batch Splitter, DB Runner & Cloner (`src/db/`, `src/baseline/`, `src/cloner/`)
- [ ] Implement a state-machine T-SQL `GO` batch splitter that safely splits scripts on standalone `GO` lines while ignoring `GO` inside `/* ... */` block comments, `-- ...` line comments, `'...'` string literals, and `[...]` identifiers.
- [ ] Implement SQL Server client wrapper (`src/db/client.rs`) and tracking table bootstrapper (`__rustdb_instance_meta` and `__rustdb_schema_history`).
- [ ] Implement `rustdb baseline build`: creates target DB, runs baseline schema & master-data scripts, applies client-scoped migrations, and records SHA-256 history.
- [ ] Implement `rustdb db copy`: supports `--mode rebuild` and `--mode backup-restore` (automated `BACKUP ... WITH COPY_ONLY` + `RESTORE FILELISTONLY` + `RESTORE ... WITH MOVE`).

#### Phase 3: Dual-Layer Comparator & Drift Classifier (`src/comparator/`, `src/report/`)
- [ ] Implement Layer 1 `CatalogInspector` querying SQL Server `sys.*` catalogs for tables, columns (types, length, precision, scale, nullability, defaults, identity), primary keys, foreign keys, indexes (including `INCLUDE` columns and filter predicates), views, stored procedures, functions, and triggers.
- [ ] Implement T-SQL module normalizer (normalizing line endings, stripping comments, computing raw and semantic SHA-256 hashes, and generating unified text diffs via `similar`).
- [ ] Implement Layer 2 `SqlPackageBridge` (optional execution of `SqlPackage /Action:Extract` & `/Action:DeployReport` when `SqlPackage` is present).
- [ ] Implement `DriftClassifier` that joins structural differences with `MigrationManifest.affected_objects` and `[__rustdb_schema_history]` to label every difference as `EXPLAINED_CLIENT_CHANGE`, `CROSS_CLIENT_CONTAMINATION`, or `UNEXPLAINED_MANUAL_DRIFT`.
- [ ] Implement Markdown and JSON report generators (`src/report/`).

#### Phase 4: Demo Catalog & Verification Suite (`catalog/`, `tests/`)
- [ ] Create realistic demo catalog in `catalog/` for `sistema-erp@v1`, `cliente-a` (`CC-A001`, `CC-A002`), and `cliente-b` (`CC-B001` Digiflow table/SP).
- [ ] Write automated unit tests for:
  1. T-SQL `GO` splitter edge cases.
  2. Manifest SHA-256 tamper detection.
  3. DAG sorting and cross-client contamination rejection.
  4. Schema diff & contamination classification (`DEMO_PROD_BASE` vs `DEMO_CLIENTE_A` tainted with `CC-B001` + manual hotfix).

---

## 10. Effort & Complexity Estimation

| Module | Complexity | Primary Uncertainty / Risk | Mitigation |
|---|---|---|---|
| **Manifests, Hashes & DAG Engine** | Low | Legacy scripts having undocumented implicit order dependencies. | Enforce explicit `dependencies` array in `MigrationManifest` and validate during `baseline build`. |
| **T-SQL `GO` Splitter & Runner** | Medium | Scripts containing `GO` inside comments or strings, or `GO <count>`. | State-machine lexer tested against real SQL Server DDL/SP scripts. |
| **DB Copy (`BACKUP/RESTORE` & Rebuild)** | Medium | SQL Server service account file permissions for temporary `.bak` files on remote servers. | Query `SERVERPROPERTY('InstanceDefaultBackupPath')` and `InstanceDefaultDataPath` dynamically so SQL Server writes to its own authorized directories. |
| **Catalog Inspector & Diff Classifier** | Medium-High | Formatting noise in Stored Procedures causing false diffs. | Compute both raw SHA-256 and comment/whitespace-normalized SHA-256 + unified diff view. |
| **`SqlPackage` Bridge** | Low | `SqlPackage` CLI not installed on every machine. | Graceful degradation: Layer 1 (`sys.*` native inspector) always works out-of-the-box; Layer 2 (`SqlPackage`) activates when installed or requested via `--engine dacpac`. |

---

## 11. Pending Decisions for Stakeholder Confirmation

Before writing the Rust code for MVP 0.1, only one operational detail needs confirmation when you are ready to run live tests:
1. **Local SQL Server Test Instance:** Do you already have a local SQL Server instance (or Docker container) running where we will execute the live integration tests for `DEMO_PROD_BASE` and `DEMO_DEV`, or should we include a `docker-compose.yml` with SQL Server 2022 Express/Developer in the repository?
