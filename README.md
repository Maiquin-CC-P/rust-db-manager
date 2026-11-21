# RustDB Manager (MVP 0.1)

Multi-client database baseline builder, database cloner (`Rebuild`, `Backup/Restore WITH MOVE`, `DACPAC Schema Clone`), migration DAG validator, and context-aware schema drift comparator for **SQL Server** (built with **Hexagonal Architecture** ready for **MySQL** in MVP 0.2).

---

## 1. Key Capabilities

1. **Product Baselines & Client Isolation (`catalog/`):**
   - Keeps an immutable, SHA-256-verified baseline for each product (`sistema-erp@v1`).
   - Separates common product migrations (`scope: "product"`) from client-exclusive customizations (`scope: "client"`, `client_id: "cliente-a"`).
   - **Hard Scope Guard:** Strictly blocks any migration belonging to `cliente-b` from ever being scheduled or applied to `cliente-a`.
2. **Three Database Copy Modalities (`rustdb db copy` & `rustdb baseline build`):**
   - `--mode rebuild`: Creates a 100% clean database from baseline scripts + master data + approved client migrations (zero cross-client residue, zero sensitive transactional data).
   - `--mode backup-restore`: Executes a physical `BACKUP DATABASE ... WITH COPY_ONLY` and `RESTORE DATABASE ... WITH MOVE` on the SQL Server instance, automatically computing `.mdf`/`.ldf` file paths and stamping `[__rustdb_instance_meta]`.
   - `--mode schema-clone`: Extracts and publishes a schema-only clone via Microsoft `SqlPackage` (`.dacpac`).
3. **Structure Comparison & Contamination Detection (`rustdb compare`):**
   - Inspects Schemas, Tables, Columns (types, lengths, precision, scale, nullability, defaults, identity), Primary Keys, Indexes (`INCLUDE` columns & filters), Foreign Keys, Check Constraints, Views, Stored Procedures, Functions, and Triggers.
   - Normalizes T-SQL definitions (stripping comments and redundant whitespace) to avoid formatting noise and generates unified code diffs.
   - Classifies every difference into:
     - `EXPLAINED_BY_MIGRATION` (authorized change for this client),
     - `CROSS_CLIENT_CONTAMINATION` (CRITICAL alert: object belongs to another client's migration!),
     - `UNEXPLAINED_MANUAL_DRIFT` (HIGH alert: undocumented manual alteration in DB),
     - `PENDING_KNOWN_MIGRATION`, or
     - `FORMATTING_ONLY`.

---

## 2. Prerequisites

- **Rust Toolchain (`rustup` / `cargo`):**
  If Rust is not installed on your Windows machine yet, install it with:
  ```powershell
  winget install Rustlang.Rustup
  ```
- **SQL Server:** Local instance (already detected on your machine via `SQLCMD.EXE`).
- **Optional (for Layer 2 Hybrid DACPAC / `--mode schema-clone`):**
  ```powershell
  dotnet tool install -g microsoft.sqlpackage
  ```

---

## 3. Quick Start & Demo Walkthrough

### Step 1: Run Offline Unit Tests & Validate the Demo Catalog
```powershell
cargo test
cargo run -- baseline validate --catalog ./tests/fixtures/catalog --product sistema-erp --version v1 --client cliente-a
```

### Step 2: Configure Local SQL Server Connection
Edit `config/connections.example.toml` (or copy it to `config/connections.toml`) and set your password environment variable (or use `--profile local_windows_auth` for Windows Integrated Authentication):
```powershell
$env:RUSTDB_SQL_PASSWORD = "YourSqlPassword"
cargo run -- connection test --profile local_windows_auth
```

### Step 3: Build a Clean Product Baseline & Clean Client A Instance
```powershell
# Build pure product baseline DEMO_PROD_BASE
cargo run -- baseline build `
  --catalog ./tests/fixtures/catalog `
  --product sistema-erp `
  --version v1 `
  --profile local_windows_auth `
  --database DEMO_PROD_BASE `
  --drop-if-exists

# Build clean Client A database (Baseline v1 + CC-001 + CC-A001 + CC-A002)
cargo run -- baseline build `
  --catalog ./tests/fixtures/catalog `
  --product sistema-erp `
  --version v1 `
  --client cliente-a `
  --profile local_windows_auth `
  --database DEMO_CLIENTE_A `
  --drop-if-exists
```

### Step 4: Copy a Database (Physical Backup/Restore or Clean Rebuild)
```powershell
# Physical clone via BACKUP WITH COPY_ONLY + RESTORE WITH MOVE
cargo run -- db copy `
  --profile local_windows_auth `
  --mode backup-restore `
  --source DEMO_CLIENTE_A `
  --target DEMO_CLIENTE_A_CLONE
```

### Step 5: Compare Structure & Generate Markdown/JSON Drift Report
```powershell
cargo run -- compare `
  --profile local_windows_auth `
  --source DEMO_PROD_BASE `
  --target DEMO_CLIENTE_A `
  --catalog ./tests/fixtures/catalog `
  --product sistema-erp `
  --client cliente-a `
  --format markdown `
  --output ./reports/diff_cliente_a.md
```
