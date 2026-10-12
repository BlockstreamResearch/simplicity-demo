# Allowance manifest in Humid's dialect, with an earlier demo page

This directory holds the allowance contract's manifest written for the Humid browser
extension, the patch Humid needs before it can run the contract, and an early demo page
that drives the extension directly.

```
txmanifest.json              the manifest, in Humid's 0.1.0 dialect
allowance.simf               the contract, byte-identical to ../../contracts/allowance.simf
humid-allowance-fixes.patch  two fixes Humid needs to run this contract at all
dapp-demo.html               an earlier demo page, superseded by ../allowance-humid-only/
HUMID-BUGS.md                the bugs and capability gaps found against Humid
```

For a worked example to read or copy, use `../allowance-humid-only/` instead: it covers
the same lifecycle, is the one `HOWTO.md` walks through, and handles sessions correctly.
What this directory is still the best source for is the manifest dialect, the patch, and
the request shapes documented below.

## Apply the patch first

`humid-allowance-fixes.patch` fixes two bugs, both filed upstream and unfixed as of
2026-10-07. Without it nothing here works past `Fund`:

- Humid refuses BIP68 relative timelocks categorically, so every `PartialWithdraw` and
  `FullWithdraw` fails with `{"reject": "unimplemented-construct"}`. The refusal is
  correct in general, because one shared `nSequence` would wrongly time-lock a separate
  fee-paying input against its own age, but it fires without checking how many inputs the
  transaction has. This contract permits exactly one, via `enforce_single_input()`.
- `FullWithdraw` is refused as "pays nothing, so there is nothing to build", because its
  only output is policy-asset change.

Apply it from the root of a Humid checkout and rebuild:

```
git apply /path/to/humid-allowance-fixes.patch
bun run build        # then reload the extension at chrome://extensions
```

The patch header records the base commits it was generated against, describes both fixes
well enough to redo by hand against a newer Humid, and covers what to do if `git apply`
rejects it. `HOWTO.md` Step 3 covers the same ground in sequence.

## How this manifest differs from `examples/allowance/txmanifest.json`

Both describe the same contract. `examples/allowance/` is dialect `0.2.0`, read by
`tx-manifest-wallet`; this one is `0.1.0`, read by `@humid/tx-manifest`. The differences
are conventions of the reader, not changes to what the contract does.

- **`"manifest_version": "0.1.0"`, not `"0.2.0"`.** Humid's fixtures use `0.1.0`.
- **`"confidential": false` on `utxo_types.allowance`.** Every Humid fixture declares
  this. It is not load-bearing for safety: `evaluation/plan.ts` always passes
  `unblindable: "covenant"` for any output whose destination is a covenant `utxo_type`,
  and `resolveBlinding` in `evaluation/blinding.ts` treats `unblindable` as an
  unconditional override that builds the output explicit, checked before it looks at the
  declared `confidential` value. A manifest omitting the field still gets an explicit
  output for this contract. Declaring it is worth doing anyway: it matches every real
  fixture, and `refuseUnbuildableUtxoType` in `document/refuse.ts` reads it to *reject* a
  manifest that wrongly claims `confidential: true` for a covenant.
- **The `contract_templates` / `actions` container**, not the `classes` / `methods`
  container some other Humid fixtures use. `corpus.test.ts` imports
  `__fixtures__/current/*` separately from the root `__fixtures__/*` ones and tests the
  `current/` set for full-read support; every fixture under `current/` uses
  `contract_templates`.
