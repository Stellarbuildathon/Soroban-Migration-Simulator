pub mod error;
pub mod models;
pub mod render;
pub mod summary;

pub use error::ReportError;
pub use models::{FieldChangeType, FieldDiff, MigrationReport};
pub use render::{render_markdown, render_text};
pub use summary::DiffSummary;

#[cfg(test)]
mod tests {
    use super::*;
    use state_engine::{ContractState, Durability, StateEntry, StateValue};
    use std::collections::BTreeMap;

    fn make_test_record(owner: &str, value: u64, version: Option<u32>) -> StateValue {
        let mut map = BTreeMap::new();
        map.insert("owner".to_string(), StateValue::Address(owner.to_string()));
        map.insert("value".to_string(), StateValue::U64(value));
        if let Some(v) = version {
            map.insert("version".to_string(), StateValue::U32(v));
        }
        StateValue::Struct(map)
    }

    fn setup_test_states() -> (ContractState, ContractState) {
        let owner = "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH";

        // Pre-state: V1 record (without version) + Admin config
        let pre_entries = vec![
            StateEntry {
                durability: Durability::Persistent,
                key: StateValue::Address(owner.to_string()),
                value: make_test_record(owner, 100, None),
            },
            StateEntry {
                durability: Durability::Persistent,
                key: StateValue::Symbol("Admin".to_string()),
                value: StateValue::Address(
                    "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7".to_string(),
                ),
            },
        ];
        let pre_state =
            ContractState::new("test-contract".to_string(), pre_entries).expect("Valid pre-state");

        // Post-state: V2 record (with version=2) + Admin config (unchanged) + NewFlag (added)
        let post_entries = vec![
            StateEntry {
                durability: Durability::Persistent,
                key: StateValue::Address(owner.to_string()),
                value: make_test_record(owner, 100, Some(2)),
            },
            StateEntry {
                durability: Durability::Persistent,
                key: StateValue::Symbol("Admin".to_string()),
                value: StateValue::Address(
                    "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7".to_string(),
                ),
            },
            StateEntry {
                durability: Durability::Persistent,
                key: StateValue::Symbol("NewFlag".to_string()),
                value: StateValue::Bool(true),
            },
        ];
        let post_state = ContractState::new("test-contract".to_string(), post_entries)
            .expect("Valid post-state");

        (pre_state, post_state)
    }

