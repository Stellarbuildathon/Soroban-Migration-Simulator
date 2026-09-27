# Soroban Migration Simulator

**Simulate · Verify · Upgrade Safely**

An open-source developer tool for the Stellar/Soroban ecosystem that locally simulates contract migration behavior and analyzes state changes before deployment.

**Status: Experimental / Early Development**

---

## The Problem

When upgrading Soroban smart contracts, developers need confidence that:

```
existing state
      ↓
migration logic
      ↓
new state
```

...will behave as expected. A migration that silently corrupts storage, loses data, or produces unexpected state can be catastrophic on-chain. SMS lets you simulate the entire migration pipeline locally, capture real ledger state before and after execution, and produce a structured diff of exactly what changed.

## What SMS Does Today

- **Soroban WASM analysis** — Inspects compiled `.wasm` binaries to extract contract specifications, function signatures, and type definitions from embedded XDR.
- **Storage analysis** — Infers persistent storage models from contract interfaces (key types, value types, durability).
- **Normalized state representation** — Represents contract state as a canonical `StateValue` / `ContractState` model (Address, U32, U64, Bool, Symbol, Bytes, Struct, Map, Vec, Option).
- **Local Soroban migration execution** — Registers WASM contracts in a local `soroban_sdk::Env`, seeds real `ContractData` entries, and invokes actual migration functions.
- **Ledger snapshot state capture** — Captures pre-migration and post-migration state by inspecting `to_ledger_snapshot()` and extracting `ContractData` entries.
- **Pre/post migration state comparison** — Produces a structured `StateDiff` identifying added, removed, modified, and unchanged entries.
- **Nested state change detection** — Detects field-level changes within Structs, index-level changes within Vecs, and key-level changes within Maps.
- **Deterministic state fingerprints** — SHA-256 fingerprints of canonical state for deterministic comparison.
- **Migration fixture testing** — Built-in V1 → V2 migration fixtures that demonstrate real contract execution.

## Current Limitations

- **Migration simulation, not full upgrade lifecycle**: SMS currently simulates migration execution and state transformation locally. Full Soroban executable upgrade lifecycle simulation (WASM replacement, on-chain upgrade orchestration) is future work.
- **Local simulation only**: All execution happens in a local test environment. No production ledger state is fetched.
- **Simulated authorization**: `mock_all_auths()` is used for local testing. Production authorization requires real signatures.
- **Storage-key configuration**: The migration engine requires a `key_prefix` parameter to construct storage keys matching the contract's enum variant encoding.
- **No production RPC**: No Horizon, RPC, testnet, or mainnet integration.
- **One-level nested diffing**: The state diff engine decomposes Struct/Vec/Map at one level of nesting.

## End-to-End Example

The repository includes a V1 → V2 migration fixture. Here's what happens conceptually:

```
V1 (before migration)          V2 (after migration)
─────────────────────          ──────────────────────
owner: Address(X)              owner: Address(X)     ✓ preserved
value: U64(100)                value: U64(100)       ✓ preserved
                               version: U32(2)       + added
```

SMS captures the actual ledger state before and after the migration function executes, then produces:

```
StateDiff:
  modified: 1 entry
    - owner: unchanged
    - value: unchanged
    - version: added (U32(2))
```

### Running the Example

```bash
# Build the Soroban fixture contracts
cargo build --target wasm32v1-none -p migration_v1 -p migration_v2 --release

# Run the migration integration tests
cargo test -p migration-engine -- --nocapture

# Run all tests across the workspace
cargo test --workspace
```

## Quick Start

### Requirements

- **Rust** 1.96.0+ (stable)
- **Cargo** (included with Rust)
- **Soroban/Stellar CLI** — Required for building WASM fixtures: `cargo install --locked stellar-cli`

### Build

```bash
cargo build --workspace
```

### Test

```bash
cargo test --workspace
```

### Quality Checks

```bash
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Architecture

See [docs/architecture.md](docs/architecture.md) for the full pipeline description.

```
Phase 1: Static WASM Analysis    (analyzer, wasm-diff, storage-analyzer)
Phase 2: State Engine             (state-engine)
Phase 3: Migration Execution      (migration-engine)
Phase 4: State Diff               (state-engine)
```

Each phase is a distinct, testable crate in the workspace.

## Workspace Structure

```
crates/
├── analyzer/            # WASM binary analysis (contractspecv0 XDR extraction)
├── cli/                 # CLI (not yet implemented)
├── invariant-engine/    # Invariant validation (placeholder)
├── migration-engine/    # Local Soroban migration execution and state capture
├── report/              # Report generation (placeholder)
├── simulator/           # Top-level orchestrator (placeholder)
├── state-engine/        # State representation, fingerprints, and diff engine
├── storage-analyzer/    # Storage model inference from WASM analysis
└── wasm-diff/           # WASM interface diffing

contracts/fixtures/
├── migration_v1/        # V1 contract fixture
├── migration_v2/        # V2 contract fixture with migration function
└── state/               # JSON state fixtures (v1-state.json, v2-expected-state.json)

docs/
├── architecture.md      # System architecture documentation
├── getting-started.md   # Contributor getting-started guide
└── migration-guide.md   # Migration simulation model documentation
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow, branching conventions, and contribution guidelines.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for completed work and future contribution opportunities.

## Contributors

Thanks to everyone helping build SMS! See [CONTRIBUTORS.md](CONTRIBUTORS.md) for the full list.

<!-- ALL-CONTRIBUTORS-BADGE:START -->
[![Contributors](https://img.shields.io/github/contributors/Chigybillionz/Soroban-Migration-Simulator?style=flat-square&color=blue)](CONTRIBUTORS.md)
<!-- ALL-CONTRIBUTORS-BADGE:END -->

Want to see your profile here? Check out [CONTRIBUTING.md](CONTRIBUTING.md) to get started, then add yourself to [CONTRIBUTORS.md](CONTRIBUTORS.md).

## License

SMS is licensed under the [Apache License, Version 2.0](LICENSE).

You may not use this file except in compliance with the License. You may obtain a copy of the License at <http://www.apache.org/licenses/LICENSE-2.0>.

## Security

See [SECURITY.md](SECURITY.md) for responsible disclosure guidelines.

---

SMS is being developed as an open-source Soroban developer infrastructure project and is intended to grow through community contributions. It is **not** an official Stellar project and does **not** guarantee migration safety. Simulation results depend on the supplied contract, migration logic, state fixtures, and assumptions.
