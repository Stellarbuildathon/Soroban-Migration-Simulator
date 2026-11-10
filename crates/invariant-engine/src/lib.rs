//! `invariant-engine` crate: Safety verification layer for the Soroban
//! Migration Simulator.
//!
//! Provides a declarative framework to define and verify migration invariants
//! across pre-migration and post-migration contract states.

use serde::{Deserialize, Serialize};
use state_engine::{ContractState, StateValue};

// ─── Rule Definitions ────────────────────────────────────────────────────────

/// A rule that must remain valid across contract state migration.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", content = "config")]
pub enum InvariantRule {
    /// A specific field within struct values must be preserved across migration.
    FieldPreserved { field: String },

    /// The value for a specific key must remain unchanged.
    ValueUnchanged { key: StateValue },

    /// A required key must exist in the post-migration state.
    EntryMustExist { key: StateValue },

    /// The durability of a specific key must not change.
    DurabilityPreserved { key: StateValue },

    /// No pre-migration entries may be deleted during migration.
    NoEntryRemoved,
}

// ─── Result Models ───────────────────────────────────────────────────────────

/// Outcome of evaluating an individual invariant rule.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InvariantResult {
    /// Whether the invariant rule passed.
    pub passed: bool,
    /// Identifier or label of the rule evaluated.
    pub rule: String,
    /// Detailed diagnostic message describing why the rule passed or failed.
    pub message: String,
}

/// Comprehensive report containing outcomes for all evaluated invariants.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InvariantReport {
    /// Overall status (true if and only if all evaluated rules passed).
    pub passed: bool,
    /// Total number of rules evaluated.
    pub total_rules: usize,
    /// Count of passing rules.
    pub passed_count: usize,
    /// Count of failing rules.
    pub failed_count: usize,
    /// Individual evaluation results.
    pub results: Vec<InvariantResult>,
}

impl InvariantReport {
    /// Create a report from an array of individual invariant results.
    pub fn new(results: Vec<InvariantResult>) -> Self {
        let total_rules = results.len();
        let passed_count = results.iter().filter(|r| r.passed).count();
        let failed_count = total_rules.saturating_sub(passed_count);
        let passed = failed_count == 0;

        Self {
            passed,
            total_rules,
            passed_count,
            failed_count,
            results,
        }
    }
}

// ─── Invariant Engine ───────────────────────────────────────────────────────

/// Engine responsible for evaluating migration invariant rules.
pub struct InvariantEngine;

impl InvariantEngine {
    /// Check a collection of invariant rules against pre and post migration states.
    pub fn check(
        rules: &[InvariantRule],
        pre_state: &ContractState,
        post_state: &ContractState,
    ) -> InvariantReport {
        check_invariants(rules, pre_state, post_state)
    }
}

