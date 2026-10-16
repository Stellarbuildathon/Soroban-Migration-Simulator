use serde::{Deserialize, Serialize};
use state_engine::{NestedChange, StateDiff, StateValue};

use crate::error::ReportError;
use crate::summary::DiffSummary;

/// Detailed field-level change extracted from modified entries.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// Storage key of the entry containing the field.
    pub entry_key: StateValue,
    /// Name of the struct field.
    pub field_name: String,
    /// Specific change type (Added, Removed, or Modified).
    pub change_type: FieldChangeType,
}

/// The nature of a field-level change.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FieldChangeType {
    /// Field was newly added to the struct.
    Added { value: StateValue },
    /// Field was removed from the struct.
    Removed { value: StateValue },
    /// Field value was modified from `before` to `after`.
    Modified {
        before: StateValue,
        after: StateValue,
    },
}

/// Structured migration report that converts migration results into a developer-readable format.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Unique contract identifier that was migrated.
    pub contract_id: String,

    /// Name of the migration function executed.
    pub migration_function: String,

    /// Execution status: true if migration succeeded without error.
    pub success: bool,

    /// Execution duration in milliseconds.
    pub duration_ms: u64,

    /// Summary of state changes (counts of added, removed, modified, unchanged).
    pub summary: DiffSummary,

    /// Detailed state differences between pre-migration and post-migration states.
    pub changes: StateDiff,

    /// Optional error message if the migration failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl MigrationReport {
    /// Create a new report with automatically computed summary from `changes`.
    pub fn new(
        contract_id: impl Into<String>,
        migration_function: impl Into<String>,
        success: bool,
        duration_ms: u64,
        changes: StateDiff,
    ) -> Self {
        let summary = DiffSummary::from(&changes);
        Self {
            contract_id: contract_id.into(),
            migration_function: migration_function.into(),
            success,
            duration_ms,
            summary,
            changes,
            error: None,
        }
    }

    /// Create a new report with an explicitly provided summary and changes.
    pub fn with_summary(
        contract_id: impl Into<String>,
        migration_function: impl Into<String>,
        success: bool,
        duration_ms: u64,
        summary: DiffSummary,
        changes: StateDiff,
    ) -> Self {
        Self {
            contract_id: contract_id.into(),
            migration_function: migration_function.into(),
            success,
            duration_ms,
            summary,
            changes,
            error: None,
        }
    }

    /// Attach an error message to this report.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Create a migration report from a `MigrationResult` and execution metadata.
    pub fn from_result(
        contract_id: impl Into<String>,
        migration_function: impl Into<String>,
        result: &migration_engine::MigrationResult,
    ) -> Self {
        let diff = result.pre_state.diff(&result.post_state);
        let summary = DiffSummary::from(&diff);
        let total_us = result.execution_info.pre_capture_us
            + result.execution_info.migration_us
            + result.execution_info.post_capture_us;
        let duration_ms = total_us / 1000;

        Self {
            contract_id: contract_id.into(),
            migration_function: migration_function.into(),
            success: result.success,
            duration_ms,
            summary,
            changes: diff,
            error: result.error.clone(),
        }
    }

    /// Create a migration report directly from `MigrationInput` and `MigrationResult`.
    pub fn from_execution(
        input: &migration_engine::MigrationInput,
        result: &migration_engine::MigrationResult,
    ) -> Self {
        Self::from_result(&input.contract_id, &input.migration_fn, result)
    }

    /// Return a human-readable execution status string ("SUCCESS" or "FAILED").
    pub fn status(&self) -> &'static str {
        if self.success {
            "SUCCESS"
        } else {
            "FAILED"
        }
    }

    /// Returns true if any state modifications, additions, or deletions occurred.
    pub fn has_changes(&self) -> bool {
        self.summary.has_changes()
    }

    /// Collect all field additions across modified struct entries.
    pub fn added_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.modified {
            for change in &entry.nested_changes {
                if let NestedChange::FieldAdded { field_name, value } = change {
                    fields.push(FieldDiff {
                        entry_key: entry.key.clone(),
                        field_name: field_name.clone(),
                        change_type: FieldChangeType::Added {
                            value: value.clone(),
                        },
                    });
                }
            }
        }
        fields
    }

    /// Collect all field removals across modified struct entries.
    pub fn removed_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.modified {
            for change in &entry.nested_changes {
                if let NestedChange::FieldRemoved { field_name, value } = change {
                    fields.push(FieldDiff {
                        entry_key: entry.key.clone(),
                        field_name: field_name.clone(),
                        change_type: FieldChangeType::Removed {
                            value: value.clone(),
                        },
                    });
                }
            }
        }
        fields
    }

    /// Collect all field modifications across modified struct entries.
    pub fn modified_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.modified {
            for change in &entry.nested_changes {
                if let NestedChange::FieldModified {
                    field_name,
                    before,
                    after,
                } = change
                {
                    fields.push(FieldDiff {
                        entry_key: entry.key.clone(),
                        field_name: field_name.clone(),
                        change_type: FieldChangeType::Modified {
                            before: before.clone(),
                            after: after.clone(),
                        },
                    });
                }
            }
        }
        fields
    }

    /// Collect all field-level differences (added, removed, modified) across modified struct entries.
    pub fn detailed_field_changes(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.modified {
            for change in &entry.nested_changes {
                match change {
                    NestedChange::FieldAdded { field_name, value } => {
                        fields.push(FieldDiff {
                            entry_key: entry.key.clone(),
                            field_name: field_name.clone(),
                            change_type: FieldChangeType::Added {
                                value: value.clone(),
                            },
                        });
                    }
                    NestedChange::FieldRemoved { field_name, value } => {
                        fields.push(FieldDiff {
                            entry_key: entry.key.clone(),
                            field_name: field_name.clone(),
                            change_type: FieldChangeType::Removed {
                                value: value.clone(),
                            },
                        });
                    }
                    NestedChange::FieldModified {
                        field_name,
                        before,
                        after,
                    } => {
                        fields.push(FieldDiff {
                            entry_key: entry.key.clone(),
                            field_name: field_name.clone(),
                            change_type: FieldChangeType::Modified {
                                before: before.clone(),
                                after: after.clone(),
                            },
                        });
                    }
                    _ => {}
                }
            }
        }
        fields
    }

    /// Collect all struct fields present inside newly added entries.
    pub fn added_entry_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.added {
            if let StateValue::Struct(map) = &entry.value {
                for (name, val) in map {
                    fields.push(FieldDiff {
                        entry_key: entry.key.clone(),
                        field_name: name.clone(),
                        change_type: FieldChangeType::Added { value: val.clone() },
                    });
                }
            }
        }
        fields
    }

    /// Collect all struct fields present inside removed entries.
    pub fn removed_entry_fields(&self) -> Vec<FieldDiff> {
        let mut fields = Vec::new();
        for entry in &self.changes.removed {
            if let StateValue::Struct(map) = &entry.value {
                for (name, val) in map {
                    fields.push(FieldDiff {
                        entry_key: entry.key.clone(),
                        field_name: name.clone(),
                        change_type: FieldChangeType::Removed { value: val.clone() },
                    });
                }
            }
        }
        fields
    }

    /// Serialize the migration report to a compact JSON string.
    pub fn to_json(&self) -> Result<String, ReportError> {
        serde_json::to_string(self).map_err(ReportError::Serialization)
    }

    /// Serialize the migration report to a pretty-printed, indented JSON string.
    pub fn to_json_pretty(&self) -> Result<String, ReportError> {
        serde_json::to_string_pretty(self).map_err(ReportError::Serialization)
    }

    /// Deserialize a migration report from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, ReportError> {
        serde_json::from_str(json).map_err(ReportError::Serialization)
    }

    /// Save the report as pretty JSON to a file path.
    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<(), ReportError> {
        let json = self.to_json_pretty()?;
        std::fs::write(path, json).map_err(ReportError::Io)
    }

    /// Load a migration report from a JSON file.
    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<Self, ReportError> {
        let content = std::fs::read_to_string(path).map_err(ReportError::Io)?;
        Self::from_json(&content)
    }

    /// Format this report into a human-readable text string.
    pub fn render_text(&self) -> String {
        crate::render::render_text(self)
    }

    /// Format this report into GitHub-flavored Markdown.
    pub fn render_markdown(&self) -> String {
        crate::render::render_markdown(self)
    }
}

impl std::fmt::Display for MigrationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render_text())
    }
}
