use crate::domain::diff::{ComparisonReport, DriftClassification};
use crate::domain::error::DomainError;

pub fn render_report_json(report: &ComparisonReport) -> Result<String, DomainError> {
    serde_json::to_string_pretty(report)
        .map_err(|e| DomainError::DatabaseError(format!("Failed to serialize JSON report: {e}")))
}

pub fn render_report_markdown(report: &ComparisonReport) -> String {
    let mut md = String::new();

    md.push_str("# RustDB Manager — Schema Comparison & Drift Report\n\n");
    md.push_str(&format!(
        "- **Source Database:** `{}`\n",
        report.source_database
    ));
    md.push_str(&format!(
        "- **Target Database:** `{}`\n",
        report.target_database
    ));
    md.push_str(&format!(
        "- **Target Client Context:** `{}`\n",
        report.target_client_id.as_deref().unwrap_or("NONE (Baseline)")
    ));
    md.push_str(&format!(
        "- **Comparison Engine:** `{}`\n",
        report.comparison_engine
    ));
    md.push_str(&format!(
        "- **Compared At (UTC):** `{}`\n",
        report.compared_at_utc.to_rfc3339()
    ));
    md.push_str(&format!(
        "- **Deployment Gate Status:** {}\n\n",
        if report.summary.is_clean_for_deployment {
            "**PASS (Clean / All Differences Explained)**"
        } else {
            "**BLOCKED (Contamination or Unexplained Drift Detected)**"
        }
    ));

    md.push_str("## 1. Executive Summary\n\n");
    md.push_str("| Metric | Count |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!(
        "| Total Detected Differences | **{}** |\n",
        report.summary.total_differences
    ));
    md.push_str(&format!(
        "| Explained by Authorized Migrations (`EXPLAINED_BY_MIGRATION`) | `{}` |\n",
        report.summary.explained_changes
    ));
    md.push_str(&format!(
        "| Cross-Client Contaminations (`CROSS_CLIENT_CONTAMINATION`) | **`{}`** |\n",
        report.summary.cross_client_contaminations
    ));
    md.push_str(&format!(
        "| Unexplained Manual Drifts (`UNEXPLAINED_MANUAL_DRIFT`) | **`{}`** |\n",
        report.summary.unexplained_manual_drifts
    ));
    md.push_str(&format!(
        "| Pending Known Migrations (`PENDING_KNOWN_MIGRATION`) | `{}` |\n",
        report.summary.pending_migrations
    ));
    md.push_str(&format!(
        "| Formatting / Comment-Only Differences (`FORMATTING_ONLY`) | `{}` |\n\n",
        report.summary.formatting_only_diffs
    ));

    if report.differences.is_empty() {
        md.push_str("> **No differences found.** Source and Target schemas are 100% identical.\n");
        return md;
    }

    md.push_str("## 2. Detailed Object Findings\n\n");

    for (idx, diff) in report.differences.iter().enumerate() {
        let badge = match diff.classification {
            DriftClassification::CrossClientContamination => "[CRITICAL - CROSS-CLIENT CONTAMINATION]",
            DriftClassification::UnexplainedManualDrift => "[HIGH - UNEXPLAINED MANUAL DRIFT]",
            DriftClassification::PendingKnownMigration => "[NOTICE - PENDING MIGRATION]",
            DriftClassification::ExplainedByMigration => "[OK - EXPLAINED BY MIGRATION]",
            DriftClassification::FormattingOnly => "[INFO - FORMATTING ONLY]",
        };

        md.push_str(&format!(
            "### {}. {} `{}` ({})\n\n",
            idx + 1,
            badge,
            diff.object_name,
            diff.object_type
        ));
        md.push_str(&format!("- **Summary:** {}\n", diff.summary));
        md.push_str(&format!("- **Diff Kind:** `{:?}`\n", diff.diff_kind));
        md.push_str(&format!("- **Severity:** `{:?}`\n", diff.severity));
        if let Some(mig_id) = &diff.attributed_migration_id {
            md.push_str(&format!("- **Attributed Migration:** `{mig_id}`\n"));
        }
        if let Some(cid) = &diff.attributed_client_id {
            md.push_str(&format!("- **Attributed Client:** `{cid}`\n"));
        }

        if !diff.details.is_empty() {
            md.push_str("- **Details:**\n");
            for d in &diff.details {
                md.push_str(&format!("  - {d}\n"));
            }
        }

        if let Some(udiff) = &diff.unified_diff {
            md.push_str("\n```diff\n");
            md.push_str(udiff.trim_end());
            md.push_str("\n```\n");
        }
        md.push('\n');
    }

    md
}
