# sonar-sim

Solana transaction simulation engine powered by [LiteSVM](https://github.com/LiteSVM/litesvm).

## Usage

### Pipeline API

```rust
use sonar_sim::{Pipeline, Mutations, SimulationResult};

let result = Pipeline::new("https://api.mainnet-beta.solana.com".into())
    .parse(raw_tx)?           // base64 or base58 encoded transaction
    .load_accounts()?         // fetches all referenced accounts via RPC
    .execute()?;              // runs simulation, returns result with balance changes

println!("success: {}", result.success);
for change in &result.sol_changes {
    println!("{}: {} -> {} ({:+})", change.account, change.before, change.after, change.change);
}
```

### With Mutations

```rust
use sonar_sim::{Pipeline, Mutations};
use sonar_sim::internals::{SolFunding, AccountOverride};

let mutations = Mutations::builder()
    .fund_sol(SolFunding { pubkey: wallet, amount_lamports: 1_000_000_000 })
    .close_account(old_account)
    .add_override(account_override)
    .build();

let result = Pipeline::new(rpc_url)
    .parse(raw_tx)?
    .load_accounts()?
    .with_mutations(mutations)
    .execute()?;
```

### Bundle Simulation

```rust
let bundle = Pipeline::new(rpc_url)
    .parse_bundle(&[tx1, tx2, tx3])?
    .load_accounts()?
    .execute_bundle()?;        // BundleResult<Result<SimulationResult>>, fail-fast

for result in &bundle.executed {
    let sim = result.as_ref().unwrap();
    println!("success: {}", sim.success);
}
if bundle.skipped_count() > 0 {
    println!("{} transactions were skipped due to prior failure", bundle.skipped_count());
}
```

### Configuration

```rust
use std::sync::Arc;
use sonar_sim::{Pipeline, AccountSource, FetchObserver};

let result = Pipeline::new(rpc_url)
    .with_source(Arc::new(my_local_source))   // check local accounts before RPC
    .with_observer(Arc::new(my_observer))      // progress callbacks
    .offline(true)                             // block all RPC calls
    .verify_signatures(true)                   // default: false
    .slot(123456)                              // override SVM slot
    .timestamp(1700000000)                     // override SVM clock
    .parse(raw_tx)?
    .load_accounts()?
    .execute()?;
```

### Intermediate State Access

```rust
let pipeline = Pipeline::new(rpc_url)
    .parse(raw_tx)?;

let parsed = pipeline.parsed().unwrap();  // access ParsedTransaction
// inspect transaction before loading accounts...

let pipeline = pipeline.load_accounts()?;
let resolved = pipeline.resolved().unwrap();  // access ResolvedAccounts

let result = pipeline.execute()?;
```

### Custom RPC Provider (Testing)

```rust
use std::sync::Arc;
use sonar_sim::Pipeline;
use sonar_sim::internals::FakeAccountProvider;

let provider = Arc::new(FakeAccountProvider::from_accounts(vec![
    (pubkey, account_data),
]));

let result = Pipeline::with_provider(provider)
    .parse(raw_tx)?
    .load_accounts()?
    .execute()?;
```

## Public API

| Export | Description |
|--------|-------------|
| `Pipeline` | Fluent simulation API |
| `Mutations` | Consolidated mutation config |
| `MutationsBuilder` | Builder for `Mutations` |
| `SimulationResult` | Execution result with auto-computed balance changes |
| `SolBalanceChange` | SOL balance change for an account |
| `TokenBalanceChange` | Token balance change for an account |
| `AccountSource` | Trait: provide accounts from local sources before RPC |
| `FetchObserver` | Trait: receive account fetch lifecycle events |
| `FetchEvent` | Enum: events emitted during account fetching |
| `SonarSimError` | Error type |
| `Result<T>` | Type alias for `Result<T, SonarSimError>` |

For advanced/low-level access, use `sonar_sim::internals::*`.
