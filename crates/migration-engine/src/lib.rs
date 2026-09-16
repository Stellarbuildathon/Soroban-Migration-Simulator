pub mod capture;
pub mod conversion;

use soroban_sdk::{Env, Symbol, Val, Vec as SorobanVec};

use state_engine::{ContractState, Durability, StateEntry, StateValue};
use thiserror::Error;

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum MigrationError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Invalid WASM: {0}")]
    InvalidWasm(String),

    #[error("Contract registration failed: {0}")]
    ContractRegistrationFailure(String),

    #[error("Contract invocation failed: {0}")]
    ContractInvocationFailure(String),

    #[error("State capture failed: {0}")]
    StateCaptureFailure(String),

    #[error("State conversion failed: {0}")]
    StateConversionFailure(String),

    #[error("Migration execution failed: {0}")]
    MigrationFailure(String),

    #[error("State error: {0}")]
    StateError(#[from] state_engine::StateError),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

// ─── Input ──────────────────────────────────────────────────────────────────

/// Input for a migration execution.
#[derive(Debug, Clone)]
pub struct MigrationInput {
    /// Unique contract identifier (used to filter ledger entries).
    pub contract_id: String,

    /// Compiled WASM bytes of the contract that contains the migration logic.
    pub wasm: Vec<u8>,

    /// Pre-migration state entries to seed into the environment.
    pub initial_state: Vec<StateEntry>,

    /// Enum variant name for the storage key prefix.
    /// E.g. "Record" for `DataKey::Record(Address)` produces key = `Vec[Symbol("Record"), Address]`.
    pub key_prefix: String,

    /// Name of the migration function to invoke.
    pub migration_fn: String,

    /// Explicit arguments for the migration function invocation.
    ///
    /// When `Some(args)`, these arguments are passed directly and in order to the
    /// migration function, without prepending the first state entry's key.
    ///
    /// When `None`, falls back to passing the first entry's key from `initial_state`
    /// as the sole argument, maintaining backward compatibility with single-entry migrations.
    pub migration_args: Option<Vec<StateValue>>,
}

// ─── Result ─────────────────────────────────────────────────────────────────

/// Result of a migration execution.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Whether the migration completed successfully.
    pub success: bool,

    /// Contract state captured before migration execution.
    pub pre_state: ContractState,

    /// Contract state captured after migration execution.
    pub post_state: ContractState,

    /// Error message if the migration failed.
    pub error: Option<String>,

    /// Execution timing information.
    pub execution_info: ExecutionInfo,
}

/// Timing and metadata about execution.
#[derive(Debug, Clone)]
pub struct ExecutionInfo {
    /// Duration of pre-state capture (microseconds).
    pub pre_capture_us: u64,
    /// Duration of migration invocation (microseconds).
    pub migration_us: u64,
    /// Duration of post-state capture (microseconds).
    pub post_capture_us: u64,
}

// ─── Engine ─────────────────────────────────────────────────────────────────

pub struct MigrationEngine;

impl MigrationEngine {
    /// Execute a migration simulation.
    ///
    /// Pipeline:
    /// 1. Create a fresh Soroban `Env`
    /// 2. Register the WASM contract
    /// 3. Seed initial state into contract storage
    /// 4. Capture PRE-STATE from ledger snapshot
    /// 5. Invoke the migration function
    /// 6. Capture POST-STATE from ledger snapshot
    /// 7. Return `MigrationResult`
    pub fn execute(input: &MigrationInput) -> Result<MigrationResult, MigrationError> {
        // ── Validate input ──
        if input.wasm.is_empty() {
            return Err(MigrationError::InvalidInput(
                "WASM bytes are empty".to_string(),
            ));
        }
        if input.contract_id.is_empty() {
            return Err(MigrationError::InvalidInput(
                "Contract ID is empty".to_string(),
            ));
        }
        if input.migration_fn.is_empty() {
            return Err(MigrationError::InvalidInput(
                "Migration function name is empty".to_string(),
            ));
        }

        // ── Step 1: Create fresh Env (simulation-only) ──
        // `mock_all_auths()` is simulation-only — in production, authorization
        // would require real signatures or CAP-54 auth trees.
        let env = Env::default();
        env.mock_all_auths();

        // ── Step 2: Register WASM contract ──
        // Uses `env.register()` (non-deprecated) with `&[u8]` WASM bytes.
        let contract_id = env.register(&input.wasm as &[u8], ());

        // ── Step 3: Seed initial state ──
        Self::seed_state(&env, &contract_id, input)?;

        // ── Step 4: Capture PRE-STATE ──
        let t0 = std::time::Instant::now();
        let snapshot = env.to_ledger_snapshot();
        let pre_state = capture::capture_state_from_snapshot(&env, &contract_id, &snapshot)
            .map_err(|e| {
                MigrationError::StateCaptureFailure(format!("Pre-state capture failed: {:?}", e))
            })?;
        let pre_capture_us = t0.elapsed().as_micros() as u64;

        // ── Step 5: Invoke migration function ──
        let t1 = std::time::Instant::now();
        let result = Self::invoke_migration(&env, &contract_id, input);
        let migration_us = t1.elapsed().as_micros() as u64;

        match result {
            Ok(()) => {
                // ── Step 6: Capture POST-STATE ──
                let t2 = std::time::Instant::now();
                let snapshot = env.to_ledger_snapshot();
                let post_state =
                    capture::capture_state_from_snapshot(&env, &contract_id, &snapshot).map_err(
                        |e| {
                            MigrationError::StateCaptureFailure(format!(
                                "Post-state capture failed: {:?}",
                                e
                            ))
                        },
                    )?;
                let post_capture_us = t2.elapsed().as_micros() as u64;

                Ok(MigrationResult {
                    success: true,
                    pre_state,
                    post_state,
                    error: None,
                    execution_info: ExecutionInfo {
                        pre_capture_us,
                        migration_us,
                        post_capture_us,
                    },
                })
            }
            Err(e) => {
                // ── Capture post-state even on failure ──
                // (Soroban may or may not rollback; capture what remains.)
                let t2 = std::time::Instant::now();
                let snapshot = env.to_ledger_snapshot();
                let post_state =
                    capture::capture_state_from_snapshot(&env, &contract_id, &snapshot)
                        .unwrap_or_else(|_| pre_state.clone());
                let post_capture_us = t2.elapsed().as_micros() as u64;

                Ok(MigrationResult {
                    success: false,
                    pre_state,
                    post_state,
                    error: Some(format!("{}", e)),
                    execution_info: ExecutionInfo {
                        pre_capture_us,
                        migration_us,
                        post_capture_us,
                    },
                })
            }
        }
    }

