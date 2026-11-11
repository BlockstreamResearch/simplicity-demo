# Allowance covenant built on `lwk_wasm`: single-page demo

The whole allowance lifecycle in one static HTML file: compile `allowance.simf` in the
browser, derive its covenant address, fund it, index live instances, and withdraw.

No backend, no browser extension, no `txmanifest.json`, no build step, no framework.
The architecture is a static page plus one wasm library,
which is how `simplicity-lending` (Blockstream's own Simplicity app) builds its wallet
layer, minus the React/Vite/Postgres apparatus that a production app needs and a demo
does not.

> **Status: partly verified.** The single-wallet path through steps 1-6 was run in a browser
> and reported working on 2026-10-07, so the library bindings, the in-browser compile, the
> address derivation, funding, indexing and claiming are all real rather than theoretical.
> The two-party additions made after that run (the terms handoff in step 4 and the
> payout-address override in step 6) have **not** been exercised. Every call was written
> against the generated `lwk_wasm.d.ts` of a real `--features simplicity` build, and every
> class and static method the page uses is checked to exist in those typings, but names
> resolving is not the same as semantics being right. Each numbered step is an independent
> checkpoint and prints raw errors on purpose.

Liquid **testnet** only. The wallet mnemonic and the beneficiary's secret key are stored in
`localStorage` in plaintext, so use the page only with testnet funds.

## Setup

You need a `lwk_wasm` build **with the Simplicity feature enabled**. It is behind a cargo
feature flag that is off by default and marked unstable upstream:

```toml
# lwk_wasm/Cargo.toml
# Experimental: API behind this feature is unstable and may change without notice.
simplicity = ["lwk_simplicity"]
```

1. Build it, from an `lwk` checkout at the commit the page is written for,
   **into its own output directory**:
   ```
   cd <lwk>
   git checkout 6412ff6bfd823a0cd36d4d07d2f5aee01d6c9c01
   cd lwk_wasm
   wasm-pack build --release --target web --out-dir pkg-web -- --features simplicity
   ```
   `--target web` matters: it emits an ES module with a default `init` export that a plain
   `<script type="module">` can load. The `bundler` target expects Vite or webpack.

   **`--out-dir pkg-web`** keeps this build apart from the default `pkg/`, which holds the
   *bundler*-target build that other projects in the same checkout may depend on. Building
   the web target into `pkg/` would overwrite theirs.

   Builds take a while: `wasm-opt` alone runs for minutes on a module this size. To skip
   it while iterating, either use `--dev` (no `wasm-opt`, debug build, slower at runtime)
   or set `[package.metadata.wasm-pack.profile.release] wasm-opt = false`.

2. Copy the two files this page needs into `vendor/` (git-ignored, since the wasm is about 15 MB):
   ```
   mkdir -p vendor
   cp <lwk>/lwk_wasm/pkg-web/lwk_wasm.js vendor/
   cp <lwk>/lwk_wasm/pkg-web/lwk_wasm_bg.wasm vendor/
   ```
   Once copied, `vendor/` is independent: rebuilding anything in the `lwk` checkout cannot
   affect this demo.
   Only those two. `lwk_wasm.js` is self-contained and resolves the `.wasm` relative to
   its own URL; `lwk_wasm_bg.js` belongs to the bundler target and is not used here.

3. Serve it over HTTP, not `file://`, which breaks ES module imports and `fetch()` of
   the contract source:
   ```
   python3 -m http.server 8099
   ```
   Then open <http://localhost:8099/index.html>.

Alternatively, `@lilbonekit/lwk-web` on npm is the same library pre-built with this
feature on, which is what `simplicity-lending` depends on. If you install that instead,
point the import in `index.html` at wherever your package manager puts it.

## What the page does, step by step

1. **Load the library**: initializes the wasm and asserts the Simplicity classes are
   actually present, with the rebuild command in the error if they aren't. This is the
   check worth running first: a feature-off build fails here rather than with a less clear error later.
2. **Wallet**: a single-signature LWK wallet (`Signer` → `wpkhSlip77Descriptor` → `Wollet`),
   with an Esplora scan, balance, and a faucet button.
3. **Beneficiary key**: `Keypair` from 32 random bytes, exposing `.xOnlyPublicKey` as
   `BENEFICIARY_KEY` and signing the witness with `.signSchnorr()`. The browser-extension
   path has no API that exposes your own pubkey, so there the key has to be recovered by
   BIP32-deriving a BIP84 descriptor by hand.
4. **Compile**: the three compile-time params become `SimplicityArguments`
   (`fromU256Hex` for the pubkey, `fromU64` for the amount, `fromU16` for the distance),
   then `SimplicityProgram.load(source, args)` and `createP2trAddress(NUMS_KEY, network)`.
   The internal key is BIP341's unspendable NUMS point, so the Simplicity script path is
   the only way to spend.
5. **Fund**: `TxBuilder.addExplicitRecipient(...)`, deliberately *not*
   `addLbtcRecipient`. The contract reads its own asset and amount, so a confidential
   output to this address is unspendable forever. **Broadcast for real** starts selected, and
   a real Fund shows a confirmation with the amount, address, terms and fee before it signs
   and sends; clear the box for a dry run.
6. **Live instances**, the indexer that neither library provides: `/address/{a}/utxo`
   plus `/blocks/tip/height`, classifying each UTXO independently by balance against
   `ALLOWANCE_AMOUNT` and depth against `MIN_DISTANCE`. Every UTXO at the address is its
   own allowance with its own timelock; they are never summed and can never be merged.
7. **Withdraw**: builds the PSET by hand, because the covenant input is not a wallet
   UTXO: `PsetInputBuilder.fromPrevout(...).witnessUtxo(...).sequence(TxSequence.fromHeight(MIN_DISTANCE))`,
   then explicit outputs in the order `recursive_covenant()` requires (continuation at 0,
   payout at 1, fee at 2; an Elements fee output is an explicit output with an empty
   script), then `getSighashAll` → `signSchnorr` → `run` locally → `finalizeTransaction`
   → `broadcastTx`. The three Simplicity calls take the spend's control block, from
   `program.controlBlock(internalKey)`.

## The two signing paths

The page can use the same contract in two ways, chosen in the "Who signs" section.

**This page** (default): `lwk_wasm` compiles the contract, derives the address, builds the
PSET, signs and broadcasts, all in the browser. Keys are stored in `localStorage`. This path doesn't use an
extension or a manifest.

**Humid extension**: the page passes Humid `txmanifest.humid.json` plus the contract source
and it does the compiling, signing and broadcasting itself, showing its own confirmation.
This is the more realistic architecture: a real wallet that has never seen this covenant
before learns it from a manifest. Requirements and caveats:

- Humid must be installed in this browser profile and injecting into the page, so serve over
  `http://`, not `file://`.
- **It needs Humid 1.1.5 or newer.** Earlier releases refuse every withdrawal of this
  contract.
- Humid signs with its own BIP84 key at `m/84'/1'/0'/0/0`, which it exposes nowhere in its
  UI. "Get my key from Humid" recovers it by calling `getWalletDescriptor` and doing two
  non-hardened BIP32 steps, printing the raw descriptor alongside so the derivation is
  checkable. The curve math is a dynamic `import()` from a CDN, so the local-only path keeps
  working with no network.
- **The two paths cannot share an allowance.** Different signer means a different
  `BENEFICIARY_KEY`, which means a different compiled address. Pick the signer before
  compiling.
- The indexer is this page's either way: Humid has no address-based UTXO lookup.

On the manifest format, checked against Humid's source rather than assumed: Humid
reads `manifest_version` but never validates it (`corpus.test.ts` deletes the field to prove
it isn't load-bearing), and `document/registry.ts` (a machine-readable list of which fields
it reads, shows, ignores or hasn't implemented) accepts both the `contract_templates` and
the older `classes` container. Every field in `txmanifest.humid.json` is one Humid
reads, with no unknown or ignored keys. So the `0.1.0`-versus-`0.3.0` difference against
`tx-manifest-wallet` is cosmetic for Humid's purposes.

## Running it as two separate parties

The page provides both roles, one per tab, and the realistic test is two browser profiles:
each profile has its own `localStorage` (and, in Humid mode, its own Humid), so each holds
exactly one party's keys.

The beneficiary's key has to come from whatever will sign the claim. The benefactor's
signer doesn't matter, since funding is only a payment to the covenant address, so the two
parties can use different signers.

**Beneficiary**, in a separate profile, gets the beneficiary key and sends the 64 hex characters to
the benefactor:

- Signing with the page: on **I'm the beneficiary**, press **Generate / show my key**. This
  needs no wallet, mnemonic or funds.
- Signing with Humid: under "Who signs", select **Humid extension**, press **Check for
  Humid**, then **Get my key from Humid** in the beneficiary's Humid.

**Benefactor**, in another profile: on **I'm the benefactor**, paste that key into **Beneficiary's
public key**, set the amount and interval, press **Compile & derive address**, then
**Copy** under "Hand the terms to the beneficiary" and send the terms. Fund the
allowance from either signer. In Humid mode the compile output warns that the key isn't the
one Humid last reported; with two parties that's expected.

**Beneficiary** again: paste the terms under **Load the allowance** and press **Load**. The
page re-derives the covenant address from the three params and refuses to continue if it
disagrees with the address in the terms, so altered or garbled terms are caught before
anything is paid, rather than producing a different covenant. Then check **can
withdraw** in the status panel. It reads "yes" only when this profile's key, or this Humid's
key after **Get my key from Humid**, is the `BENEFICIARY_KEY` the terms name; otherwise you
are only watching. To claim, set a payout address (any unblinded address, or **Use my wallet
address** with a wallet open; Humid pays to its own change address instead), refresh, and
press **Claim** on whichever instances are ready.

Claiming requires only the beneficiary key, the terms, and somewhere to be paid.
In particular the beneficiary never needs a file the benefactor created: the covenant
address is derived from public values, and the current UTXO is found by looking at the chain.

## Blinded vs. explicit: which outputs must be which

Liquid blinds by default, and that default is the right one in the abstract: confidential
transactions hide asset and amount from everyone but the parties. But a covenant that
*reasons about its own value* cannot read a blinded amount, so some smart-contract
functionality is incompatible with the confidentiality the network encourages.

How much this matters depends heavily on the library, and this demo uses the more difficult
one. LWK is primarily a confidential descriptor-wallet library, and its defaults favor
confidentiality throughout: `wpkhSlip77Descriptor()` means
`wollet.address()` is confidential by construction, `addLbtcRecipient` blinds while
`addExplicitRecipient` is the separate, less obvious call, and `Address.parse` refuses any
non-blinded address. This contract wants the opposite shape (explicit outputs, and
an input no wallet owns), so the page works against the library's defaults throughout, and changes
to those calls need the most care.

A covenant-first library has much less of this. `txmanifest-wallet` treats an explicit
covenant output as the default rather than an opt-out, and states output shapes
declaratively; `smplx-wasm` makes `addCovenantInput` a first-class concept with no
descriptor-wallet machinery insisting on blinding; Humid removes the choice entirely by
forcing `unblindable: "covenant"` for any covenant-destined output, *before* it reads the
manifest's declared value, so an author who forgets the field still cannot strand funds.
So do not read this page as "what Liquid covenant development feels like"; read it as what
it feels like through a general-purpose confidential wallet library.

The two mistakes have very different consequences, whichever library you use:
forgetting on a *covenant* output strands the funds forever and silently, while forgetting
elsewhere merely raises a confusing error.

The good news is that the choice is per output, not per transaction. For `allowance.simf`:

| Output | Must be | Why |
|---|---|---|
| The funding output, and the continuation at index 0 | **Explicit** | `same_asset()` and `full_withdrawal()` `unwrap_right` the asset and amount. A blinded value panics, and since the only spending path runs that code, the UTXO can never be spent. |
| The fee output at index 2 | **Explicit** | Elements fee outputs are explicit by definition. |
| The beneficiary payout at index 1 | **Either** | `recursive_covenant()` pins the output count, output 0's script hash and output 2's fee-ness. It never looks at output 1, so that output is free to be confidential. |

So the privacy cost of this contract is narrower than "covenants can't be confidential": it is
confined to the covenant's own value, and the payout (the output that says who got paid)
could be blinded. **This demo makes everything explicit anyway**, because building a single
explicit PSET avoids a blinding pass entirely and keeps the page readable. That is a
simplification, not a requirement.

If you want to blind the payout, the route through this API is `PsetOutputBuilder`'s
`blindingPubkey(...)` on output 1 plus `PsetBuilder.blindLast(input_indices, secrets)`,
passing `TxOutSecrets.fromExplicit(asset, value)` for the explicit covenant input being spent.

One honest caveat before bothering: for *this* contract it buys less than it appears to. The
covenant's balance is explicit at every step by construction, so an observer can difference
two consecutive covenant balances and recover each withdrawal amount regardless of how
output 1 is blinded. What blinding the payout actually hides is the destination and its
linkage to the beneficiary's other coins, which is worth having, but not amount privacy. A contract
whose covenant value *didn't* have to be public would get far more from it.

## Constraints to keep when changing the page

- **Builder methods move the builder.** `PsetBuilder.addInput`/`addOutput`, and the
  `PsetInputBuilder`/`PsetOutputBuilder` setters, are `mut self` in Rust, so each call frees
  the JS handle it was called on and returns a new one. Chaining calls is safe; saving a handle and
  calling two methods on it is not, and the symptom is `null pointer passed to rust` raised
  from somewhere unrelated. Unlike the `TxOut` hazard below, nothing in the typings warns
  about this: it is only visible in the Rust signatures.
- **The `TxOut` arrays are consumed.** `getSighashAll`, `run` and `finalizeTransaction`
  each destroy the array passed to them; the upstream typings document this. The page
  rebuilds a fresh `TxOut` for every call via `freshWitnessUtxo()`; reusing one is a
  use-after-free that surfaces as an unrelated error elsewhere.
- **Esplora has no per-output endpoint.** The witness UTXO comes from fetching
  `/tx/{txid}/hex` and taking `.output(vout)`. This is the one job `smplx-wasm` cannot do
  (it requires a pre-encoded `tx_out_hex` and exposes no transaction parser), and the
  reason this demo uses `lwk_wasm` rather than `smplx-wasm`.
- **`new Address(s)` and `Address.parse(s, network)` are not interchangeable.** `parse()`
  goes through `lwk_common::Address::parse`, which *requires* a blinded address and fails
  with "Expected a blinded address but got a non-blinded one"; there is an upstream test
  asserting that. Since this page builds explicit-only transactions, its payout addresses
  are deliberately unconfidential, so it uses the plain constructor and checks the network
  itself with `isMainnet()`. Using `parse()` here was a real bug: the page's own "use my
  wallet address" button fills an unconfidential address that `parse()` then rejected.
- **`blockstream.info` is not an arbitrary choice.** It is the only Liquid Esplora mirror
  that sends CORS headers, so a browser page cannot use `liquid.network` even though that
  mirror has far gentler rate limits and is the right choice server-side.
- **Wallet scans are the page's largest request load.** LWK's `fullScan` costs one Esplora
  request per wallet address, 40 or more per sync, and those requests don't pass through the
  page's own `esplora()` helper. `scanWallet()` applies the same throttle rules to them: it
  doesn't start a scan during a stand-down, runs one scan at a time (the faucet's balance
  check waits for a sync already running), and starts a stand-down when a scan is refused
  with HTTP 429.
- **One claim at a time, page-wide.** Every Claim button disables while any claim is in
  flight, and status refreshes guard against overlapping. A per-row lock alone is not
  enough: a single click could submit twice, and the second attempt came back as
  `insufficient fee, rejecting replacement`. That error means
  the *first* attempt already reached the mempool, so the withdrawal probably succeeded;
  the page says so rather than reporting a bare failure.
- **Fees come out of the allowance.** `enforce_single_input()` permits exactly one input,
  so nothing outside the covenant can contribute to the fee. `WITHDRAW_FEE_SATS` is a
  flat constant taken from the payout; real fee estimation is left out on purpose.