/// Standalone function to evaluate migration rules against contract states.
pub fn check_invariants(
    rules: &[InvariantRule],
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantReport {
    let mut results = Vec::with_capacity(rules.len());

    for rule in rules {
        let res = evaluate_rule(rule, pre_state, post_state);
        results.push(res);
    }

    InvariantReport::new(results)
}

fn evaluate_rule(
    rule: &InvariantRule,
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantResult {
    match rule {
        InvariantRule::FieldPreserved { field } => {
            evaluate_field_preserved(field, pre_state, post_state)
        }
        InvariantRule::ValueUnchanged { key } => {
            evaluate_value_unchanged(key, pre_state, post_state)
        }
        InvariantRule::EntryMustExist { key } => evaluate_entry_must_exist(key, post_state),
        InvariantRule::DurabilityPreserved { key } => {
            evaluate_durability_preserved(key, pre_state, post_state)
        }
        InvariantRule::NoEntryRemoved => evaluate_no_entry_removed(pre_state, post_state),
    }
}

fn evaluate_field_preserved(
    field: &str,
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantResult {
    let rule_name = format!("FieldPreserved({})", field);
    let mut matching_count = 0;

    for pre_entry in &pre_state.entries {
        if let StateValue::Struct(ref pre_map) = pre_entry.value {
            if let Some(pre_field_val) = pre_map.get(field) {
                matching_count += 1;
                // Find matching post-entry with identical (durability, key)
                let post_entry = post_state
                    .entries
                    .iter()
                    .find(|e| e.durability == pre_entry.durability && e.key == pre_entry.key);

                match post_entry {
                    None => {
                        return InvariantResult {
                            passed: false,
                            rule: rule_name,
                            message: format!(
                                "Entry with key {:?} containing field '{}' was removed in post-migration state",
                                pre_entry.key, field
                            ),
                        };
                    }
                    Some(post_entry) => match post_entry.value {
                        StateValue::Struct(ref post_map) => match post_map.get(field) {
                            None => {
                                return InvariantResult {
                                    passed: false,
                                    rule: rule_name,
                                    message: format!(
                                        "Field '{}' was dropped from entry with key {:?} in post-migration state",
                                        field, pre_entry.key
                                    ),
                                };
                            }
                            Some(post_field_val) if post_field_val != pre_field_val => {
                                return InvariantResult {
                                    passed: false,
                                    rule: rule_name,
                                    message: format!(
                                        "Field '{}' changed in entry with key {:?}: before={:?}, after={:?}",
                                        field, pre_entry.key, pre_field_val, post_field_val
                                    ),
                                };
                            }
                            _ => {}
                        },
                        _ => {
                            return InvariantResult {
                                passed: false,
                                rule: rule_name,
                                message: format!(
                                    "Entry with key {:?} is no longer a struct in post-migration state, losing field '{}'",
                                    pre_entry.key, field
                                ),
                            };
                        }
                    },
                }
            }
        }
    }

    if matching_count == 0 {
        InvariantResult {
            passed: true,
            rule: rule_name,
            message: format!(
                "Field '{}' was not present in pre-migration structs (trivially satisfied)",
                field
            ),
        }
    } else {
        InvariantResult {
            passed: true,
            rule: rule_name,
            message: format!(
                "Field '{}' preserved across all {} matching entries",
                field, matching_count
            ),
        }
    }
}

fn evaluate_value_unchanged(
    key: &StateValue,
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantResult {
    let rule_name = format!("ValueUnchanged({:?})", key);

    let pre_entry = pre_state.entries.iter().find(|e| &e.key == key);
    let post_entry = post_state.entries.iter().find(|e| &e.key == key);

    match (pre_entry, post_entry) {
        (None, _) => InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!("Key {:?} was not present in pre-migration state", key),
        },
        (Some(_), None) => InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!(
                "Entry with key {:?} was removed in post-migration state",
                key
            ),
        },
        (Some(pre), Some(post)) => {
            if pre.value == post.value {
                InvariantResult {
                    passed: true,
                    rule: rule_name,
                    message: format!("Value for key {:?} remained unchanged", key),
                }
            } else {
                InvariantResult {
                    passed: false,
                    rule: rule_name,
                    message: format!(
                        "Value for key {:?} changed from {:?} to {:?}",
                        key, pre.value, post.value
                    ),
                }
            }
        }
    }
}

fn evaluate_entry_must_exist(key: &StateValue, post_state: &ContractState) -> InvariantResult {
    let rule_name = format!("EntryMustExist({:?})", key);
    let exists = post_state.entries.iter().any(|e| &e.key == key);

    if exists {
        InvariantResult {
            passed: true,
            rule: rule_name,
            message: format!(
                "Required entry with key {:?} exists in post-migration state",
                key
            ),
        }
    } else {
        InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!(
                "Required entry with key {:?} does not exist in post-migration state",
                key
            ),
        }
    }
}

fn evaluate_durability_preserved(
    key: &StateValue,
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantResult {
    let rule_name = format!("DurabilityPreserved({:?})", key);

    let pre_entry = pre_state.entries.iter().find(|e| &e.key == key);
    let post_entry = post_state.entries.iter().find(|e| &e.key == key);

    match (pre_entry, post_entry) {
        (None, _) => InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!("Key {:?} was not present in pre-migration state", key),
        },
        (Some(_), None) => InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!(
                "Entry with key {:?} was removed in post-migration state",
                key
            ),
        },
        (Some(pre), Some(post)) => {
            if pre.durability == post.durability {
                InvariantResult {
                    passed: true,
                    rule: rule_name,
                    message: format!(
                        "Durability {:?} preserved for key {:?}",
                        pre.durability, key
                    ),
                }
            } else {
                InvariantResult {
                    passed: false,
                    rule: rule_name,
                    message: format!(
                        "Durability for key {:?} changed from {:?} to {:?}",
                        key, pre.durability, post.durability
                    ),
                }
            }
        }
    }
}

