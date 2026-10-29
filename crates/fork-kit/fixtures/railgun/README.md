# Railgun local deployment fixtures

The smart-wallet, Poseidon, WETH and RelayAdapt fixtures support fresh local
Railgun deployments through `deploy_railgun`. Their existing verification keys
remain unchanged.

## Railgun fee adapter

`fee_adapter.json` contains only ABI and creation bytecode from
[`Robert-MacWha/privacy-paymaster` at `5648ec0`](https://github.com/Robert-MacWha/privacy-paymaster/tree/5648ec0117469a2d0b43ed96f0eedaaad789d558).
This is the actual `RailgunFeeAdapter`, not a verifier or paymaster mock.
It follows the existing PrivacyPaymaster/TornadoFeeAdapter fixture layout;
the source SPDX identifier is recorded in the provenance file.

`fee_adapter.provenance.json` records the exact compiler, settings, source
identities and SHA-256s. Reproduce in a disposable checkout of that revision:

```sh
git submodule update --init --recursive
forge build contracts/fee_adapters/railgun/RailgunFeeAdapter.sol \
  --use 0.8.30 --evm-version prague \
  --out out-pinned --cache-path cache-pinned
python3 - <<'PY'
import json
from pathlib import Path
artifact = json.loads(Path('out-pinned/RailgunFeeAdapter.sol/RailgunFeeAdapter.json').read_text())
fixture = {'abi': artifact['abi'], 'bytecode': artifact['bytecode']['object']}
Path('fee_adapter.json').write_text(json.dumps(fixture, indent=2) + '\n')
PY
```

The fixture is constructed for each fresh local smart-wallet address and the
public paymaster master key. Its transactions still pass the deployed wallet's
real verification keys. Only gas simulation uses the production temporary
verifier state-override path; no verifier override is written to chain state.