    /// Seed initial contract state into the Soroban environment.
    fn seed_state(
        env: &Env,
        contract_id: &soroban_sdk::Address,
        input: &MigrationInput,
    ) -> Result<(), MigrationError> {
        for entry in &input.initial_state {
            let storage_key =
                conversion::state_value_to_storage_key(env, &entry.key, &input.key_prefix)?;
            let storage_val = conversion::state_value_to_storage_value(env, &entry.value)?;

            env.as_contract(contract_id, || match entry.durability {
                Durability::Persistent => {
                    env.storage().persistent().set(&storage_key, &storage_val);
                }
                Durability::Temporary => {
                    env.storage().temporary().set(&storage_key, &storage_val);
                }
                Durability::Instance => {
                    env.storage().instance().set(&storage_key, &storage_val);
                }
            });
        }
        Ok(())
    }

    /// Invoke the migration function on the contract.
    ///
    /// If `input.migration_args` is `Some(args)`, those arguments are passed directly
    /// to the contract invocation in the provided order.
    ///
    /// If `input.migration_args` is `None`, falls back to passing the first state
    /// entry's key as the primary argument (for backward compatibility).
    ///
    /// Uses `catch_unwind` because the Soroban host panics on invocation failures
    /// (non-existent functions, mismatched argument counts/types, contract panics).
    fn invoke_migration(
        env: &Env,
        contract_id: &soroban_sdk::Address,
        input: &MigrationInput,
    ) -> Result<(), MigrationError> {
        let fn_name = Symbol::new(env, &input.migration_fn);

        // Build arguments: pass explicit migration_args if provided,
        // otherwise fall back to the first state entry's key.
        let mut args: SorobanVec<Val> = soroban_sdk::vec![env];
        if let Some(explicit_args) = &input.migration_args {
            for sv in explicit_args {
                let arg_val = conversion::state_value_to_storage_value(env, sv)?;
                args.push_back(arg_val);
            }
        } else if let Some(first) = input.initial_state.first() {
            let arg_val = conversion::state_value_to_storage_value(env, &first.key)?;
            args.push_back(arg_val);
        }

        // The Soroban host panics on invocation errors (non-existent functions,
        // wrong argument counts, contract panics). We catch the panic and
        // convert it to a typed error.
        let env_clone = env.clone();
        let contract_clone = contract_id.clone();
        let fn_name_clone = fn_name.clone();
        let args_clone = args.clone();

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env_clone.invoke_contract::<Val>(&contract_clone, &fn_name_clone, args_clone);
        }))
        .map_err(|panic_payload| {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic during migration invocation".to_string()
            };
            MigrationError::ContractInvocationFailure(msg)
        })
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use state_engine::{Durability, StateEntry, StateValue};
    use std::collections::BTreeMap;

    fn load_v1_wasm() -> Vec<u8> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../target/wasm32v1-none/release/migration_v1.wasm");
        std::fs::read(&path).expect("Failed to read V1 WASM. Run `cargo build --target wasm32v1-none -p migration_v1` first.")
    }

    fn load_v2_wasm() -> Vec<u8> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../target/wasm32v1-none/release/migration_v2.wasm");
        std::fs::read(&path).expect("Failed to read V2 WASM. Run `cargo build --target wasm32v1-none -p migration_v2` first.")
    }

    fn load_multi_entry_v1_wasm() -> Vec<u8> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../target/wasm32v1-none/release/multi_entry_v1.wasm");
        std::fs::read(&path).expect("Failed to read multi_entry_v1 WASM. Run `cargo build --target wasm32v1-none -p multi_entry_v1` first.")
    }

    fn load_multi_entry_v2_wasm() -> Vec<u8> {
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../target/wasm32v1-none/release/multi_entry_v2.wasm");
        std::fs::read(&path).expect("Failed to read multi_entry_v2 WASM. Run `cargo build --target wasm32v1-none -p multi_entry_v2` first.")
    }

    /// Build a V1 state entry for `DataKey::Record(owner)` with `Record { owner, value }`.
    fn make_v1_state_entry(owner: &soroban_sdk::Address, value: u64) -> StateEntry {
        let owner_addr = format!("{}", owner.to_string());
        let mut fields = BTreeMap::new();
        fields.insert("owner".to_string(), StateValue::Address(owner_addr.clone()));
        fields.insert("value".to_string(), StateValue::U64(value));

        StateEntry {
            durability: Durability::Persistent,
            key: StateValue::Address(owner_addr),
            value: StateValue::Struct(fields),
        }
    }

    /// Build a V2 state entry for `DataKey::Record(owner)` with `RecordV2 { owner, value, version }`.
    fn make_v2_state_entry(owner: &soroban_sdk::Address, value: u64, version: u32) -> StateEntry {
        let owner_addr = format!("{}", owner.to_string());
        let mut fields = BTreeMap::new();
        fields.insert("owner".to_string(), StateValue::Address(owner_addr.clone()));
        fields.insert("value".to_string(), StateValue::U64(value));
        fields.insert("version".to_string(), StateValue::U32(version));

        StateEntry {
            durability: Durability::Persistent,
            key: StateValue::Address(owner_addr),
            value: StateValue::Struct(fields),
        }
    }

    /// Build a V1 User state entry for `DataKey::User(owner)` with `UserRecord { balance, owner }`.
    fn make_user_v1_entry(owner: &soroban_sdk::Address, balance: u64) -> StateEntry {
        let owner_addr = format!("{}", owner.to_string());
        let mut fields = BTreeMap::new();
        fields.insert("balance".to_string(), StateValue::U64(balance));
        fields.insert("owner".to_string(), StateValue::Address(owner_addr.clone()));

        StateEntry {
            durability: Durability::Persistent,
            key: StateValue::Address(owner_addr),
            value: StateValue::Struct(fields),
        }
    }

    /// Build a V2 User state entry for `DataKey::User(owner)` with `UserRecordV2 { balance, owner, version }`.
    fn make_user_v2_entry(owner: &soroban_sdk::Address, balance: u64, version: u32) -> StateEntry {
        let owner_addr = format!("{}", owner.to_string());
        let mut fields = BTreeMap::new();
        fields.insert("balance".to_string(), StateValue::U64(balance));
        fields.insert("owner".to_string(), StateValue::Address(owner_addr.clone()));
        fields.insert("version".to_string(), StateValue::U32(version));

        StateEntry {
            durability: Durability::Persistent,
            key: StateValue::Address(owner_addr),
            value: StateValue::Struct(fields),
        }
    }

    // ─── TEST 1: V1 → V2 Integration ────────────────────────────────────────

    #[test]
    fn test_v1_to_v2_integration() {
        let env = Env::default();
        env.mock_all_auths();

        // Generate a deterministic owner address for the test
        let owner = soroban_sdk::Address::generate(&env);

        let v1_state = make_v1_state_entry(&owner, 100);

        let input = MigrationInput {
            contract_id: "test-v1-to-v2".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");

        assert!(result.success, "Migration should succeed");

        // Verify PRE-STATE: should contain V1 data (owner + value, no version)
        assert!(
            !result.pre_state.entries.is_empty(),
            "Pre-state should have entries"
        );
        let pre_entry = &result.pre_state.entries[0];
        if let StateValue::Struct(fields) = &pre_entry.value {
            assert!(
                fields.contains_key("owner"),
                "Pre-state should have 'owner'"
            );
            assert!(
                fields.contains_key("value"),
                "Pre-state should have 'value'"
            );
            assert!(
                !fields.contains_key("version"),
                "Pre-state should NOT have 'version'"
            );
            // Verify values
            if let Some(StateValue::U64(v)) = fields.get("value") {
                assert_eq!(*v, 100, "Pre-state value should be 100");
            }
        } else {
            panic!("Pre-state value should be a Struct");
        }

        // Verify POST-STATE: should contain V2 data (owner + value + version=2)
        assert!(
            !result.post_state.entries.is_empty(),
            "Post-state should have entries"
        );
        let post_entry = &result.post_state.entries[0];
        if let StateValue::Struct(fields) = &post_entry.value {
            assert!(
                fields.contains_key("owner"),
                "Post-state should have 'owner'"
            );
            assert!(
                fields.contains_key("value"),
                "Post-state should have 'value'"
            );
            assert!(
                fields.contains_key("version"),
                "Post-state should have 'version'"
            );
            // Verify values preserved
            if let Some(StateValue::U64(v)) = fields.get("value") {
                assert_eq!(*v, 100, "Post-state value should be preserved as 100");
            }
            if let Some(StateValue::U32(v)) = fields.get("version") {
                assert_eq!(*v, 2, "Post-state version should be 2");
            }
            // Verify owner preserved
            if let Some(StateValue::Address(a)) = fields.get("owner") {
                assert_eq!(a, &format!("{}", owner.to_string()));
            }
        } else {
            panic!("Post-state value should be a Struct");
        }

        println!("✓ V1→V2 integration test passed");
        println!("  Pre-state entries: {}", result.pre_state.entries.len());
        println!("  Post-state entries: {}", result.post_state.entries.len());
    }

    // ─── TEST 2: New Entry Discovery ────────────────────────────────────────

    #[test]
    fn test_new_entry_discovery() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);

        let v1_state = make_v1_state_entry(&owner, 100);

        let input = MigrationInput {
            contract_id: "test-new-entry".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");

        // Verify: 'version' field NOT in pre-state but IS in post-state
        let pre_entry = &result.pre_state.entries[0];
        let post_entry = &result.post_state.entries[0];

        let pre_has_version = if let StateValue::Struct(f) = &pre_entry.value {
            f.contains_key("version")
        } else {
            false
        };
        let post_has_version = if let StateValue::Struct(f) = &post_entry.value {
            f.contains_key("version")
        } else {
            false
        };

        assert!(!pre_has_version, "Pre-state should NOT contain 'version'");
        assert!(
            post_has_version,
            "Post-state SHOULD contain 'version' (newly added by migration)"
        );

        // Verify the new entry's value
        if let StateValue::Struct(f) = &post_entry.value {
            assert_eq!(
                f.get("version"),
                Some(&StateValue::U32(2)),
                "New 'version' entry should equal 2"
            );
        }

        println!("✓ New entry discovery test passed");
    }

    // ─── TEST 3: Deleted Entry Discovery ────────────────────────────────────

    #[test]
    fn test_deleted_entry_discovery() {
        // The V1 record has { owner, value } (2 fields).
        // After migration to V2, the record has { owner, value, version } (3 fields).
        // We verify that the OLD value map (without 'version') is not the post-state.
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);

        let v1_state = make_v1_state_entry(&owner, 100);

        let input = MigrationInput {
            contract_id: "test-deleted-entry".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");

        // Verify: The pre-state struct has exactly {owner, value} (no version)
        let pre_entry = &result.pre_state.entries[0];
        if let StateValue::Struct(f) = &pre_entry.value {
            assert_eq!(f.len(), 2, "Pre-state struct should have exactly 2 fields");
            assert!(f.contains_key("owner"));
            assert!(f.contains_key("value"));
            assert!(!f.contains_key("version"));
        }

        // Verify: The post-state struct has {owner, value, version} (3 fields)
        let post_entry = &result.post_state.entries[0];
        if let StateValue::Struct(f) = &post_entry.value {
            assert_eq!(f.len(), 3, "Post-state struct should have exactly 3 fields");
            assert!(f.contains_key("owner"));
            assert!(f.contains_key("value"));
            assert!(f.contains_key("version"));
        }

        // The old structure (without version) is effectively "deleted" —
        // the post-state no longer has the 2-field struct shape.
        println!("✓ Deleted entry discovery test passed");
    }

    // ─── TEST 4: Failed Migration ───────────────────────────────────────────

    #[test]
    fn test_failed_migration() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);

        // Create V1 state
        let v1_state = make_v1_state_entry(&owner, 100);

        // Use V1 WASM with V2's migrate_record function — it won't exist.
        // This simulates a migration function that doesn't exist on the contract.
        let input = MigrationInput {
            contract_id: "test-failed".to_string(),
            wasm: load_v1_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(), // V1 contract has no migrate_record
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input);

        match result {
            Ok(r) => {
                assert!(
                    !r.success,
                    "Result should indicate failure when migration function doesn't exist"
                );
                assert!(
                    r.error.is_some(),
                    "Failed result should contain error message"
                );
                // Pre-state should still be valid
                assert!(
                    !r.pre_state.entries.is_empty(),
                    "Pre-state should be captured even on failure"
                );
                println!("✓ Failed migration test passed (engine-level catch)");
                println!("  Error: {:?}", r.error);
            }
            Err(MigrationError::ContractInvocationFailure(_)) => {
                // Also acceptable: the engine propagates the invocation error
                println!("✓ Failed migration test passed (propagated error)");
            }
            Err(e) => {
                panic!("Unexpected error type: {:?}", e);
            }
        }
    }

    // ─── TEST 5: Determinism ────────────────────────────────────────────────

    #[test]
    fn test_determinism() {
        let run_migration = || -> MigrationResult {
            let env = Env::default();
            env.mock_all_auths();
            let owner = soroban_sdk::Address::generate(&env);

            let v1_state = make_v1_state_entry(&owner, 100);

            let input = MigrationInput {
                contract_id: "test-determinism".to_string(),
                wasm: load_v2_wasm(),
                initial_state: vec![v1_state],
                key_prefix: "Record".to_string(),
                migration_fn: "migrate_record".to_string(),
                migration_args: None,
            };

            MigrationEngine::execute(&input).expect("Migration should not fail")
        };

        let r1 = run_migration();
        let r2 = run_migration();

        assert!(r1.success);
        assert!(r2.success);

        // Pre-states should be identical
        assert_eq!(
            r1.pre_state.fingerprint(),
            r2.pre_state.fingerprint(),
            "Pre-state fingerprints should be deterministic"
        );
        assert_eq!(
            r1.pre_state.entries, r2.pre_state.entries,
            "Pre-state entries should be identical"
        );

        // Post-states should be identical
        assert_eq!(
            r1.post_state.fingerprint(),
            r2.post_state.fingerprint(),
            "Post-state fingerprints should be deterministic"
        );
        assert_eq!(
            r1.post_state.entries, r2.post_state.entries,
            "Post-state entries should be identical"
        );

        println!("✓ Determinism test passed");
        println!("  Pre-state fingerprint: {}", r1.pre_state.fingerprint());
        println!("  Post-state fingerprint: {}", r1.post_state.fingerprint());
    }

    // ─── TEST 6: Input Validation ───────────────────────────────────────────

    #[test]
    fn test_invalid_input_empty_wasm() {
        let input = MigrationInput {
            contract_id: "test".to_string(),
            wasm: vec![],
            initial_state: vec![],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input);
        assert!(matches!(result, Err(MigrationError::InvalidInput(_))));
    }

    #[test]
    fn test_invalid_input_empty_fn() {
        let input = MigrationInput {
            contract_id: "test".to_string(),
            wasm: vec![0, 1, 2],
            initial_state: vec![],
            key_prefix: "Record".to_string(),
            migration_fn: "".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input);
        assert!(matches!(result, Err(MigrationError::InvalidInput(_))));
    }

    // ─── TEST 7: State Durability ───────────────────────────────────────────

    #[test]
    fn test_state_durability_preserved() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);

        let v1_state = make_v1_state_entry(&owner, 100);

        let input = MigrationInput {
            contract_id: "test-durability".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");

        // Both pre and post should have Persistent durability
        for entry in &result.pre_state.entries {
            assert_eq!(
                entry.durability,
                Durability::Persistent,
                "Pre-state entries should be Persistent"
            );
        }
        for entry in &result.post_state.entries {
            assert_eq!(
                entry.durability,
                Durability::Persistent,
                "Post-state entries should be Persistent"
            );
        }

        println!("✓ State durability test passed");
    }

    // ─── TEST 8: Multiple Records ───────────────────────────────────────────

    #[test]
    fn test_multiple_records_migration() {
        // migrate_record(env, owner) only migrates one record at a time.
        // With two seeded records, only the first (the primary key passed to
        // migrate_record) should be upgraded to V2; the second remains V1.
        let env = Env::default();
        env.mock_all_auths();
        let owner1 = soroban_sdk::Address::generate(&env);
        let owner2 = soroban_sdk::Address::generate(&env);

        let entry1 = make_v1_state_entry(&owner1, 100);
        let entry2 = make_v1_state_entry(&owner2, 200);

        let input = MigrationInput {
            contract_id: "test-multi".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![entry1, entry2],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(result.success);

        // Both pre and post should have 2 entries
        assert_eq!(
            result.pre_state.entries.len(),
            2,
            "Pre-state should have 2 entries"
        );
        assert_eq!(
            result.post_state.entries.len(),
            2,
            "Post-state should have 2 entries"
        );

        // The post-state should have at least one entry with version=2
        // (the migrated record) and one without (the unmigrated record).
        let has_versioned = result.post_state.entries.iter().any(|e| {
            if let StateValue::Struct(f) = &e.value {
                f.get("version") == Some(&StateValue::U32(2))
            } else {
                false
            }
        });
        assert!(
            has_versioned,
            "At least one record should be migrated to V2"
        );

        let has_unversioned = result.post_state.entries.iter().any(|e| {
            if let StateValue::Struct(f) = &e.value {
                !f.contains_key("version") && f.len() == 2
            } else {
                false
            }
        });
        assert!(
            has_unversioned,
            "At least one record should remain V1 (unmigrated)"
        );

        println!("✓ Multiple records migration test passed");
    }

    // ─── TEST 9: Execution Info ─────────────────────────────────────────────

    #[test]
    fn test_execution_info_populated() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);

        let v1_state = make_v1_state_entry(&owner, 100);

        let input = MigrationInput {
            contract_id: "test-exec-info".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v1_state],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: None,
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");

        assert!(
            result.execution_info.pre_capture_us > 0 || result.execution_info.post_capture_us > 0,
            "Execution info should have non-zero timing"
        );

        println!("✓ Execution info test passed");
        println!("  Pre-capture: {}µs", result.execution_info.pre_capture_us);
        println!("  Migration: {}µs", result.execution_info.migration_us);
        println!(
            "  Post-capture: {}µs",
            result.execution_info.post_capture_us
        );
    }

    // ─── TEST 10: Explicit Single-Argument Invocation ────────────────────────

    #[test]
    fn test_explicit_single_argument_invocation() {
        let env = Env::default();
        env.mock_all_auths();
        let owner1 = soroban_sdk::Address::generate(&env);
        let owner2 = soroban_sdk::Address::generate(&env);

        let entry1 = make_v1_state_entry(&owner1, 100);
        let entry2 = make_v1_state_entry(&owner2, 200);

        let owner2_str = format!("{}", owner2.to_string());
        let owner1_str = format!("{}", owner1.to_string());

        // When explicit migration_args is supplied with owner2, it must migrate owner2
        // even though owner1 is the first entry in initial_state.
        let input = MigrationInput {
            contract_id: "test-explicit-single".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![entry1, entry2],
            key_prefix: "Record".to_string(),
            migration_fn: "migrate_record".to_string(),
            migration_args: Some(vec![StateValue::Address(owner2_str.clone())]),
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(
            result.success,
            "Migration should succeed with explicit single argument"
        );

        // Locate owner2's post-state entry: should have version 2
        let owner2_post = result
            .post_state
            .entries
            .iter()
            .find(|e| {
                if let StateValue::Struct(f) = &e.value {
                    f.get("owner") == Some(&StateValue::Address(owner2_str.clone()))
                } else {
                    false
                }
            })
            .expect("owner2 entry should exist in post-state");

        if let StateValue::Struct(f) = &owner2_post.value {
            assert_eq!(
                f.get("version"),
                Some(&StateValue::U32(2)),
                "owner2 should be migrated to V2 with version=2"
            );
            assert_eq!(
                f.get("value"),
                Some(&StateValue::U64(200)),
                "owner2 value should be preserved as 200"
            );
        } else {
            panic!("owner2 post-state should be a Struct");
        }

        // Locate owner1's post-state entry: should remain V1 (no version)
        let owner1_post = result
            .post_state
            .entries
            .iter()
            .find(|e| {
                if let StateValue::Struct(f) = &e.value {
                    f.get("owner") == Some(&StateValue::Address(owner1_str.clone()))
                } else {
                    false
                }
            })
            .expect("owner1 entry should exist in post-state");

        if let StateValue::Struct(f) = &owner1_post.value {
            assert!(
                !f.contains_key("version"),
                "owner1 should NOT be migrated because owner2 was explicitly targeted"
            );
            assert_eq!(
                f.get("value"),
                Some(&StateValue::U64(100)),
                "owner1 value should remain 100"
            );
        } else {
            panic!("owner1 post-state should be a Struct");
        }

        println!("✓ Explicit single-argument invocation test passed");
    }

    // ─── TEST 11: Multiple Migration Arguments Invocation ───────────────────

    #[test]
    fn test_multiple_migration_arguments_invocation() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);
        let owner_str = format!("{}", owner.to_string());

        // Seed initial state with a V2 record having value = 100, version = 2
        let v2_state = make_v2_state_entry(&owner, 100, 2);

        // Call update_record(owner, 999) which requires 2 arguments: (Address, u64)
        let input = MigrationInput {
            contract_id: "test-multi-args".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v2_state],
            key_prefix: "Record".to_string(),
            migration_fn: "update_record".to_string(),
            migration_args: Some(vec![
                StateValue::Address(owner_str.clone()),
                StateValue::U64(999),
            ]),
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(
            result.success,
            "Migration with multiple arguments should succeed"
        );

        // Verify PRE-STATE had value = 100
        let pre_entry = &result.pre_state.entries[0];
        if let StateValue::Struct(f) = &pre_entry.value {
            assert_eq!(f.get("value"), Some(&StateValue::U64(100)));
        }

        // Verify POST-STATE has updated value = 999 and version = 2 preserved
        let post_entry = &result.post_state.entries[0];
        if let StateValue::Struct(f) = &post_entry.value {
            assert_eq!(
                f.get("value"),
                Some(&StateValue::U64(999)),
                "Value should be updated to 999 by multi-argument invocation"
            );
            assert_eq!(
                f.get("version"),
                Some(&StateValue::U32(2)),
                "Version should remain 2"
            );
            assert_eq!(
                f.get("owner"),
                Some(&StateValue::Address(owner_str)),
                "Owner address should match"
            );
        } else {
            panic!("Post-state entry should be a Struct");
        }

        println!("✓ Multiple migration arguments invocation test passed");
    }

    // ─── TEST 12: Argument Ordering Preserved ───────────────────────────────

    #[test]
    fn test_argument_ordering_preserved() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);
        let owner_str = format!("{}", owner.to_string());

        let v2_state = make_v2_state_entry(&owner, 100, 2);

        // Supply arguments in reverse order: [U64(999), Address(owner)]
        // Since update_record expects (Address, u64), this will fail contract invocation type check.
        let input_reversed = MigrationInput {
            contract_id: "test-arg-order".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v2_state],
            key_prefix: "Record".to_string(),
            migration_fn: "update_record".to_string(),
            migration_args: Some(vec![StateValue::U64(999), StateValue::Address(owner_str)]),
        };

        let result_reversed = MigrationEngine::execute(&input_reversed);
        match result_reversed {
            Ok(r) => {
                assert!(
                    !r.success,
                    "Reversed arguments should fail invocation due to signature mismatch"
                );
                assert!(
                    r.error.is_some(),
                    "Failed result should have error message explaining invocation failure"
                );
            }
            Err(MigrationError::ContractInvocationFailure(_)) => {
                // Also valid: error returned directly
            }
            Err(e) => panic!("Unexpected error variant: {:?}", e),
        }

        println!("✓ Argument ordering preserved test passed");
    }

    // ─── TEST 13: Multiple Arguments Failure Handling ───────────────────────

    #[test]
    fn test_multiple_arguments_failure_handling() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);
        let owner_str = format!("{}", owner.to_string());

        let v2_state = make_v2_state_entry(&owner, 100, 2);

        // update_record expects 2 arguments: (Address, u64).
        // Pass only 1 argument: [Address(owner)]
        let input = MigrationInput {
            contract_id: "test-multi-arg-fail".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![v2_state],
            key_prefix: "Record".to_string(),
            migration_fn: "update_record".to_string(),
            migration_args: Some(vec![StateValue::Address(owner_str)]),
        };

        let result = MigrationEngine::execute(&input);
        match result {
            Ok(r) => {
                assert!(
                    !r.success,
                    "Calling function with insufficient arguments should indicate failure"
                );
                assert!(r.error.is_some(), "Error message should be present");
                // Pre-state should still be preserved
                assert_eq!(r.pre_state.entries.len(), 1);
            }
            Err(MigrationError::ContractInvocationFailure(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }

        println!("✓ Multiple arguments failure handling test passed");
    }

    // ─── TEST 14: Multiple Arguments Determinism ────────────────────────────

    #[test]
    fn test_multiple_arguments_determinism() {
        let run_multi_arg = || -> MigrationResult {
            let env = Env::default();
            env.mock_all_auths();
            let owner = soroban_sdk::Address::generate(&env);
            let owner_str = format!("{}", owner.to_string());

            let v2_state = make_v2_state_entry(&owner, 50, 2);

            let input = MigrationInput {
                contract_id: "test-multi-determinism".to_string(),
                wasm: load_v2_wasm(),
                initial_state: vec![v2_state],
                key_prefix: "Record".to_string(),
                migration_fn: "update_record".to_string(),
                migration_args: Some(vec![StateValue::Address(owner_str), StateValue::U64(750)]),
            };

            MigrationEngine::execute(&input).expect("Migration should succeed")
        };

        let r1 = run_multi_arg();
        let r2 = run_multi_arg();

        assert!(r1.success);
        assert!(r2.success);

        // Verify post-state values updated consistently
        if let StateValue::Struct(f1) = &r1.post_state.entries[0].value {
            assert_eq!(f1.get("value"), Some(&StateValue::U64(750)));
        }
        if let StateValue::Struct(f2) = &r2.post_state.entries[0].value {
            assert_eq!(f2.get("value"), Some(&StateValue::U64(750)));
        }

        // Test determinism with fixed owner address across two runs
        let fixed_owner = {
            let env = Env::default();
            soroban_sdk::Address::generate(&env)
        };
        let fixed_owner_str = format!("{}", fixed_owner.to_string());

        let run_fixed = || -> MigrationResult {
            let v2_state = make_v2_state_entry(&fixed_owner, 50, 2);

            let input = MigrationInput {
                contract_id: "test-multi-determinism-fixed".to_string(),
                wasm: load_v2_wasm(),
                initial_state: vec![v2_state],
                key_prefix: "Record".to_string(),
                migration_fn: "update_record".to_string(),
                migration_args: Some(vec![
                    StateValue::Address(fixed_owner_str.clone()),
                    StateValue::U64(750),
                ]),
            };

            MigrationEngine::execute(&input).expect("Migration should succeed")
        };

        let fixed_r1 = run_fixed();
        let fixed_r2 = run_fixed();

        assert_eq!(
            fixed_r1.pre_state.fingerprint(),
            fixed_r2.pre_state.fingerprint(),
            "Pre-state fingerprints should be identical"
        );
        assert_eq!(
            fixed_r1.post_state.fingerprint(),
            fixed_r2.post_state.fingerprint(),
            "Post-state fingerprints should be identical"
        );
        assert_eq!(fixed_r1.post_state.entries, fixed_r2.post_state.entries);

        println!("✓ Multiple arguments determinism test passed");
    }

    // ─── TEST 15: Explicit Arguments Empty State Invocation ─────────────────

    #[test]
    fn test_explicit_arguments_without_initial_state() {
        let env = Env::default();
        env.mock_all_auths();
        let owner = soroban_sdk::Address::generate(&env);
        let owner_str = format!("{}", owner.to_string());

        // create_record(owner, 500) creates a new record from scratch with 2 arguments.
        // It does not need any pre-existing initial_state entries.
        let input = MigrationInput {
            contract_id: "test-create-no-initial".to_string(),
            wasm: load_v2_wasm(),
            initial_state: vec![],
            key_prefix: "Record".to_string(),
            migration_fn: "create_record".to_string(),
            migration_args: Some(vec![StateValue::Address(owner_str), StateValue::U64(500)]),
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(
            result.success,
            "create_record with explicit arguments should succeed"
        );

        assert_eq!(
            result.pre_state.entries.len(),
            0,
            "Pre-state should have 0 entries"
        );
        assert_eq!(
            result.post_state.entries.len(),
            1,
            "Post-state should have 1 entry"
        );

        if let StateValue::Struct(f) = &result.post_state.entries[0].value {
            assert_eq!(f.get("value"), Some(&StateValue::U64(500)));
            assert_eq!(f.get("version"), Some(&StateValue::U32(2)));
        } else {
            panic!("Post-state entry should be a Struct");
        }

        println!("✓ Explicit arguments without initial state test passed");
    }

    // ─── TEST 16: Multi-Entry V1 → V2 Migration ────────────────────────────

    #[test]
    fn test_multi_entry_v1_to_v2_migration() {
        let env = Env::default();
        env.mock_all_auths();

        let user_a = soroban_sdk::Address::generate(&env);
        let user_b = soroban_sdk::Address::generate(&env);
        let user_c = soroban_sdk::Address::generate(&env);

        let user_a_str = format!("{}", user_a.to_string());
        let user_b_str = format!("{}", user_b.to_string());
        let user_c_str = format!("{}", user_c.to_string());

        let entry_a = make_user_v1_entry(&user_a, 100);
        let entry_b = make_user_v1_entry(&user_b, 250);
        let entry_c = make_user_v1_entry(&user_c, 500);

        let input = MigrationInput {
            contract_id: "test-multi-entry-v1-v2".to_string(),
            wasm: load_multi_entry_v2_wasm(),
            initial_state: vec![entry_a, entry_b, entry_c],
            key_prefix: "User".to_string(),
            migration_fn: "migrate_users".to_string(),
            migration_args: Some(vec![StateValue::Vec(vec![
                StateValue::Address(user_a_str.clone()),
                StateValue::Address(user_b_str.clone()),
                StateValue::Address(user_c_str.clone()),
            ])]),
        };

        let result =
            MigrationEngine::execute(&input).expect("Multi-entry migration should not fail");
        assert!(result.success, "Multi-entry migration must succeed");

        // Verify PRE-STATE: 3 entries, all without version field
        assert_eq!(
            result.pre_state.entries.len(),
            3,
            "Pre-state should contain 3 user records"
        );
        for entry in &result.pre_state.entries {
            if let StateValue::Struct(f) = &entry.value {
                assert!(
                    !f.contains_key("version"),
                    "Pre-state user record must not have version"
                );
                assert!(
                    f.contains_key("balance"),
                    "Pre-state user record must have balance"
                );
                assert!(
                    f.contains_key("owner"),
                    "Pre-state user record must have owner"
                );
            } else {
                panic!("Pre-state value must be a Struct");
            }
        }

        // Verify POST-STATE: 3 entries, all upgraded with version = 2 and preserved fields
        assert_eq!(
            result.post_state.entries.len(),
            3,
            "Post-state should contain 3 user records"
        );

        let check_user = |owner: &soroban_sdk::Address, expected_balance: u64| {
            let owner_str = format!("{}", owner.to_string());
            let expected_entry = make_user_v2_entry(owner, expected_balance, 2);
            let entry = result
                .post_state
                .entries
                .iter()
                .find(|e| {
                    if let StateValue::Struct(f) = &e.value {
                        f.get("owner") == Some(&StateValue::Address(owner_str.clone()))
                    } else {
                        false
                    }
                })
                .unwrap_or_else(|| panic!("User {} must exist in post-state", owner_str));

            assert_eq!(
                entry.value, expected_entry.value,
                "Entry value must match expected V2 schema for user {}",
                owner_str
            );
        };

        check_user(&user_a, 100);
        check_user(&user_b, 250);
        check_user(&user_c, 500);

        println!("✓ Multi-entry V1 → V2 migration test passed");
    }

    // ─── TEST 17: Multi-Entry State Diff Detection ──────────────────────────

    #[test]
    fn test_multi_entry_state_diff_detection() {
        let env = Env::default();
        env.mock_all_auths();

        let user_a = soroban_sdk::Address::generate(&env);
        let user_b = soroban_sdk::Address::generate(&env);
        let user_c = soroban_sdk::Address::generate(&env);

        let user_a_str = format!("{}", user_a.to_string());
        let user_b_str = format!("{}", user_b.to_string());
        let user_c_str = format!("{}", user_c.to_string());

        let entry_a = make_user_v1_entry(&user_a, 100);
        let entry_b = make_user_v1_entry(&user_b, 250);
        let entry_c = make_user_v1_entry(&user_c, 500);

        let input = MigrationInput {
            contract_id: "test-multi-diff".to_string(),
            wasm: load_multi_entry_v2_wasm(),
            initial_state: vec![entry_a, entry_b, entry_c],
            key_prefix: "User".to_string(),
            migration_fn: "migrate_users".to_string(),
            migration_args: Some(vec![StateValue::Vec(vec![
                StateValue::Address(user_a_str),
                StateValue::Address(user_b_str),
                StateValue::Address(user_c_str),
            ])]),
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(result.success);

        let diff = result.pre_state.diff(&result.post_state);

        assert_eq!(diff.added_count(), 0, "No entries should be added");
        assert_eq!(diff.removed_count(), 0, "No entries should be removed");
        assert_eq!(diff.unchanged_count(), 0, "No entries should be unchanged");
        assert_eq!(
            diff.modified_count(),
            3,
            "Exactly 3 entries must be modified"
        );
        assert!(diff.has_changes(), "Diff must report changes");

        // Verify each modified entry detected the version field addition
        for modified in &diff.modified {
            let version_added = modified.nested_changes.iter().any(|c| {
                matches!(
                    c,
                    state_engine::NestedChange::FieldAdded {
                        field_name,
                        value: StateValue::U32(2)
                    } if field_name == "version"
                )
            });
            assert!(
                version_added,
                "Diff must detect that 'version' field was added with value 2"
            );
        }

        println!("✓ Multi-entry state diff detection test passed");
    }

    // ─── TEST 18: Multi-Entry Partial Migration ─────────────────────────────

    #[test]
    fn test_multi_entry_partial_migration() {
        let env = Env::default();
        env.mock_all_auths();

        let user_a = soroban_sdk::Address::generate(&env);
        let user_b = soroban_sdk::Address::generate(&env);
        let user_c = soroban_sdk::Address::generate(&env);

        let user_a_str = format!("{}", user_a.to_string());
        let user_b_str = format!("{}", user_b.to_string());

        let entry_a = make_user_v1_entry(&user_a, 100);
        let entry_b = make_user_v1_entry(&user_b, 250);
        let entry_c = make_user_v1_entry(&user_c, 500);

        // Migrate only user_a and user_b; user_c remains unmigrated (V1)
        let input = MigrationInput {
            contract_id: "test-partial-migration".to_string(),
            wasm: load_multi_entry_v2_wasm(),
            initial_state: vec![entry_a, entry_b, entry_c],
            key_prefix: "User".to_string(),
            migration_fn: "migrate_users".to_string(),
            migration_args: Some(vec![StateValue::Vec(vec![
                StateValue::Address(user_a_str),
                StateValue::Address(user_b_str),
            ])]),
        };

        let result = MigrationEngine::execute(&input).expect("Migration should not fail");
        assert!(result.success);

        let diff = result.pre_state.diff(&result.post_state);

        assert_eq!(
            diff.modified_count(),
            2,
            "Exactly 2 entries must be modified"
        );
        assert_eq!(
            diff.unchanged_count(),
            1,
            "Exactly 1 entry must remain unchanged (user_c)"
        );

        println!("✓ Multi-entry partial migration test passed");
    }

    // ─── TEST 19: Multi-Entry Determinism ───────────────────────────────────

    #[test]
    fn test_multi_entry_determinism() {
        let env = Env::default();
        let user_a = soroban_sdk::Address::generate(&env);
        let user_b = soroban_sdk::Address::generate(&env);
        let user_c = soroban_sdk::Address::generate(&env);

        let user_a_str = format!("{}", user_a.to_string());
        let user_b_str = format!("{}", user_b.to_string());
        let user_c_str = format!("{}", user_c.to_string());

        let run_multi_entry = || -> MigrationResult {
            let entry_a = make_user_v1_entry(&user_a, 100);
            let entry_b = make_user_v1_entry(&user_b, 250);
            let entry_c = make_user_v1_entry(&user_c, 500);

            let input = MigrationInput {
                contract_id: "test-multi-entry-determinism".to_string(),
                wasm: load_multi_entry_v2_wasm(),
                initial_state: vec![entry_a, entry_b, entry_c],
                key_prefix: "User".to_string(),
                migration_fn: "migrate_users".to_string(),
                migration_args: Some(vec![StateValue::Vec(vec![
                    StateValue::Address(user_a_str.clone()),
                    StateValue::Address(user_b_str.clone()),
                    StateValue::Address(user_c_str.clone()),
                ])]),
            };

            MigrationEngine::execute(&input).expect("Migration should succeed")
        };

        let r1 = run_multi_entry();
        let r2 = run_multi_entry();

        assert!(r1.success);
        assert!(r2.success);

        assert_eq!(
            r1.pre_state.fingerprint(),
            r2.pre_state.fingerprint(),
            "Pre-state fingerprints must be identical across runs"
        );
        assert_eq!(
            r1.post_state.fingerprint(),
            r2.post_state.fingerprint(),
            "Post-state fingerprints must be identical across runs"
        );
        assert_eq!(r1.pre_state.entries, r2.pre_state.entries);
        assert_eq!(r1.post_state.entries, r2.post_state.entries);

        println!("✓ Multi-entry determinism test passed");
    }

    // ─── TEST 20: Multi-Entry Failed Migration ──────────────────────────────

    #[test]
    fn test_multi_entry_failed_migration() {
        let env = Env::default();
        env.mock_all_auths();

        let user = soroban_sdk::Address::generate(&env);
        let entry = make_user_v1_entry(&user, 100);

        // MultiEntryV1Contract has no "migrate_users" function, so invocation fails
        let input = MigrationInput {
            contract_id: "test-multi-fail".to_string(),
            wasm: load_multi_entry_v1_wasm(),
            initial_state: vec![entry],
            key_prefix: "User".to_string(),
            migration_fn: "migrate_users".to_string(),
            migration_args: Some(vec![StateValue::Vec(vec![StateValue::Address(format!(
                "{}",
                user.to_string()
            ))])]),
        };

        let result = MigrationEngine::execute(&input);
        match result {
            Ok(r) => {
                assert!(
                    !r.success,
                    "Migration must fail when function does not exist on V1"
                );
                assert!(r.error.is_some());
                assert_eq!(r.pre_state.entries.len(), 1);
            }
            Err(MigrationError::ContractInvocationFailure(_)) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }

        println!("✓ Multi-entry failed migration test passed");
    }
}