fn evaluate_no_entry_removed(
    pre_state: &ContractState,
    post_state: &ContractState,
) -> InvariantResult {
    let rule_name = "NoEntryRemoved".to_string();
    let mut missing_keys = Vec::new();

    for pre in &pre_state.entries {
        let exists = post_state
            .entries
            .iter()
            .any(|post| post.durability == pre.durability && post.key == pre.key);
        if !exists {
            missing_keys.push(format!("{:?}", pre.key));
        }
    }

    if missing_keys.is_empty() {
        InvariantResult {
            passed: true,
            rule: rule_name,
            message: format!(
                "All {} pre-migration entries are preserved in post-migration state",
                pre_state.entries.len()
            ),
        }
    } else {
        InvariantResult {
            passed: false,
            rule: rule_name,
            message: format!(
                "{} pre-migration entries were removed: [{}]",
                missing_keys.len(),
                missing_keys.join(", ")
            ),
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use state_engine::{Durability, StateEntry};
    use std::collections::BTreeMap;

    fn make_record(owner: &str, balance: u64, role: &str) -> StateValue {
        let mut map = BTreeMap::new();
        map.insert("owner".to_string(), StateValue::Address(owner.to_string()));
        map.insert("balance".to_string(), StateValue::U64(balance));
        map.insert("role".to_string(), StateValue::Symbol(role.to_string()));
        StateValue::Struct(map)
    }

    fn sample_states() -> (ContractState, ContractState) {
        let alice = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7";
        let bob = "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH";

        let pre = ContractState::new(
            "test-contract".to_string(),
            vec![
                StateEntry {
                    durability: Durability::Persistent,
                    key: StateValue::Address(alice.to_string()),
                    value: make_record(alice, 1000, "Admin"),
                },
                StateEntry {
                    durability: Durability::Persistent,
                    key: StateValue::Address(bob.to_string()),
                    value: make_record(bob, 500, "User"),
                },
                StateEntry {
                    durability: Durability::Instance,
                    key: StateValue::Symbol("Config".to_string()),
                    value: StateValue::U32(1),
                },
            ],
        )
        .unwrap();

        // Valid post-state: owner preserved, new field added in upgraded struct, config preserved
        let mut alice_v2_map = BTreeMap::new();
        alice_v2_map.insert("owner".to_string(), StateValue::Address(alice.to_string()));
        alice_v2_map.insert("balance".to_string(), StateValue::U64(1000));
        alice_v2_map.insert("role".to_string(), StateValue::Symbol("Admin".to_string()));
        alice_v2_map.insert("version".to_string(), StateValue::U32(2));

        let mut bob_v2_map = BTreeMap::new();
        bob_v2_map.insert("owner".to_string(), StateValue::Address(bob.to_string()));
        bob_v2_map.insert("balance".to_string(), StateValue::U64(500));
        bob_v2_map.insert("role".to_string(), StateValue::Symbol("User".to_string()));
        bob_v2_map.insert("version".to_string(), StateValue::U32(2));

        let post = ContractState::new(
            "test-contract".to_string(),
            vec![
                StateEntry {
                    durability: Durability::Persistent,
                    key: StateValue::Address(alice.to_string()),
                    value: StateValue::Struct(alice_v2_map),
                },
                StateEntry {
                    durability: Durability::Persistent,
                    key: StateValue::Address(bob.to_string()),
                    value: StateValue::Struct(bob_v2_map),
                },
                StateEntry {
                    durability: Durability::Instance,
                    key: StateValue::Symbol("Config".to_string()),
                    value: StateValue::U32(1),
                },
            ],
        )
        .unwrap();

        (pre, post)
    }

    #[test]
    fn test_passing_invariant_checks() {
        let (pre, post) = sample_states();
        let rules = vec![
            InvariantRule::FieldPreserved {
                field: "owner".to_string(),
            },
            InvariantRule::FieldPreserved {
                field: "balance".to_string(),
            },
            InvariantRule::ValueUnchanged {
                key: StateValue::Symbol("Config".to_string()),
            },
            InvariantRule::EntryMustExist {
                key: StateValue::Symbol("Config".to_string()),
            },
            InvariantRule::NoEntryRemoved,
        ];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(report.passed);
        assert_eq!(report.total_rules, 5);
        assert_eq!(report.passed_count, 5);
        assert_eq!(report.failed_count, 0);
    }

    #[test]
    fn test_failed_ownership_preservation() {
        let (pre, mut post) = sample_states();
        let alice = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7";

        // Tamper with Alice's owner field in post-state
        for entry in &mut post.entries {
            if entry.key == StateValue::Address(alice.to_string()) {
                if let StateValue::Struct(ref mut map) = entry.value {
                    map.insert(
                        "owner".to_string(),
                        StateValue::Address("G_MALICIOUS_ADDRESS".to_string()),
                    );
                }
            }
        }

        let rules = vec![InvariantRule::FieldPreserved {
            field: "owner".to_string(),
        }];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(!report.passed);
        assert_eq!(report.failed_count, 1);
        assert!(report.results[0].message.contains("Field 'owner' changed"));
    }

    #[test]
    fn test_removed_required_entries() {
        let (pre, mut post) = sample_states();
        // Remove the Config entry
        post.entries
            .retain(|e| e.key != StateValue::Symbol("Config".to_string()));

        let rules = vec![
            InvariantRule::EntryMustExist {
                key: StateValue::Symbol("Config".to_string()),
            },
            InvariantRule::NoEntryRemoved,
        ];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(!report.passed);
        assert_eq!(report.failed_count, 2);
    }

    #[test]
    fn test_changed_protected_values() {
        let (pre, mut post) = sample_states();
        // Mutate Config value from 1 to 999
        for entry in &mut post.entries {
            if entry.key == StateValue::Symbol("Config".to_string()) {
                entry.value = StateValue::U32(999);
            }
        }

        let rules = vec![InvariantRule::ValueUnchanged {
            key: StateValue::Symbol("Config".to_string()),
        }];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(!report.passed);
        assert_eq!(report.failed_count, 1);
        assert!(report.results[0]
            .message
            .contains("changed from U32(1) to U32(999)"));
    }

    #[test]
    fn test_durability_preserved() {
        let (pre, mut post) = sample_states();
        // Change Config durability from Instance to Temporary
        for entry in &mut post.entries {
            if entry.key == StateValue::Symbol("Config".to_string()) {
                entry.durability = Durability::Temporary;
            }
        }

        let rules = vec![InvariantRule::DurabilityPreserved {
            key: StateValue::Symbol("Config".to_string()),
        }];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(!report.passed);
        assert_eq!(report.failed_count, 1);
        assert!(report.results[0].message.contains("Durability for key"));
    }

    #[test]
    fn test_multiple_invariant_evaluation() {
        let (pre, mut post) = sample_states();
        // Tamper with Config
        for entry in &mut post.entries {
            if entry.key == StateValue::Symbol("Config".to_string()) {
                entry.value = StateValue::U32(42);
            }
        }

        let rules = vec![
            InvariantRule::FieldPreserved {
                field: "owner".to_string(),
            },
            InvariantRule::ValueUnchanged {
                key: StateValue::Symbol("Config".to_string()),
            },
            InvariantRule::EntryMustExist {
                key: StateValue::Symbol("Config".to_string()),
            },
        ];

        let report = InvariantEngine::check(&rules, &pre, &post);
        assert!(!report.passed);
        assert_eq!(report.total_rules, 3);
        assert_eq!(report.passed_count, 2);
        assert_eq!(report.failed_count, 1);
    }

    #[test]
    fn test_serialization_round_trip() {
        let rule = InvariantRule::FieldPreserved {
            field: "owner".to_string(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let decoded: InvariantRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, decoded);

        let result = InvariantResult {
            passed: true,
            rule: "FieldPreserved(owner)".to_string(),
            message: "Field preserved".to_string(),
        };
        let report = InvariantReport::new(vec![result]);
        let report_json = serde_json::to_string(&report).unwrap();
        let decoded_report: InvariantReport = serde_json::from_str(&report_json).unwrap();
        assert_eq!(report, decoded_report);
    }
}
