# account-checks

Explicit account freshness checks for privacy applications.

`is_address_unused` checks for current code/delegation, an ETH balance, or a
nonzero native transaction nonce. `is_sender_unused` checks `EntryPoint` nonce key
zero. Both return errors when a check fails or the provider cannot read state.
They do not enumerate ERC-20 balances, other `EntryPoint` nonce keys, or historical
activity that leaves no current state signal.

Applications can apply these checks before proving or signing, and skip them
only when their caller explicitly opts into account reuse:

```rust,no_run
use alloy::{primitives::Address, providers::Provider};
use kohaku_account_checks::{is_address_unused, is_sender_unused};

async fn check_account(
    provider: &impl Provider,
    entry_point: Address,
    sender: Address,
    allow_used: bool,
) -> Result<(), kohaku_account_checks::Error> {
    if !allow_used {
        is_address_unused(provider, sender).await?;
        is_sender_unused(provider, &entry_point, &sender).await?;
    }
    Ok(())
}
```

Generic ERC-4337 account construction does not apply these privacy policies.
