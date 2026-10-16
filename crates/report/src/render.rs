use crate::models::MigrationReport;
use state_engine::NestedChange;

/// Formats a migration report into a human-readable text string.
pub fn render_text(report: &MigrationReport) -> String {
    let mut out = String::new();
    out.push_str(
        "================================================================================\n",
    );
    out.push_str("                    SOROBAN MIGRATION SIMULATION REPORT\n");
    out.push_str(
        "================================================================================\n",
    );
    out.push_str(&format!("Contract ID:        {}\n", report.contract_id));
    out.push_str(&format!(
        "Migration Function: {}\n",
        report.migration_function
    ));
    out.push_str(&format!("Execution Status:   {}\n", report.status()));
    out.push_str(&format!("Duration:           {} ms\n", report.duration_ms));

    if let Some(err) = &report.error {
        out.push_str(&format!("Error:              {}\n", err));
    }

    out.push_str("\nState Change Summary:\n");
    out.push_str(&format!("  Added Entries:    {}\n", report.summary.added));
    out.push_str(&format!("  Removed Entries:  {}\n", report.summary.removed));
    out.push_str(&format!(
        "  Modified Entries: {}\n",
        report.summary.modified
    ));
    out.push_str(&format!(
        "  Unchanged:        {}\n",
        report.summary.unchanged
    ));
    out.push_str(&format!(
        "  Total Changes:    {}\n",
        report.summary.total_changes()
    ));

    out.push_str("\nDetailed State Differences:\n");

    if report.changes.added.is_empty()
        && report.changes.removed.is_empty()
        && report.changes.modified.is_empty()
    {
        out.push_str("  (No state changes detected)\n");
    } else {
        if !report.changes.added.is_empty() {
            out.push_str(&format!(
                "  [ADDED ENTRIES] ({})\n",
                report.changes.added.len()
            ));
            for entry in &report.changes.added {
                out.push_str(&format!(
                    "    + Durability: {:?}, Key: {:?}\n",
                    entry.durability, entry.key
                ));
                out.push_str(&format!("      Value: {:?}\n", entry.value));
            }
        }

        if !report.changes.removed.is_empty() {
            out.push_str(&format!(
                "  [REMOVED ENTRIES] ({})\n",
                report.changes.removed.len()
            ));
            for entry in &report.changes.removed {
                out.push_str(&format!(
                    "    - Durability: {:?}, Key: {:?}\n",
                    entry.durability, entry.key
                ));
                out.push_str(&format!("      Value: {:?}\n", entry.value));
            }
        }

        if !report.changes.modified.is_empty() {
            out.push_str(&format!(
                "  [MODIFIED ENTRIES] ({})\n",
                report.changes.modified.len()
            ));
            for entry in &report.changes.modified {
                out.push_str(&format!("    * Key: {:?}\n", entry.key));
                if entry.nested_changes.is_empty() {
                    out.push_str(&format!("      Before: {:?}\n", entry.before));
                    out.push_str(&format!("      After:  {:?}\n", entry.after));
                } else {
                    for nc in &entry.nested_changes {
                        match nc {
                            NestedChange::FieldAdded { field_name, value } => {
                                out.push_str(&format!(
                                    "      + Field Added: '{}' = {:?}\n",
                                    field_name, value
                                ));
                            }
                            NestedChange::FieldRemoved { field_name, value } => {
                                out.push_str(&format!(
                                    "      - Field Removed: '{}' (was {:?})\n",
                                    field_name, value
                                ));
                            }
                            NestedChange::FieldModified {
                                field_name,
                                before,
                                after,
                            } => {
                                out.push_str(&format!(
                                    "      * Field Modified: '{}': {:?} -> {:?}\n",
                                    field_name, before, after
                                ));
                            }
                            NestedChange::ItemAdded { index, value } => {
                                out.push_str(&format!(
                                    "      + Item Added at index {}: {:?}\n",
                                    index, value
                                ));
                            }
                            NestedChange::ItemRemoved { index, value } => {
                                out.push_str(&format!(
                                    "      - Item Removed at index {}: {:?}\n",
                                    index, value
                                ));
                            }
                            NestedChange::ItemModified {
                                index,
                                before,
                                after,
                            } => {
                                out.push_str(&format!(
                                    "      * Item Modified at index {}: {:?} -> {:?}\n",
                                    index, before, after
                                ));
                            }
                            NestedChange::MapEntryModified { key, before, after } => {
                                out.push_str(&format!(
                                    "      * Map Entry Modified for key {:?}: {:?} -> {:?}\n",
                                    key, before, after
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    out.push_str(
        "================================================================================\n",
    );
    out
}

/// Formats a migration report into GitHub-flavored Markdown.
pub fn render_markdown(report: &MigrationReport) -> String {
    let mut out = String::new();
    out.push_str("# Soroban Migration Simulation Report\n\n");

    out.push_str("## Execution Metadata\n\n");
    out.push_str("| Property | Value |\n");
    out.push_str("|---|---|\n");
    out.push_str(&format!("| **Contract ID** | `{}` |\n", report.contract_id));
    out.push_str(&format!(
        "| **Migration Function** | `{}` |\n",
        report.migration_function
    ));
    out.push_str(&format!(
        "| **Status** | {} |\n",
        if report.success {
            "✅ **SUCCESS**"
        } else {
            "❌ **FAILED**"
        }
    ));
    out.push_str(&format!("| **Duration** | {} ms |\n", report.duration_ms));

    if let Some(err) = &report.error {
        out.push_str(&format!("| **Error** | `{}` |\n", err));
    }

    out.push_str("\n## State Change Summary\n\n");
    out.push_str("| Metric | Count |\n");
    out.push_str("|---|---|\n");
    out.push_str(&format!("| Added Entries | {} |\n", report.summary.added));
    out.push_str(&format!(
        "| Removed Entries | {} |\n",
        report.summary.removed
    ));
    out.push_str(&format!(
        "| Modified Entries | {} |\n",
        report.summary.modified
    ));
    out.push_str(&format!(
        "| Unchanged Entries | {} |\n",
        report.summary.unchanged
    ));
    out.push_str(&format!(
        "| **Total Changes** | **{}** |\n",
        report.summary.total_changes()
    ));

    out.push_str("\n## State Differences\n\n");

    if report.changes.added.is_empty()
        && report.changes.removed.is_empty()
        && report.changes.modified.is_empty()
    {
        out.push_str("_No state changes detected._\n");
    } else {
        if !report.changes.added.is_empty() {
            out.push_str(&format!(
                "### Added Entries ({})\n\n",
                report.changes.added.len()
            ));
            for entry in &report.changes.added {
                out.push_str(&format!(
                    "- **{:?}** Key: `{:?}` -> Value: `{:?}`\n",
                    entry.durability, entry.key, entry.value
                ));
            }
            out.push('\n');
        }

        if !report.changes.removed.is_empty() {
            out.push_str(&format!(
                "### Removed Entries ({})\n\n",
                report.changes.removed.len()
            ));
            for entry in &report.changes.removed {
                out.push_str(&format!(
                    "- **{:?}** Key: `{:?}` -> Value: `{:?}`\n",
                    entry.durability, entry.key, entry.value
                ));
            }
            out.push('\n');
        }

        if !report.changes.modified.is_empty() {
            out.push_str(&format!(
                "### Modified Entries ({})\n\n",
                report.changes.modified.len()
            ));
            for entry in &report.changes.modified {
                out.push_str(&format!("- Key: `{:?}`\n", entry.key));
                if entry.nested_changes.is_empty() {
                    out.push_str(&format!("  - Before: `{:?}`\n", entry.before));
                    out.push_str(&format!("  - After: `{:?}`\n", entry.after));
                } else {
                    for nc in &entry.nested_changes {
                        match nc {
                            NestedChange::FieldAdded { field_name, value } => {
                                out.push_str(&format!(
                                    "  - ➕ Field Added: `{}` = `{:?}`\n",
                                    field_name, value
                                ));
                            }
                            NestedChange::FieldRemoved { field_name, value } => {
                                out.push_str(&format!(
                                    "  - ➖ Field Removed: `{}` (was `{:?}`)\n",
                                    field_name, value
                                ));
                            }
                            NestedChange::FieldModified {
                                field_name,
                                before,
                                after,
                            } => {
                                out.push_str(&format!(
                                    "  - ✏️ Field Modified: `{}`: `{:?}` -> `{:?}`\n",
                                    field_name, before, after
                                ));
                            }
                            NestedChange::ItemAdded { index, value } => {
                                out.push_str(&format!(
                                    "  - ➕ Item Added [{}]: `{:?}`\n",
                                    index, value
                                ));
                            }
                            NestedChange::ItemRemoved { index, value } => {
                                out.push_str(&format!(
                                    "  - ➖ Item Removed [{}]: `{:?}`\n",
                                    index, value
                                ));
                            }
                            NestedChange::ItemModified {
                                index,
                                before,
                                after,
                            } => {
                                out.push_str(&format!(
                                    "  - ✏️ Item Modified [{}]: `{:?}` -> `{:?}`\n",
                                    index, before, after
                                ));
                            }
                            NestedChange::MapEntryModified { key, before, after } => {
                                out.push_str(&format!(
                                    "  - ✏️ Map Entry `{:?}`: `{:?}` -> `{:?}`\n",
                                    key, before, after
                                ));
                            }
                        }
                    }
                }
            }
            out.push('\n');
        }
    }

    out
}