- **Otherwise structurally the same**: the same three actions (`Fund`, `PartialWithdraw`,
  `FullWithdraw`, still two withdraw actions rather than one witness-selected `SPEND_PATH`,
  which is a property of the contract's logic rather than of the reader), the same
  `sequence.relative_blocks`, the same bare single-`SIGNATURE` witness shape that Humid's
  own `p2pk.manifest.json` fixture uses, and the same `required_index`-plus-array-order
  pattern for the continuation output. That last one matters more here than it does for
  `tx-manifest-wallet`: Humid's `review/index.ts` validates a stated `required_index`
  against the real output position rather than parsing and ignoring it.

Validating this manifest against `examples/allowance/txmanifest.schema.json` reports one
error, on `confidential`. That schema describes the `0.2.0` dialect only, which permits
`confidential` on an output but not on a utxo type. `REFERENCE.md` covers the divergence.

## Checking a manifest without the extension

Humid's `apps/web` ships a Manifest Inspector, a paste-a-manifest developer tool that
runs the same reader the extension uses (`inspectManifestDocument` from
`@humid/tx-manifest`), entirely client-side, with no wallet or network call. It is the
cheapest way to check that a manifest and contract source are accepted at all.

```
cd ~/src/humid
bun run web:dev
```

1. Open the dev server in a browser and click through from the home screen to "Manifest
   Inspector", which is a view switch rather than a separate URL.
2. Paste this directory's `txmanifest.json` into the textarea.
3. Upload `allowance.simf` from this directory with the "Contract sources" file picker.
   It is matched to the manifest's `"./allowance.simf"` reference by filename.
4. Read the verdict, construct table, and rewrite list.

## How Humid exposes itself to a page

A content script injects `window.humid = { request(args), on(args) }` into every page. Its
`matches` covers `file://*/*`, `http://*/*` and `https://*/*`, so a same-machine demo
needs no WalletConnect pairing.

The injected provider exposes exactly six CAIP-25/27 session-management methods, listed in
`core/caip25/methods.ts` and wired up in `createInjectedRpcHandlers`
(`core/extension-background/injected-rpc/index.ts`): `wallet_createSession`,
`wallet_getSession`, `wallet_revokeSession`, `wallet_invokeMethod`, `wallet_addChain` and
`wallet_switchChain`. Nothing else is a valid top-level `method` name. Chain methods,
including `processConfidentialTransaction` and `getWalletDescriptor`, go through
`wallet_invokeMethod` inside a session. Calling one directly returns `No handler for
method: <name>` immediately, with no confirmation prompt.

```js
// once, or whenever a fresh session is wanted:
await window.humid.request({
  method: "wallet_createSession",
  params: {
    requiredScopes: {
      "bip122:a771da8e52ee6ad581ed1e9a99825e5b": {  // Liquid testnet CAIP-2 id,
        methods: ["processConfidentialTransaction"], // from LiquidChain.ts's
        notifications: [],                            // LIQUID_TESTNET_CHAIN_ID
      },
    },
  },
});

// then, for every actual call:
await window.humid.request({
  method: "wallet_invokeMethod",
  params: {
    scope: "bip122:a771da8e52ee6ad581ed1e9a99825e5b",
    request: { method: "processConfidentialTransaction", params: { /* ... */ } },
  },
});
```

`processConfidentialTransaction`'s own params are Zod-validated in
`packages/tx-manifest/src/request/validation.ts`: `{ manifest, contractSources, action,
params?, instance?, state?, broadcast? }`, with `broadcast` defaulting to `false`.

**Create sessions sparingly.** `wallet_createSession` shows a connect prompt and revokes
any existing session first, so calling it before each action destroys a working 30-day
authorization and re-approves it every time. `../allowance-humid-only/index.html` tries
`wallet_invokeMethod` first and creates a session only when a call is actually refused,
which is the shape to copy.

## Request shapes that are easy to get wrong

**`instance` is double-nested; `state` is not.** `normaliseInstance()`
(`packages/tx-manifest/src/document/normalise.ts`) takes the request's `instance` value as
`raw` and reads `raw.instance.fields`, not `raw.fields`:

```js
instance: { instance: { fields: { BENEFICIARY_KEY: "...", /* ... */ } } }
```

Passing `instance: { fields: {...} }`, which is what `tx-manifest-wallet`'s own
`instance.json` file contains, resolves to an empty fields object. The Zod schema does not
catch it, because `instance` is a generic `Record<string, unknown>` at that layer, so it
fails silently and surfaces much later as `"X is neither a parameter nor an argument of
this action"` from code that never names `instance`.

`state` has no such quirk: `review/index.ts` reads `request.state.utxos` directly,
single-level.

Both `instance` and `state` are unconditionally required for the withdraw actions, per
`resolveActionRequirements` in `request/requirements.ts`. Humid neither remembers a prior
`Fund`'s terms nor scans the chain for the right UTXO, so the caller states both on every
call. A minimal `state` is `{utxos: [{txid, vout, utxo_type}]}`, the shape used in Humid's
own `review/covenantSpend.test.ts`. Sending `amount_sat` and `asset` alongside is
unnecessary: re-deriving those from the chain rather than trusting the page's claim is the
point of the design.

## Getting a beneficiary pubkey

Humid shows a wallet's own public key nowhere and has no manifest-native mechanism for
handing one over, so recover it from `getWalletDescriptor` plus a BIP32 child derivation:

1. Call `getWalletDescriptor` with `descriptorType: "publicWalletDescriptor"` and
   `descriptorFormat: [{format: "bip380-split-branches"}]`. The response's
   `descriptors[0].branchDescriptors` holds
   `elwpkh([<fingerprint>/84h/1h/0h]<xpub>/0/*)` for the external chain and `.../1/*)`
   for the internal one: plain BIP-84 segwit v0, not Taproot.
2. The xpub is already at `m/84'/1'/0'`. Derive two more **non-hardened** steps, `/0` for
   the external chain then `/0` for the first address index, to reach the account's first
   receive pubkey.
3. Drop the leading parity byte from the 33-byte compressed pubkey. The remaining 64 hex
   characters are the `BENEFICIARY_KEY` value.

This is the same key `signingDerivationPathOf`
(`processConfidentialTransaction/index.ts`) uses for covenant-witness signing, on the
external chain at that address index. `../allowance-humid-only/index.html` implements the
derivation in about 75 lines, including a CKDpub helper.

## Running `dapp-demo.html`

```
cd examples/allowance-humid
python3 -m http.server 8099
```

Then open `http://localhost:8099/dapp-demo.html` in the browser profile Humid is installed
in. Serving over HTTP rather than opening the file as `file://` avoids inconsistent
browser handling of `fetch()` between sibling local files; the content script matches
`http://` the same as `file://`.

The page persists known values across reloads in `localStorage`: a derived pubkey, `Fund`'s
params, and the covenant address. It derives the covenant address from the `Fund` txid by
reading `vout[0]` of `GET {esplora}/tx/{txid}`, and includes a small client-side indexer
against `blockstream.info`, the Esplora mirror that sends CORS headers for browser use
where `liquid.network` does not. The indexer lists live instances, labels each
`PartialWithdraw` or `FullWithdraw` according to whether its balance is at or below
`ALLOWANCE_AMOUNT`, and offers a Claim button per instance. Humid provides no
address-based UTXO lookup, so an application has to supply one on every architecture.

Its one known defect is session handling: it calls `ensureSession()` before every request,
which means a connect prompt on every action. `../allowance-humid-only/index.html` fixes
that.

A useful order for trying it:

1. Click "Check for `window.humid`" to confirm the extension is enabled and injecting on
   this page. Nothing past this can work until it does.
2. Fund a wallet with testnet coins, through Humid's own receive address and the faucet at
   `https://liquidtestnet.com/api/faucet?address=<addr>&action=lbtc`.
3. Call `Fund` with "broadcast" **unchecked** first. Humid's review layer returns a
   description of the transaction it would build without spending anything, whatever the
   numbers are. This is the cheapest check that the manifest and contract source are
   accepted at all, before risking a real spend.
4. Call `Fund` again with broadcast checked.
5. Try `PartialWithdraw` the same dry-run-then-broadcast way, once `MIN_DISTANCE` blocks
   have passed.
6. For two real parties, use two browser profiles with their own Humid installs rather
   than one wallet playing both roles. Keys should not be shared between parties even in a
   demo.

Humid's Esplora backend is configurable through `chain.settings.backend`, read by
`createEsploraTxOutReader` and `createEsploraFeeRateReader` in the extension's
`processConfidentialTransaction` handler. If it defaults to `blockstream.info`, consider
pointing it at `liquid.network`, which has gentler rate limits.

## Known gaps in the format and its implementations

- **A bare `fee` reference in an `amount_sat` expression is not implemented.**
  `instance.ALLOWANCE_AMOUNT - fee`, the pattern Humid's own
  `current/last_will.manifest.json` uses in `ColdBreak` and `Refresh`, fails with `"fee"
  is neither a parameter nor an argument of this action.` The keyword appears nowhere in
  the package outside tests and fixtures, and nothing in `evaluate.ts`, `plan.ts` or
  `references.ts` resolves it. Since `current/` is the fixture set `corpus.test.ts`
  expects to read in full, this is a gap between what is tested at the document level,
  meaning parsing and normalising, and what works end to end against a live review.

  The manifest here avoids it. An output whose amount is "whatever is left after the other
  named outputs and the fee, returned to the caller's own wallet" is what `destination: "change"`
  with no `amount_sat` already means, as `Fund`'s own `change_out` uses it. Its amount is
  resolved by the wallet's PSET-building step in `assembleReviewedTransaction.ts` rather
  than by the expression evaluator, so no `fee` keyword is needed. Both withdraw actions'
  `beneficiary_payout` is written this way. That is safe for this contract specifically:
  `recursive_covenant()` pins output 0 as the covenant and output 2 as the fee, and never
  constrains output 1's identity or amount, so an auto-placed change output landing there
  is fine. A contract that did pin output 1's value exactly would need the real `fee`
  keyword, so this is a fit to one contract's shape rather than a general substitute.
- **The two implementations have already diverged.** This contract's manifest is `0.2.0`
  for `stringhandler/txmanifest-wallet` and `0.1.0` for Humid, and they disagree on at
  least one schema detail, namely where `confidential` may appear.
- **Humid's fixture corpus disagrees with itself in both directions.** For the same three
  protocols (`last_will`, `dex`, `vaultlet`), the root-level fixture uses
  `classes`/`methods` while the `current/` copy of the same protocol uses
  `contract_templates`/`actions`. `vaultlet`'s root copy is stamped `manifest_version:
  "1.0"` while every other fixture checked, root or current, is `"0.1.0"`. From outside it
  is not clear whether `classes`/`methods` is a retired name, a proposed replacement, or
  an intentionally parallel form, which anyone writing a new manifest has to guess at.
- **One structural safeguard works in the format's favour.** Humid's `unblindable:
  "covenant"` rule means a manifest author who forgets to mark a covenant output
  non-confidential cannot cause a stuck-funds outcome in Humid, which is a real mitigation
  for exactly the risk the txmanifest documentation names when it says the format has no
  means to confirm or verify the correctness of descriptions.
