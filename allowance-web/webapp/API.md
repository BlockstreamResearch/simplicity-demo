# Allowance web app: backend API (as actually built)

Supersedes an earlier version of this document written before the actual backend
approach was decided (a hand-rolled Rust server doing PSET construction/signing
itself; see `webapp/backend/STATUS.md`, unverified, abandoned in favor of this). The
real implementation is `webapp/server/` (Python, stdlib only), which drives the real
`github.com/stringhandler/txmanifest-wallet` engine as a subprocess for every
contract-touching operation rather than reimplementing any of that logic. Full
reasoning trail: `logs/LOG.md` Steps 4-7.

Run: `python3 webapp/server/app.py [--port 8787] [--data-dir ./data]`. Requires the
`tx-manifest-wallet` binary (see `webapp/server/README.md` for how to get it) on
`$TMW_BIN` or at `webapp/server/bin/tx-manifest-wallet`.

**One wallet per running instance.** Each party (benefactor, beneficiary) runs their
own copy of this app with their own local `wallet.json` under `--data-dir`. There is
no multi-tenant key storage. See `logs/LOG.md` Step 5 for the custody reasoning.

All endpoints are same-origin with the statically-served frontend (`webapp/frontend/`,
served at `/`), so no CORS setup is needed.

## GET /api/wallet

Returns (creating the wallet first if this is the first call for this data dir):
```json
{
  "network": "liquidtestnet",
  "fingerprint": "...",
  "receive_address": "tlq1...",
  "signing_pubkey": "<64 hex>",
  "oracle_pubkey": "<64 hex>",
  "balance_sat": 0,
  "utxo_count": 0
}
```

## POST /api/wallet/faucet

No body. Requests testnet funds to this wallet's receive address from
`liquidtestnet.com`'s faucet (a third-party service; see `logs/LOG.md` Step 2/7 for
why this can be slow/unreliable). Returns `{"txid": "...", "raw": {...upstream body}}`
or 502 on faucet failure.

## POST /api/wallet/sync

No body. Re-syncs this wallet's balance against Esplora. Returns
`{"balance_sat": N, "utxo_count": N}`.

## POST /api/contracts

Benefactor creates and funds a new contract instance in one step (this engine funds
directly from its own wallet, and there is no separate "send to this address from
anywhere" step, unlike the original Rust-backend design; see `logs/LOG.md` Step 6).

Request:
```json
{
  "allowance_amount_sats": 5000,
  "min_distance_blocks": 10,
  "beneficiary_pubkey_hex": "<64 hex, from the beneficiary's own GET /api/wallet>",
  "fund_amount_sats": 20000
}
```

Response:
```json
{
  "contract_id": "a1b2c3d4",
  "address": "tex1p...",
  "fund_txid": "<64 hex>",
  "params": {
    "allowance_amount_sats": 5000,
    "min_distance_blocks": 10,
    "beneficiary_pubkey_hex": "..."
  },
  "fund_amount_sats": 20000
}
```

400 on invalid params (non-positive amounts, out-of-range distance, malformed
pubkey), 502 if the underlying `Fund` action fails (insufficient wallet balance,
faucet funds not yet confirmed, and so on; the error message is the tool's own).

## GET /api/contracts, GET /api/contracts/:id

List / fetch this installation's own previously-created contracts (its local
bookkeeping only; a beneficiary who received a handoff blob out of band doesn't need
these, they use `/api/lookup` directly against the address instead).

## GET /api/lookup?address=&allowance_amount_sats=&min_distance_blocks=

Stateless indexer. Works for any address, regardless of which installation (if any)
created it. Queries Esplora directly for that address's UTXOs (see `logs/LOG.md`
Step 3(A) for why this had to be built new: the reference tooling had no
address-based lookup at all). Returns:
```json
{
  "address": "tex1p...",
  "tip_height": 123456,
  "instances": [
    {
      "txid": "...", "vout": 0, "value_sats": 20000, "blinded": false,
      "confirmed": true, "block_height": 123400,
      "blocks_since_confirm": 12, "blocks_required": 10,
      "withdrawable_now": true, "action": "PartialWithdraw"
    }
  ]
}
```
`blocks_since_confirm` counts the funding block itself, matching BIP68's "distance".

`blinded` is true for a confidential output sent to the covenant address. Esplora
reports those with a `valuecommitment` and no `value`, so `value_sats` and `action`
are both `null` and `withdrawable_now` is false: the contract reads its own asset and
amount directly, which a blinded output does not expose, so nothing can ever spend it.
Such an output is listed rather than skipped, because the funds really are there and
really are stuck, and a caller that tried to withdraw one would otherwise get a
timelock error that never stops being true.

`action` is `"FullWithdraw"` once `value_sats <= allowance_amount_sats` (claiming it
empties the instance), else `"PartialWithdraw"`, since allowance.simf has no
witness-selected spend path, so the manifest exposes these as two separate actions
and the caller must pick the one the current balance makes valid (see
`logs/txmanifest-implementation-log.md`).

## POST /api/claim

Beneficiary claims. Requires this installation's own wallet key to match
`beneficiary_pubkey_hex` (400 with a clear message otherwise, since this app never signs
on behalf of a key it doesn't hold). Re-derives eligibility server-side rather than
trusting the client's claimed action or timelock status.

Request:
```json
{
  "address": "tex1p...",
  "allowance_amount_sats": 5000,
  "min_distance_blocks": 10,
  "beneficiary_pubkey_hex": "<must match this installation's own wallet>",
  "txid": "...", "vout": 0,
  "action": "PartialWithdraw"
}
```

Response: `{"kind": "PartialWithdraw", "txid": "...", "broadcasted": true}`, or 400/502
with a specific error (timelock not satisfied, UTXO already spent, wrong action for
current balance, broadcast failure).

## GET /api/manifest

Returns `{"manifest": <the real txmanifest.json content>, "status": "real: ..."}`.
Unlike the original design (a speculative, illustrative manifest; see
`docs-notes/txmanifest-notes.md` for that history), this is the actual manifest
driving this app's own backend.
