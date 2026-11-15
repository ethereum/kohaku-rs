# kohaku-tacit-paymaster

Pays for a 4337 operation out of a Tacit pool note, so a new account can be seeded without a funded
account and without a trusted relayer. The paymaster is the withdrawal's relayer, so the pool pays
it the fee in the same call; the rest of the note goes to the recipient, which is the operation's
sender, and the paymaster refunds whatever the gas did not cost.

Needs `TacitPaymaster` from
[kohaku-contracts](https://github.com/ethereum/kohaku-contracts/tree/master/privacy-paymaster), and
takes only a whole-note withdrawal: one that inserts nothing, so its proof cannot go stale.

A Tacit proof binds the fee it was made for, so the fee must be known before the proof exists. The
withdrawal is therefore built twice, once with a dummy proof to size the gas and then with the real
fee, which is why `with_tacit_paymaster` takes a closure rather than a transaction.

```rust,no_run
use kohaku_tacit::{Wallet, circuit::Prover};
use kohaku_tacit_paymaster::UserOperationPaymasterExt;
use kohaku_userop_kit::{builder::UserOperationBuilder, bundler::Bundler};

# async fn example(
#     provider: impl alloy::providers::Provider,
#     bundler: &impl Bundler,
#     builder: UserOperationBuilder<()>,
#     wallet: &Wallet,
#     prover: &Prover,
#     paymaster: alloy::primitives::Address,
#     account: alloy::primitives::Address,
# ) -> Result<(), Box<dyn std::error::Error>> {
let note = wallet.notes().next().expect("a note to spend").value;
let mut rng = rand::rng();

let builder = builder
    .with_tacit_paymaster(
        paymaster,
        |payer| wallet.unshield(account, note - payer.fee, payer, wallet.tree(), &mut rand::rng()),
        prover,
        &provider,
        bundler,
        &mut rng,
    )
    .await?;
# Ok(())
# }
```

## Features

- `parallel`: parallel proving.
