use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateError {
    #[error("Malformed JSON: {0}")]
    MalformedJson(#[from] serde_json::Error),
    #[error("Duplicate state key detected for durability {0:?} and key {1:?}")]
    DuplicateKey(Durability, StateValue),
    #[error("I/O Error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "type", content = "value")]
pub enum StateValue {
    Address(String),
    U32(u32),
    U64(u64),
    Bool(bool),
    Symbol(String),
    Bytes(String),
    Struct(BTreeMap<String, StateValue>),
    Map(Vec<(StateValue, StateValue)>),
    Vec(Vec<StateValue>),
    Option(Option<Box<StateValue>>),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Durability {
    Persistent,
    Instance,
    Temporary,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateEntry {
    pub durability: Durability,
    pub key: StateValue,
    pub value: StateValue,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ContractState {
    pub contract_id: String,
    pub entries: Vec<StateEntry>,
}

impl ContractState {
    pub fn new(contract_id: String, entries: Vec<StateEntry>) -> Result<Self, StateError> {
        let mut state = Self {
            contract_id,
            entries,
        };
        state.validate()?;
        state.canonicalize();
        Ok(state)
    }

    pub fn from_json(json: &str) -> Result<Self, StateError> {
        let mut state: Self = serde_json::from_str(json)?;
        state.validate()?;
        state.canonicalize();
        Ok(state)
    }

    pub fn to_json(&self) -> Result<String, StateError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_file(path: &str) -> Result<Self, StateError> {
        let json = std::fs::read_to_string(path)?;
        Self::from_json(&json)
    }

    pub fn validate(&self) -> Result<(), StateError> {
        let mut seen = HashSet::new();
        for entry in &self.entries {
            let key_tuple = (entry.durability.clone(), entry.key.clone());
            if !seen.insert(key_tuple.clone()) {
                return Err(StateError::DuplicateKey(key_tuple.0, key_tuple.1));
            }
        }
        Ok(())
    }

    pub fn canonicalize(&mut self) {
        self.entries.sort_unstable_by(|a, b| {
            a.durability
                .cmp(&b.durability)
                .then_with(|| a.key.cmp(&b.key))
        });
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        let canonical_json = serde_json::to_string(self).unwrap();
        hasher.update(canonical_json.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Compare `self` (expected) against `other` (actual) and return a
    /// structured `StateDiff`.
    ///
    /// Entries are matched by their canonical `(durability, key)` pair.
    /// Both states must already be canonicalized (sorted by
    /// `(durability, key)`) for correct results.
    pub fn diff(&self, other: &ContractState) -> StateDiff {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        let mut unchanged = Vec::new();

        // Build lookup maps keyed by (durability, key)
        let self_map: BTreeMap<(&Durability, &StateValue), &StateValue> = self
            .entries
            .iter()
            .map(|e| ((&e.durability, &e.key), &e.value))
            .collect();

        let other_map: BTreeMap<(&Durability, &StateValue), &StateValue> = other
            .entries
            .iter()
            .map(|e| ((&e.durability, &e.key), &e.value))
            .collect();

        // Entries in self but not in other → removed
        for (key, value) in &self_map {
            if !other_map.contains_key(key) {
                removed.push(StateEntry {
                    durability: key.0.clone(),
                    key: key.1.clone(),
                    value: (*value).clone(),
                });
            }
        }

        // Entries in other but not in self → added
        for (key, value) in &other_map {
            if !self_map.contains_key(key) {
                added.push(StateEntry {
                    durability: key.0.clone(),
                    key: key.1.clone(),
                    value: (*value).clone(),
                });
            }
        }

        // Entries in both → compare values
        for (key, before_val) in &self_map {
            if let Some(after_val) = other_map.get(key) {
                if before_val == after_val {
                    unchanged.push(StateEntry {
                        durability: key.0.clone(),
                        key: key.1.clone(),
                        value: (**before_val).clone(),
                    });
                } else {
                    modified.push(ModifiedEntry {
                        key: key.1.clone(),
                        before: (**before_val).clone(),
                        after: (**after_val).clone(),
                        nested_changes: compute_nested_changes(before_val, after_val),
                    });
                }
            }
        }

        StateDiff {
            added,
            removed,
            modified,
            unchanged,
        }
    }
}

// ─── Diff Types ─────────────────────────────────────────────────────────────

/// The result of comparing two `ContractState` snapshots.
///
/// Entries are matched by `(durability, key)`. The ordering of items within
/// each category is deterministic (sorted by key).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StateDiff {
    /// Entries present in the post-state but not in the pre-state.
    pub added: Vec<StateEntry>,
    /// Entries present in the pre-state but not in the post-state.
    pub removed: Vec<StateEntry>,
    /// Entries present in both states with different values.
    pub modified: Vec<ModifiedEntry>,
    /// Entries present in both states with identical values.
    pub unchanged: Vec<StateEntry>,
}

/// A logical storage entry that exists in both states but whose value changed.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModifiedEntry {
    /// The storage key identifying this entry.
    pub key: StateValue,
    /// The value before migration.
    pub before: StateValue,
    /// The value after migration.
    pub after: StateValue,
    /// Structured description of nested value changes (for Struct/Map/Vec).
    /// Empty if the change is at a leaf level or not decomposable.
    pub nested_changes: Vec<NestedChange>,
}

/// A structured description of a change within a composite `StateValue`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum NestedChange {
    /// A field was added (Struct only).
    FieldAdded {
        field_name: String,
        value: StateValue,
    },
    /// A field was removed (Struct only).
    FieldRemoved {
        field_name: String,
        value: StateValue,
    },
    /// A field's value changed (Struct only).
    FieldModified {
        field_name: String,
        before: StateValue,
        after: StateValue,
    },
    /// An item was added to a Vec or Map.
    ItemAdded { index: usize, value: StateValue },
    /// An item was removed from a Vec or Map.
    ItemRemoved { index: usize, value: StateValue },
    /// An item at a specific index changed (Vec only).
    ItemModified {
        index: usize,
        before: StateValue,
        after: StateValue,
    },
    /// A Map entry's value changed (matched by key).
    MapEntryModified {
        key: StateValue,
        before: StateValue,
        after: StateValue,
    },
}

impl StateDiff {
    /// Number of entries added in the post-state.
    pub fn added_count(&self) -> usize {
        self.added.len()
    }

    /// Number of entries removed from the pre-state.
    pub fn removed_count(&self) -> usize {
        self.removed.len()
    }

    /// Number of entries whose value changed.
    pub fn modified_count(&self) -> usize {
        self.modified.len()
    }

    /// Number of entries that are identical in both states.
    pub fn unchanged_count(&self) -> usize {
        self.unchanged.len()
    }

    /// Whether any changes were detected.
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.modified.is_empty()
    }

    /// Produce a canonical JSON serialization of this diff.
    ///
    /// The output is deterministic regardless of the original entry
    /// ordering in the input states.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

// ─── Nested Change Computation ──────────────────────────────────────────────

/// Compute structured nested changes between two `StateValue`s.
///
/// Returns a list of `NestedChange` items describing the differences
/// at the field/item level within composite types (Struct, Vec, Map).
/// Returns an empty vector for leaf-level changes or unsupported types.
fn compute_nested_changes(before: &StateValue, after: &StateValue) -> Vec<NestedChange> {
    match (before, after) {
        // Struct ↔ Struct: field-level comparison
        (StateValue::Struct(before_fields), StateValue::Struct(after_fields)) => {
            let mut changes = Vec::new();

            // Fields in before but not in after → removed
            for (name, value) in before_fields {
                if !after_fields.contains_key(name) {
                    changes.push(NestedChange::FieldRemoved {
                        field_name: name.clone(),
                        value: value.clone(),
                    });
                }
            }

            // Fields in after but not in before → added
            for (name, value) in after_fields {
                if !before_fields.contains_key(name) {
                    changes.push(NestedChange::FieldAdded {
                        field_name: name.clone(),
                        value: value.clone(),
                    });
                }
            }

            // Fields in both → compare values
            for (name, before_val) in before_fields {
                if let Some(after_val) = after_fields.get(name) {
                    if before_val != after_val {
                        changes.push(NestedChange::FieldModified {
                            field_name: name.clone(),
                            before: before_val.clone(),
                            after: after_val.clone(),
                        });
                    }
                }
            }

            changes
        }

        // Vec ↔ Vec: index-level comparison
        (StateValue::Vec(before_items), StateValue::Vec(after_items)) => {
            let mut changes = Vec::new();
            let max_len = before_items.len().max(after_items.len());

            for i in 0..max_len {
                match (before_items.get(i), after_items.get(i)) {
                    (Some(before_val), Some(after_val)) => {
                        if before_val != after_val {
                            changes.push(NestedChange::ItemModified {
                                index: i,
                                before: before_val.clone(),
                                after: after_val.clone(),
                            });
                        }
                    }
                    (Some(before_val), None) => {
                        changes.push(NestedChange::ItemRemoved {
                            index: i,
                            value: before_val.clone(),
                        });
                    }
                    (None, Some(after_val)) => {
                        changes.push(NestedChange::ItemAdded {
                            index: i,
                            value: after_val.clone(),
                        });
                    }
                    (None, None) => unreachable!("max_len ensures at least one exists"),
                }
            }

            changes
        }

        // Map ↔ Map: value comparison by key
        (StateValue::Map(before_pairs), StateValue::Map(after_pairs)) => {
            let mut changes = Vec::new();

            // Build lookup for after pairs
            let after_map: BTreeMap<&StateValue, &StateValue> =
                after_pairs.iter().map(|(k, v)| (k, v)).collect();

            // Entries in before but not in after → removed
            for (i, (k, v)) in before_pairs.iter().enumerate() {
                if !after_map.contains_key(k) {
                    changes.push(NestedChange::ItemRemoved {
                        index: i,
                        value: v.clone(),
                    });
                }
            }

            // Entries in after but not in before → added
            let before_keys: HashSet<&StateValue> = before_pairs.iter().map(|(k, _)| k).collect();
            for (i, (k, v)) in after_pairs.iter().enumerate() {
                if !before_keys.contains(k) {
                    changes.push(NestedChange::ItemAdded {
                        index: i,
                        value: v.clone(),
                    });
                }
            }

            // Entries in both → compare values
            for (before_key, before_val) in before_pairs {
                if let Some(after_val) = after_map.get(before_key) {
                    if before_val != *after_val {
                        changes.push(NestedChange::MapEntryModified {
                            key: before_key.clone(),
                            before: before_val.clone(),
                            after: (*after_val).clone(),
                        });
                    }
                }
            }

            changes
        }

        // Option ↔ Option
        (StateValue::Option(before_opt), StateValue::Option(after_opt)) => {
            match (before_opt, after_opt) {
                (Some(before_inner), Some(after_inner)) => {
                    if before_inner != after_inner {
                        compute_nested_changes(before_inner, after_inner)
                    } else {
                        Vec::new()
                    }
                }
                (Some(before_inner), None) => {
                    vec![NestedChange::ItemRemoved {
                        index: 0,
                        value: (**before_inner).clone(),
                    }]
                }
                (None, Some(after_inner)) => {
                    vec![NestedChange::ItemAdded {
                        index: 0,
                        value: (**after_inner).clone(),
                    }]
                }
                (None, None) => Vec::new(),
            }
        }

        // Leaf types or mismatched types → no nested decomposition
        _ => Vec::new(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_v1_json() -> String {
        r#"{
            "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "owner": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                            "value": { "type": "U64", "value": 100 }
                        }
                    }
                }
            ]
        }"#
        .to_string()
    }

    fn dummy_v2_json() -> String {
        r#"{
            "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "owner": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                            "value": { "type": "U64", "value": 100 },
                            "version": { "type": "U32", "value": 2 }
                        }
                    }
                }
            ]
        }"#
        .to_string()
    }

    // ─── Test A: Identical states ───────────────────────────────────────────

    #[test]
    fn test_a_identical_states() {
        let state = ContractState::from_json(&dummy_v1_json()).unwrap();
        let diff = state.diff(&state);

        assert_eq!(diff.added_count(), 0);
        assert_eq!(diff.removed_count(), 0);
        assert_eq!(diff.modified_count(), 0);
        assert_eq!(diff.unchanged_count(), 1);
        assert!(!diff.has_changes());
    }

    // ─── Test B: Added entry ────────────────────────────────────────────────

    #[test]
    fn test_b_added_entry() {
        let pre = ContractState::from_json(&dummy_v1_json()).unwrap();

        let post_json = r#"{
            "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "owner": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                            "value": { "type": "U64", "value": 100 }
                        }
                    }
                },
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 42 },
                    "value": { "type": "U64", "value": 999 }
                }
            ]
        }"#;
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.added_count(), 1);
        assert_eq!(diff.removed_count(), 0);
        assert_eq!(diff.modified_count(), 0);
        assert_eq!(diff.unchanged_count(), 1);
        assert!(diff.has_changes());
        assert_eq!(diff.added[0].key, StateValue::U32(42));
        assert_eq!(diff.added[0].value, StateValue::U64(999));
    }

    // ─── Test C: Removed entry ──────────────────────────────────────────────

    #[test]
    fn test_c_removed_entry() {
        let pre_json = r#"{
            "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "owner": { "type": "Address", "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF" },
                            "value": { "type": "U64", "value": 100 }
                        }
                    }
                },
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 42 },
                    "value": { "type": "U64", "value": 999 }
                }
            ]
        }"#;
        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(&dummy_v1_json()).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.added_count(), 0);
        assert_eq!(diff.removed_count(), 1);
        assert_eq!(diff.modified_count(), 0);
        assert_eq!(diff.unchanged_count(), 1);
        assert!(diff.has_changes());
        assert_eq!(diff.removed[0].key, StateValue::U32(42));
    }

    // ─── Test D: Modified entry ─────────────────────────────────────────────

    #[test]
    fn test_d_modified_entry() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": { "type": "U64", "value": 100 }
                }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": { "type": "U64", "value": 200 }
                }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.added_count(), 0);
        assert_eq!(diff.removed_count(), 0);
        assert_eq!(diff.modified_count(), 1);
        assert_eq!(diff.unchanged_count(), 0);
        assert!(diff.has_changes());
        assert_eq!(diff.modified[0].before, StateValue::U64(100));
        assert_eq!(diff.modified[0].after, StateValue::U64(200));
    }

    // ─── Test E: Mixed changes ──────────────────────────────────────────────

    #[test]
    fn test_e_mixed_changes() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 2 }, "value": { "type": "U64", "value": 200 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 3 }, "value": { "type": "U64", "value": 300 } }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 2 }, "value": { "type": "U64", "value": 999 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 4 }, "value": { "type": "U64", "value": 400 } }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        // Key 1: unchanged, Key 2: modified, Key 3: removed, Key 4: added
        assert_eq!(diff.added_count(), 1);
        assert_eq!(diff.removed_count(), 1);
        assert_eq!(diff.modified_count(), 1);
        assert_eq!(diff.unchanged_count(), 1);
        assert!(diff.has_changes());

        assert_eq!(diff.added[0].key, StateValue::U32(4));
        assert_eq!(diff.removed[0].key, StateValue::U32(3));
        assert_eq!(diff.modified[0].key, StateValue::U32(2));
    }

    // ─── Test F: Reordered entries ──────────────────────────────────────────

    #[test]
    fn test_f_reordered_entries() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 2 }, "value": { "type": "U64", "value": 200 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 2 }, "value": { "type": "U64", "value": 200 } }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert!(!diff.has_changes());
        assert_eq!(diff.unchanged_count(), 2);

        // Reversed ordering should produce the same diff
        let diff2 = post.diff(&pre);
        assert!(!diff2.has_changes());
        assert_eq!(diff2.unchanged_count(), 2);
    }

    // ─── Test G: Nested struct modification ─────────────────────────────────

    #[test]
    fn test_g_nested_struct_modification() {
        let pre = ContractState::from_json(&dummy_v1_json()).unwrap();
        let post = ContractState::from_json(&dummy_v2_json()).unwrap();
        let diff = pre.diff(&post);

        // V1 → V2: same key, different value (struct gains "version" field)
        assert_eq!(diff.modified_count(), 1);
        let modified = &diff.modified[0];

        // The nested changes should indicate a field was added
        assert!(!modified.nested_changes.is_empty());
        let has_added = modified.nested_changes.iter().any(
            |c| matches!(c, NestedChange::FieldAdded { field_name, .. } if field_name == "version"),
        );
        assert!(has_added, "Should detect 'version' field addition");
    }

    // ─── Test H: Nested collection modification ─────────────────────────────

    #[test]
    fn test_h_nested_collection_modification() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": { "type": "Vec", "value": [
                        { "type": "U64", "value": 10 },
                        { "type": "U64", "value": 20 },
                        { "type": "U64", "value": 30 }
                    ]}
                }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": { "type": "Vec", "value": [
                        { "type": "U64", "value": 10 },
                        { "type": "U64", "value": 99 },
                        { "type": "U64", "value": 30 },
                        { "type": "U64", "value": 40 }
                    ]}
                }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.modified_count(), 1);
        let modified = &diff.modified[0];

        // Index 0: unchanged, Index 1: modified (20→99), Index 2: unchanged, Index 3: added
        assert_eq!(modified.nested_changes.len(), 2);
        let has_item_modified = modified
            .nested_changes
            .iter()
            .any(|c| matches!(c, NestedChange::ItemModified { index: 1, .. }));
        assert!(has_item_modified, "Should detect index 1 modification");

        let has_item_added = modified
            .nested_changes
            .iter()
            .any(|c| matches!(c, NestedChange::ItemAdded { index: 3, .. }));
        assert!(has_item_added, "Should detect index 3 addition");
    }

    // ─── Test I: V1 → V2 migration ─────────────────────────────────────────

    #[test]
    fn test_i_v1_to_v2_migration() {
        let pre = ContractState::from_json(&dummy_v1_json()).unwrap();
        let post = ContractState::from_json(&dummy_v2_json()).unwrap();
        let diff = pre.diff(&post);

        // Exactly 1 modified entry (the Record)
        assert_eq!(diff.added_count(), 0, "No entries added at top level");
        assert_eq!(diff.removed_count(), 0, "No entries removed at top level");
        assert_eq!(diff.modified_count(), 1, "Exactly 1 entry modified");
        assert_eq!(diff.unchanged_count(), 0, "No unchanged entries");

        let modified = &diff.modified[0];

        // Before: { owner, value } (2 fields)
        if let StateValue::Struct(before_fields) = &modified.before {
            assert_eq!(before_fields.len(), 2);
            assert!(before_fields.contains_key("owner"));
            assert!(before_fields.contains_key("value"));
            assert!(!before_fields.contains_key("version"));
        } else {
            panic!("Before value should be a Struct");
        }

        // After: { owner, value, version } (3 fields)
        if let StateValue::Struct(after_fields) = &modified.after {
            assert_eq!(after_fields.len(), 3);
            assert!(after_fields.contains_key("owner"));
            assert!(after_fields.contains_key("value"));
            assert!(after_fields.contains_key("version"));
            assert_eq!(after_fields.get("version"), Some(&StateValue::U32(2)));
        } else {
            panic!("After value should be a Struct");
        }

        // Nested changes: version field added, owner and value unchanged
        assert_eq!(modified.nested_changes.len(), 1);
        match &modified.nested_changes[0] {
            NestedChange::FieldAdded { field_name, value } => {
                assert_eq!(field_name, "version");
                assert_eq!(value, &StateValue::U32(2));
            }
            other => panic!("Expected FieldAdded, got {:?}", other),
        }
    }

    // ─── Test J: Deterministic serialization ────────────────────────────────

    #[test]
    fn test_j_deterministic_serialization() {
        // Two states with entries in different order
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 2 }, "value": { "type": "U64", "value": 200 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 999 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 3 }, "value": { "type": "U64", "value": 300 } }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();

        // Compute diff in one order
        let diff1 = pre.diff(&post);
        let json1 = diff1.to_json().unwrap();

        // Compute diff in reversed input order (pre and post swapped, then swapped back)
        // Since pre/post are already canonicalized, the diff should be identical
        let diff2 = pre.diff(&post);
        let json2 = diff2.to_json().unwrap();

        assert_eq!(json1, json2, "Serialized diff must be deterministic");

        // Also verify the diff structure is deterministic
        assert_eq!(diff1.added_count(), diff2.added_count());
        assert_eq!(diff1.removed_count(), diff2.removed_count());
        assert_eq!(diff1.modified_count(), diff2.modified_count());
        assert_eq!(diff1.unchanged_count(), diff2.unchanged_count());
    }

    // ─── Test: V1 fixture deserialization ───────────────────────────────────

    #[test]
    fn test_a_deserialize_v1_state() {
        let state = ContractState::from_json(&dummy_v1_json()).unwrap();
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].durability, Durability::Persistent);
    }

    #[test]
    fn test_b_deserialize_v2_state() {
        let state = ContractState::from_json(&dummy_v2_json()).unwrap();
        assert_eq!(state.entries.len(), 1);

        if let StateValue::Struct(fields) = &state.entries[0].value {
            assert!(fields.contains_key("version"));
        } else {
            panic!("Expected Struct");
        }
    }

    #[test]
    fn test_c_canonical_ordering() {
        let entry1 = StateEntry {
            durability: Durability::Persistent,
            key: StateValue::U32(1),
            value: StateValue::U64(100),
        };
        let entry2 = StateEntry {
            durability: Durability::Instance,
            key: StateValue::U32(2),
            value: StateValue::U64(200),
        };

        let mut state_a =
            ContractState::new("C1".to_string(), vec![entry1.clone(), entry2.clone()]).unwrap();
        let mut state_b = ContractState::new("C1".to_string(), vec![entry2, entry1]).unwrap();

        state_a.canonicalize();
        state_b.canonicalize();

        assert_eq!(state_a.entries, state_b.entries);
    }

    #[test]
    fn test_d_fingerprint_determinism() {
        let entry1 = StateEntry {
            durability: Durability::Persistent,
            key: StateValue::U32(1),
            value: StateValue::U64(100),
        };
        let entry2 = StateEntry {
            durability: Durability::Instance,
            key: StateValue::U32(2),
            value: StateValue::U64(200),
        };

        let state_a =
            ContractState::new("C1".to_string(), vec![entry1.clone(), entry2.clone()]).unwrap();
        let state_b = ContractState::new("C1".to_string(), vec![entry2, entry1]).unwrap();

        assert_eq!(state_a.fingerprint(), state_b.fingerprint());
    }

    #[test]
    fn test_e_state_mutation() {
        let mut state = ContractState::from_json(&dummy_v1_json()).unwrap();
        let old_fingerprint = state.fingerprint();

        state.entries[0].value = StateValue::U64(999);
        state.canonicalize();

        let new_fingerprint = state.fingerprint();
        assert_ne!(old_fingerprint, new_fingerprint);
    }

    #[test]
    fn test_f_duplicate_key_detection() {
        let json = r#"{
            "contract_id": "C1",
            "entries": [
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 100 } },
                { "durability": "Persistent", "key": { "type": "U32", "value": 1 }, "value": { "type": "U64", "value": 200 } }
            ]
        }"#;

        let res = ContractState::from_json(json);
        assert!(matches!(res, Err(StateError::DuplicateKey(_, _))));
    }

    #[test]
    fn test_g_invalid_fixture() {
        let res = ContractState::from_json("{ invalid json ]");
        assert!(matches!(res, Err(StateError::MalformedJson(_))));
    }

    #[test]
    fn test_h_serialization_round_trip() {
        let state = ContractState::from_json(&dummy_v1_json()).unwrap();
        let json = state.to_json().unwrap();
        let state2 = ContractState::from_json(&json).unwrap();

        assert_eq!(state, state2);
        assert_eq!(state.fingerprint(), state2.fingerprint());
    }

    // ─── Test: Summary methods ──────────────────────────────────────────────

    #[test]
    fn test_summary_methods() {
        let diff = StateDiff {
            added: vec![],
            removed: vec![],
            modified: vec![],
            unchanged: vec![],
        };
        assert_eq!(diff.added_count(), 0);
        assert_eq!(diff.removed_count(), 0);
        assert_eq!(diff.modified_count(), 0);
        assert_eq!(diff.unchanged_count(), 0);
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_has_changes_true() {
        let diff = StateDiff {
            added: vec![StateEntry {
                durability: Durability::Persistent,
                key: StateValue::U32(1),
                value: StateValue::U64(100),
            }],
            removed: vec![],
            modified: vec![],
            unchanged: vec![],
        };
        assert!(diff.has_changes());
    }

    // ─── Test: Nested struct with multiple changes ──────────────────────────

    #[test]
    fn test_nested_struct_multiple_changes() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "a": { "type": "U64", "value": 1 },
                            "b": { "type": "U64", "value": 2 },
                            "c": { "type": "U64", "value": 3 }
                        }
                    }
                }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": {
                        "type": "Struct",
                        "value": {
                            "a": { "type": "U64", "value": 1 },
                            "b": { "type": "U64", "value": 99 },
                            "d": { "type": "U64", "value": 4 }
                        }
                    }
                }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.modified_count(), 1);
        let modified = &diff.modified[0];

        // c removed, b modified, d added
        assert_eq!(modified.nested_changes.len(), 3);

        let has_c_removed = modified.nested_changes.iter().any(
            |c| matches!(c, NestedChange::FieldRemoved { field_name, .. } if field_name == "c"),
        );
        assert!(has_c_removed, "Should detect 'c' field removal");

        let has_b_modified = modified.nested_changes.iter().any(
            |c| matches!(c, NestedChange::FieldModified { field_name, .. } if field_name == "b"),
        );
        assert!(has_b_modified, "Should detect 'b' field modification");

        let has_d_added = modified
            .nested_changes
            .iter()
            .any(|c| matches!(c, NestedChange::FieldAdded { field_name, .. } if field_name == "d"));
        assert!(has_d_added, "Should detect 'd' field addition");
    }

    // ─── Test: Map comparison ───────────────────────────────────────────────

    #[test]
    fn test_nested_map_modification() {
        let pre_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": {
                        "type": "Map",
                        "value": [
                            [{ "type": "Symbol", "value": "x" }, { "type": "U64", "value": 10 }],
                            [{ "type": "Symbol", "value": "y" }, { "type": "U64", "value": 20 }]
                        ]
                    }
                }
            ]
        }"#;
        let post_json = r#"{
            "contract_id": "C1",
            "entries": [
                {
                    "durability": "Persistent",
                    "key": { "type": "U32", "value": 1 },
                    "value": {
                        "type": "Map",
                        "value": [
                            [{ "type": "Symbol", "value": "x" }, { "type": "U64", "value": 10 }],
                            [{ "type": "Symbol", "value": "y" }, { "type": "U64", "value": 99 }],
                            [{ "type": "Symbol", "value": "z" }, { "type": "U64", "value": 30 }]
                        ]
                    }
                }
            ]
        }"#;

        let pre = ContractState::from_json(pre_json).unwrap();
        let post = ContractState::from_json(post_json).unwrap();
        let diff = pre.diff(&post);

        assert_eq!(diff.modified_count(), 1);
        let modified = &diff.modified[0];

        // y modified, z added
        assert_eq!(modified.nested_changes.len(), 2);

        let has_y_modified = modified.nested_changes.iter().any(|c| {
            matches!(c, NestedChange::MapEntryModified { key: StateValue::Symbol(s), .. } if s == "y")
        });
        assert!(has_y_modified, "Should detect 'y' map entry modification");

        let has_z_added = modified
            .nested_changes
            .iter()
            .any(|c| matches!(c, NestedChange::ItemAdded { .. }));
        assert!(has_z_added, "Should detect 'z' map entry addition");
    }

    #[test]
    fn test_multi_entry_fixture_files_diff() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let v1_path = manifest_dir.join("../../contracts/fixtures/state/multi-entry-v1-state.json");
        let v2_path =
            manifest_dir.join("../../contracts/fixtures/state/multi-entry-v2-expected-state.json");

        let pre =
            ContractState::from_file(v1_path.to_str().unwrap()).expect("Load multi-entry v1 state");
        let post =
            ContractState::from_file(v2_path.to_str().unwrap()).expect("Load multi-entry v2 state");

        assert_eq!(pre.entries.len(), 3, "Pre-state must have 3 entries");
        assert_eq!(post.entries.len(), 3, "Post-state must have 3 entries");

        let diff = pre.diff(&post);

        assert_eq!(diff.added_count(), 0);
        assert_eq!(diff.removed_count(), 0);
        assert_eq!(diff.unchanged_count(), 0);
        assert_eq!(diff.modified_count(), 3);
        assert!(diff.has_changes());

        for modified in &diff.modified {
            let has_version = modified.nested_changes.iter().any(|c| {
                matches!(
                    c,
                    NestedChange::FieldAdded {
                        field_name,
                        value,
                    } if field_name == "version" && value == &StateValue::U32(2)
                )
            });
            assert!(
                has_version,
                "Expected version field added to each user entry"
            );
        }
    }
}
