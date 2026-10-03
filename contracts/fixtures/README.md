# Soroban Migration Fixtures

These fixtures provide deterministic V1 and V2 Soroban contracts to test the Migration Simulator's abilities.

## The Fixtures

### `migration_v1`
A simple key-value record storage contract. 
- **Schema**: `Record { owner: Address, value: u64 }`
- **Purpose**: Represents an initial application deployment and acts as the "before" state for simulation.

### `migration_v2`
An upgraded version of the `migration_v1` contract.
- **Schema**: `RecordV2 { owner: Address, value: u64, version: u32 }`
- **Purpose**: Introduces a schema change (the addition of a `version` field) that necessitates a migration.
- **Migration Logic**: Contains a `migrate_record` function that explicitly translates the old V1 record into a V2 record in storage.

### `multi_entry_v1`
A multi-entry user account storage contract.
- **Schema**: `UserRecord { owner: Address, balance: u64 }`
- **Storage Key**: `DataKey::User(Address)`
- **Purpose**: Represents an application managing multiple persistent user balances (User A, User B, User C) acting as the initial multi-record state.

### `multi_entry_v2`
An upgraded version of the `multi_entry_v1` contract.
- **Schema**: `UserRecordV2 { owner: Address, balance: u64, version: u32 }`
- **Storage Key**: `DataKey::User(Address)`
- **Purpose**: Introduces schema versioning across multiple persistent storage records.
- **Migration Logic**: Contains `migrate_user(owner)` for single-entry migration and `migrate_users(users)` for batch migration across all user accounts.

## Architecture

```text
V1 Contract (Single or Multi-Entry)
    │
    │ existing state entries
    ▼
Migration
    │
    │ transformed state entries
    ▼
V2 Contract (Single or Multi-Entry)
```

## Invariants

When the migration is simulated, the following invariants **must** hold true:

### Single-Record Fixture
1. `before.owner == after.owner`
2. `before.value == after.value`

### Multi-Entry User Balance Fixture
1. `before[user].owner == after[user].owner` for all users
2. `before[user].balance == after[user].balance` for all users
3. Total number of user entries remains preserved
4. `after[user].version == 2` for all migrated entries

## Building and Testing

These contracts are standard Soroban workspaces. You can test them using:

```powershell
cargo test --workspace
```

To build the Wasm binaries:

```powershell
cargo build --target wasm32v1-none -p migration_v1 -p migration_v2 -p multi_entry_v1 -p multi_entry_v2 --release
```

## Deterministic Data

JSON scenario files model the exact expected before/after state transitions:
- `migration_scenario.json`: Single-entry Record upgrade scenario
- `multi_entry_scenario.json`: Multi-entry User balance upgrade scenario
- `state/v1-state.json` / `state/v2-expected-state.json`: Single-entry contract state snapshots
- `state/multi-entry-v1-state.json` / `state/multi-entry-v2-expected-state.json`: Multi-entry contract state snapshots

