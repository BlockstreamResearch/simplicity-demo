# Allowance web app: the server

Thin Python (stdlib only, nothing to `pip install`) backend that drives the real
`tx-manifest-wallet` engine as a subprocess for every contract-touching operation.
See `webapp/API.md` for the endpoint contract and `logs/LOG.md` Steps 4-7 for why
this approach was chosen over a hand-rolled Rust backend (short version: this
sandbox has no C compiler; `tx-manifest-wallet` ships prebuilt binaries and is a
better fit for the project's txmanifest goal anyway).

## Setup

1. Download the `tx-manifest-wallet` binary for your platform from
   https://github.com/stringhandler/txmanifest-wallet/releases (Linux:
   `tx-manifest-wallet-<version>-x86_64-unknown-linux-gnu.tar.gz`). Not vendored into
   this repo (third-party binary, ~11MB).
2. Extract it and either put it at `webapp/server/bin/tx-manifest-wallet`, or point
   `TMW_BIN` at wherever you put it.
3. Run: `python3 webapp/server/app.py --port 8787 --data-dir ./data`

Each running instance is meant for **one party** (a benefactor or a beneficiary). It
creates and uses exactly one local `wallet.json` under `--data-dir`. To try both
sides yourself for testing, run two instances on two ports with two data dirs.

## Files

- `tmw.py`: subprocess wrapper around the binary, including the pty driver needed
  because `run` refuses non-interactive stdin (see its module docstring, and
  `logs/LOG.md` Step 7 for a real concurrency bug that was found and fixed here).
- `wallet_store.py`: this installation's own wallet (create-if-missing, info, sync).
- `indexer.py`: direct Esplora address-UTXO lookups; independent of any wallet or
  tx-manifest-wallet state file, since a beneficiary who didn't create the contract
  has no such state file (see `logs/LOG.md` Step 3(A)).
- `faucet.py`: thin proxy to the third-party liquidtestnet.com faucet.
- `app.py`: the HTTP server (stdlib `http.server`, no framework) and static
  frontend serving.

## Known limitation

`sync`/broadcast calls depend on `https://blockstream.info/liquidtestnet/api`, a
single shared public Esplora instance not operated by this project. It rate-limited
during this project's own testing after moderate use (see `logs/LOG.md` Step 7).
A production deployment should point at infrastructure it controls or trusts (see
`docs-notes/mainnet-notes.md`, "Indexer robustness").
