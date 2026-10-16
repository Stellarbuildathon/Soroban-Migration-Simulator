use serde::{Deserialize, Serialize};
use state_engine::StateDiff;

/// High-level numerical summary of state changes resulting from a migration.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DiffSummary {
    /// Number of new storage entries created during migration.
    pub added: usize,
    /// Number of storage entries removed during migration.
    pub removed: usize,
    /// Number of existing storage entries whose values changed.
    pub modified: usize,
    /// Number of storage entries that remained identical before and after migration.
    pub unchanged: usize,
}

impl DiffSummary {
    /// Create a new summary with explicit entry counts.
    pub fn new(added: usize, removed: usize, modified: usize, unchanged: usize) -> Self {
        Self {
            added,
            removed,
            modified,
            unchanged,
        }
    }

    /// Total number of storage entries observed (added + removed + modified + unchanged).
    pub fn total_entries(&self) -> usize {
        self.added + self.removed + self.modified + self.unchanged
    }

    /// Total number of modified, added, or removed storage entries.
    pub fn total_changes(&self) -> usize {
        self.added + self.removed + self.modified
    }

    /// Returns true if any state entries were added, removed, or modified.
    pub fn has_changes(&self) -> bool {
        self.total_changes() > 0
    }

    /// Number of added entries.
    pub fn added_count(&self) -> usize {
        self.added
    }

    /// Number of removed entries.
    pub fn removed_count(&self) -> usize {
        self.removed
    }

    /// Number of modified entries.
    pub fn modified_count(&self) -> usize {
        self.modified
    }

    /// Number of unchanged entries.
    pub fn unchanged_count(&self) -> usize {
        self.unchanged
    }
}

impl From<&StateDiff> for DiffSummary {
    fn from(diff: &StateDiff) -> Self {
        Self {
            added: diff.added_count(),
            removed: diff.removed_count(),
            modified: diff.modified_count(),
            unchanged: diff.unchanged_count(),
        }
    }
}

impl From<StateDiff> for DiffSummary {
    fn from(diff: StateDiff) -> Self {
        Self::from(&diff)
    }
}
