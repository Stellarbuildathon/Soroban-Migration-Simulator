# Migration Guide

This document explains how SMS simulates Soroban contract migrations and the distinction between migration simulation and full contract upgrade simulation.

## Migration Simulation Model

SMS currently supports **migration simulation**: executing migration logic locally and inspecting the resulting state changes. This is the core workflow:

```
Old contract/state
       ↓
State preparation (seed ContractData)
       ↓
Migration execution (invoke contract function)
       ↓
Ledger snapshot (to_ledger_snapshot)
       ↓
Post-state capture (extract ContractData)
       ↓
State diff (compare pre vs post)
```

### What This Proves

- The migration function executes without panicking
- Storage mutations happen as expected
- New fields appear, old fields are preserved or removed
- The resulting state matches expectations

### What This Does NOT Prove

- On-chain WASM replacement will succeed
- The full upgrade lifecycle will complete
- Production authorization will pass
- Network-level upgrade mechanics work correctly

## Full Contract Upgrade Simulation

Full Soroban contract upgrade simulation would involve:

1. **WASM hash replacement** — Updating the contract's executable hash on-chain
2. **Constructor invocation** — Running the new contract's constructor
3. **Ledger entry updates** — Modifying the contract instance entry
4. **Upgrade orchestration** — Managing the sequence of on-chain operations

SMS does **not** currently implement this. The current milestone focuses on migration execution and state transformation.

## The Pipeline in Detail

### 1. State Preparation

The migration engine seeds actual Soroban `ContractData` entries into a local test environment:

```rust
let input = MigrationInput {
    contract_id: "my-contract".to_string(),
    wasm: v2_wasm_bytes,
    initial_state: vec![StateEntry {
        durability: Durability::Persistent,
        key: StateValue::Address("GAAAA...".to_string()),
        value: StateValue::Struct(fields),
    }],
    key_prefix: "Record".to_string(),  // matches DataKey::Record variant
    migration_fn: "migrate_record".to_string(),
    migration_args: None,
};
```

The `key_prefix` parameter tells the engine how to construct the Soroban storage key. For a contract with `DataKey::Record(Address)`, the engine builds `Vec[Symbol("Record"), Address]`.

### 2. Migration Execution

The engine registers the V2 WASM contract and invokes the migration function:

```rust
let result = MigrationEngine::execute(&input)?;
```

This uses `env.invoke_contract()` to call the actual contract code. Real Soroban host execution occurs — no faking.

### 3. Ledger Snapshot

Before and after migration, the engine captures the full ledger state:

```rust
let snapshot = env.to_ledger_snapshot();
```

This produces a `LedgerSnapshot` containing all `ContractData` entries, which the engine filters by contract ID, durability, and entry type.

### 4. State Capture

The engine converts `ScVal` (XDR) to `StateValue` (normalized representation):

```
ScVal::Address → StateValue::Address
ScVal::U64     → StateValue::U64
ScVal::Map     → StateValue::Struct
ScVal::Vec     → StateValue::Vec
...
```

### 5. State Diff

The engine compares pre-state and post-state using `ContractState::diff()`:

```rust
let diff = pre_state.diff(&post_state);
// diff.added     — entries only in post
// diff.removed   — entries only in pre
// diff.modified  — entries with changed values (includes nested_changes)
// diff.unchanged — identical entries
```

### 6. Migration Report Generation

The `report` crate converts migration results and deterministic state differences into structured, developer-readable reports designed for CLI integration and automation workflows.

```rust
use report::MigrationReport;

// Generate report from a migration result
let report = MigrationReport::from_result(
    "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
    "migrate_record",
    &result,
);

// Or generate directly from input and result:
let report = MigrationReport::from_execution(&input, &result);

// Serialize to deterministic, pretty-printed JSON
let json_output = report.to_json_pretty()?;

// Print human-readable report for terminal display (also available via Display trait)
println!("{}", report);
```

## Migration Report Format

The migration report captures metadata, state change counts, and field-level differences in a structured format:

```rust
pub struct MigrationReport {
    pub contract_id: String,
    pub migration_function: String,
    pub success: bool,
    pub duration_ms: u64,
    pub summary: DiffSummary,
    pub changes: StateDiff,
    pub error: Option<String>,
}
```

### Report Metadata
- `contract_id`: Unique identifier of the contract migrated.
- `migration_function`: Name of the migration entrypoint executed.
- `success`: Execution status (`true` if migration succeeded without panic/error).
- `duration_ms`: Total execution duration in milliseconds (pre-capture + execution + post-capture).
- `error`: Optional error details if the migration failed (omitted from serialized JSON on success).

### State Change Summary (`DiffSummary`)
- `added`: Number of new storage entries created.
- `removed`: Number of storage entries deleted.
- `modified`: Number of storage entries whose values changed.
- `unchanged`: Number of storage entries whose values remained identical.