    #[test]
    fn test_successful_migration_report() {
        let (pre_state, post_state) = setup_test_states();
        let diff = pre_state.diff(&post_state);

        let report = MigrationReport::new(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            true,
            42,
            diff,
        );

        // Verify metadata
        assert_eq!(
            report.contract_id,
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
        );
        assert_eq!(report.migration_function, "migrate_record");
        assert!(report.success);
        assert_eq!(report.status(), "SUCCESS");
        assert_eq!(report.duration_ms, 42);
        assert!(report.error.is_none());

        // Verify summary
        assert_eq!(report.summary.added, 1);
        assert_eq!(report.summary.removed, 0);
        assert_eq!(report.summary.modified, 1);
        assert_eq!(report.summary.unchanged, 1);
        assert_eq!(report.summary.total_changes(), 2);
        assert_eq!(report.summary.total_entries(), 3);
        assert!(report.has_changes());

        // Verify detailed field differences
        let added_fields = report.added_fields();
        assert_eq!(added_fields.len(), 1);
        assert_eq!(added_fields[0].field_name, "version");
        assert_eq!(
            added_fields[0].change_type,
            FieldChangeType::Added {
                value: StateValue::U32(2)
            }
        );

        let modified_fields = report.modified_fields();
        assert!(modified_fields.is_empty());

        let removed_fields = report.removed_fields();
        assert!(removed_fields.is_empty());

        let all_field_changes = report.detailed_field_changes();
        assert_eq!(all_field_changes.len(), 1);

        // Verify JSON serialization and roundtrip
        let json = report.to_json().expect("Serialization to compact JSON");
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"duration_ms\":42"));
        assert!(json.contains(
            "\"contract_id\":\"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC\""
        ));
        assert!(json.contains("\"migration_function\":\"migrate_record\""));
        assert!(!json.contains("\"error\"")); // error is None and skipped

        let pretty_json = report
            .to_json_pretty()
            .expect("Serialization to pretty JSON");
        assert!(pretty_json.contains("\"success\": true"));

        let deserialized: MigrationReport =
            MigrationReport::from_json(&json).expect("Deserialization from JSON");
        assert_eq!(deserialized, report);

        let deserialized_pretty: MigrationReport =
            MigrationReport::from_json(&pretty_json).expect("Deserialization from pretty JSON");
        assert_eq!(deserialized_pretty, report);
    }

    #[test]
    fn test_failed_migration_report() {
        let (pre_state, _) = setup_test_states();
        // Failed migration leaves state as pre_state
        let diff = pre_state.diff(&pre_state);

        let error_msg = "Contract invocation failed: host panic: function 'migrate_record' does not exist on contract";
        let report = MigrationReport::new(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            false,
            12,
            diff,
        )
        .with_error(error_msg);

        // Verify metadata
        assert!(!report.success);
        assert_eq!(report.status(), "FAILED");
        assert_eq!(report.duration_ms, 12);
        assert_eq!(report.error.as_deref(), Some(error_msg));

        // Verify summary indicates no changes
        assert_eq!(report.summary.added, 0);
        assert_eq!(report.summary.removed, 0);
        assert_eq!(report.summary.modified, 0);
        assert_eq!(report.summary.unchanged, 2);
        assert_eq!(report.summary.total_changes(), 0);
        assert!(!report.has_changes());

        // Verify JSON contains failure information
        let json = report.to_json().expect("Serialization to JSON");
        assert!(json.contains("\"success\":false"));
        assert!(json.contains(error_msg));

        let deserialized = MigrationReport::from_json(&json).expect("Deserialization");
        assert_eq!(deserialized, report);
        assert_eq!(deserialized.error.as_deref(), Some(error_msg));

        // Verify rendered outputs mention failure and error
        let text = report.render_text();
        assert!(text.contains("FAILED"));
        assert!(text.contains(error_msg));

        let md = report.render_markdown();
        assert!(md.contains("FAILED"));
        assert!(md.contains(error_msg));
    }

    #[test]
    fn test_deterministic_json_output() {
        let (pre_state, post_state) = setup_test_states();
        let diff1 = pre_state.diff(&post_state);
        let diff2 = pre_state.diff(&post_state);

        let report1 = MigrationReport::new(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            true,
            42,
            diff1,
        );

        let report2 = MigrationReport::new(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            true,
            42,
            diff2,
        );

        let json1 = report1.to_json().expect("JSON 1");
        let json2 = report2.to_json().expect("JSON 2");
        assert_eq!(
            json1, json2,
            "Compact JSON must be deterministic across instances"
        );

        let pretty1 = report1.to_json_pretty().expect("Pretty 1");
        let pretty2 = report2.to_json_pretty().expect("Pretty 2");
        assert_eq!(
            pretty1, pretty2,
            "Pretty JSON must be deterministic across instances"
        );

        // Verify multiple consecutive serializations remain byte-for-byte identical
        for _ in 0..10 {
            assert_eq!(report1.to_json().unwrap(), json1);
            assert_eq!(report1.to_json_pretty().unwrap(), pretty1);
        }
    }

    #[test]
    fn test_diff_summary_from_state_diff() {
        let (pre_state, post_state) = setup_test_states();
        let diff = pre_state.diff(&post_state);

        let summary = DiffSummary::from(&diff);
        assert_eq!(summary.added, diff.added_count());
        assert_eq!(summary.removed, diff.removed_count());
        assert_eq!(summary.modified, diff.modified_count());
        assert_eq!(summary.unchanged, diff.unchanged_count());
        assert_eq!(summary.added_count(), 1);
        assert_eq!(summary.removed_count(), 0);
        assert_eq!(summary.modified_count(), 1);
        assert_eq!(summary.unchanged_count(), 1);
        assert_eq!(summary.total_entries(), 3);
        assert_eq!(summary.total_changes(), 2);
        assert!(summary.has_changes());

        let summary_from_owned = DiffSummary::from(diff);
        assert_eq!(summary, summary_from_owned);
    }

    #[test]
    fn test_from_migration_result() {
        let (pre_state, post_state) = setup_test_states();
        let exec_info = migration_engine::ExecutionInfo {
            pre_capture_us: 10_000,
            migration_us: 25_000,
            post_capture_us: 7_000,
        };

        let result = migration_engine::MigrationResult {
            success: true,
            pre_state,
            post_state,
            error: None,
            execution_info: exec_info,
        };

        let report = MigrationReport::from_result(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            &result,
        );

        assert_eq!(
            report.contract_id,
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
        );
        assert_eq!(report.migration_function, "migrate_record");
        assert!(report.success);
        assert_eq!(report.duration_ms, 42); // 42_000 µs = 42 ms
        assert_eq!(report.summary.added, 1);
        assert_eq!(report.summary.modified, 1);
        assert_eq!(report.summary.unchanged, 1);
        assert_eq!(report.summary.removed, 0);
    }

    #[test]
    fn test_file_save_and_load() {
        let (pre_state, post_state) = setup_test_states();
        let diff = pre_state.diff(&post_state);

        let report = MigrationReport::new("test-contract", "migrate", true, 10, diff);

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_migration_report.json");

        report
            .save_to_file(&file_path)
            .expect("Failed to save report to file");
        let loaded =
            MigrationReport::load_from_file(&file_path).expect("Failed to load report from file");

        assert_eq!(report, loaded);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_display_trait_and_render() {
        let (pre_state, post_state) = setup_test_states();
        let diff = pre_state.diff(&post_state);

        let report = MigrationReport::new(
            "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
            "migrate_record",
            true,
            42,
            diff,
        );

        let display_str = format!("{}", report);
        assert!(display_str.contains("SOROBAN MIGRATION SIMULATION REPORT"));
        assert!(display_str.contains("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"));
        assert!(display_str.contains("migrate_record"));
        assert!(display_str.contains("SUCCESS"));
        assert!(display_str.contains("42 ms"));

        let md_str = report.render_markdown();
        assert!(md_str.contains("# Soroban Migration Simulation Report"));
        assert!(md_str.contains("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"));
        assert!(md_str.contains("✅ **SUCCESS**"));
    }
}