### Detailed State Differences (`StateDiff`)
- `added`: List of `StateEntry` items present only in post-state.
- `removed`: List of `StateEntry` items present only in pre-state.
- `modified`: List of `ModifiedEntry` items with before/after values and structured `nested_changes` detailing:
  - `FieldAdded`: Newly introduced fields in structs.
  - `FieldRemoved`: Removed fields from structs.
  - `FieldModified`: Fields whose values changed.
- `unchanged`: List of `StateEntry` items unchanged by the migration.

### Example JSON Output (Successful Migration)

```json
{
  "contract_id": "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
  "migration_function": "migrate_record",
  "success": true,
  "duration_ms": 42,
  "summary": {
    "added": 0,
    "removed": 0,
    "modified": 1,
    "unchanged": 1
  },
  "changes": {
    "added": [],
    "removed": [],
    "modified": [
      {
        "key": {
          "type": "Address",
          "value": "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH"
        },
        "before": {
          "type": "Struct",
          "value": {
            "owner": {
              "type": "Address",
              "value": "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH"
            },
            "value": {
              "type": "U64",
              "value": 100
            }
          }
        },
        "after": {
          "type": "Struct",
          "value": {
            "owner": {
              "type": "Address",
              "value": "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH"
            },
            "value": {
              "type": "U64",
              "value": 100
            },
            "version": {
              "type": "U32",
              "value": 2
            }
          }
        },
        "nested_changes": [
          {
            "FieldAdded": {
              "field_name": "version",
              "value": {
                "type": "U32",
                "value": 2
              }
            }
          }
        ]
      }
    ],
    "unchanged": [
      {
        "durability": "Persistent",
        "key": {
          "type": "Symbol",
          "value": "Admin"
        },
        "value": {
          "type": "Address",
          "value": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN7"
        }
      }
    ]
  }
}
```

### Example JSON Output (Failed Migration)

```json
{
  "contract_id": "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
  "migration_function": "migrate_record",
  "success": false,
  "duration_ms": 12,
  "summary": {
    "added": 0,
    "removed": 0,
    "modified": 0,
    "unchanged": 2
  },
  "changes": {
    "added": [],
    "removed": [],
    "modified": [],
    "unchanged": [
      {
        "durability": "Persistent",
        "key": {
          "type": "Address",
          "value": "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH"
        },
        "value": {
          "type": "Struct",
          "value": {
            "owner": {
              "type": "Address",
              "value": "GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH"
            },
            "value": {
              "type": "U64",
              "value": 100
            }
          }
        }
      }
    ]
  },
  "error": "Contract invocation failed: host panic: function 'migrate_record' does not exist on contract"
}
```

### Developer-Readable CLI Output Example

```text
================================================================================
                    SOROBAN MIGRATION SIMULATION REPORT
================================================================================
Contract ID:        CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
Migration Function: migrate_record
Execution Status:   SUCCESS
Duration:           42 ms

State Change Summary:
  Added Entries:    0
  Removed Entries:  0
  Modified Entries: 1
  Unchanged:        1
  Total Changes:    1

Detailed State Differences:
  [MODIFIED ENTRIES] (1)
    * Key: Address("GBZXN7PIRZGNMHGA728VY3CWVSCCSTKQQRMAIVYEQYBNVV5JAXFVOHDH")
      + Field Added: 'version' = U32(2)
================================================================================
```


## Creating a Custom Migration Test

1. **Define your contract** with `#[contract]` and `#[contractimpl]`.
2. **Include a migration function** that reads old state and writes new state.
3. **Build the WASM**: `cargo build --target wasm32v1-none -p your-contract --release`
4. **Write a test** using `MigrationEngine::execute()` with appropriate `MigrationInput`.
5. **Assert on the diff** to verify expected state changes.

## StateValue Types

| Type | Soroban Equivalent | Example |
|---|---|---|
| `Address(String)` | `ScVal::Address` | Account or contract address |
| `U32(u32)` | `ScVal::U32` | Small integers |
| `U64(u64)` | `ScVal::U64` | Large integers, timestamps |
| `Bool(bool)` | `ScVal::Bool` | Flags |
| `Symbol(String)` | `ScVal::Symbol` | Short identifiers |
| `Bytes(String)` | `ScVal::Bytes` | Raw hex-encoded bytes |
| `Struct(BTreeMap)` | `ScVal::Map` | Contract types (Record, etc.) |
| `Map(Vec<(SV,SV)>)` | `ScVal::Map` | Generic key-value maps |
| `Vec(Vec<SV>)` | `ScVal::Vec` | Ordered collections |
| `Option(Option<Box<SV>>)` | `ScVal::Void` / inner | Nullable values |
