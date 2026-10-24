# Allowance covenant driven by `lwk_wasm`: single-page demo

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

Liquid **testnet** only. The wallet mnemonic and the beneficiary's secret key live in
`localStorage` in plaintext: play money, nothing else, ever.

## Setup

You need a `lwk_wasm` build **with the Simplicity feature enabled**. It is behind a cargo
feature flag that is off by default and marked unstable upstream:

```toml
# lwk_wasm/Cargo.toml
# Experimental: API behind this feature is unstable and may change without notice.
simplicity = ["lwk_simplicity"]
```

1. Build it, from an `lwk` checkout, **into its own output directory**:
   ```
   cd <lwk>/lwk_wasm
   wasm-pack build --release --target web --out-dir pkg-web -- --features simplicity
   ```
   `--target web` matters: it emits an ES module with a default `init` export that a plain
   `<script type="module">` can load. The `bundler` target expects Vite or webpack.

   **`--out-dir pkg-web` matters just as much.** The default `pkg/` is what Humid consumes
   as a `file:` dependency, and it needs the *bundler* target. Building the web target into
   `pkg/` overwrites that and breaks every Humid wallet call with
   `WebAssembly.instantiate(): Import #0 "wbg": module is not an object or function`,
   because the bundler-target glue (`lwk_wasm_bg.js`) is left behind, stale, next to a
   `.wasm` it no longer matches. If you have already done this, restore Humid with
   `wasm-pack build --release --target bundler -- --features simplicity`, then
   `bun install && bun run build`.

   Builds take a while: `wasm-opt` alone runs for minutes on a module this size. To skip
   it while iterating, either use `--dev` (no `wasm-opt`, debug build, slower at runtime)
   or set `[package.metadata.wasm-pack.profile.release] wasm-opt = false`.

2. Copy the two files this page needs into `vendor/` (git-ignored, since the wasm is ~14 MB):
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

1. **Load the library**: initialises the wasm and asserts the Simplicity classes are
   actually present, with the rebuild command in the error if they aren't. This is the
   check worth running first: a feature-off build fails here rather than confusingly later.
2. **Wallet**: a single-key LWK wallet (`Signer` → `wpkhSlip77Descriptor` → `Wollet`),
   with a Waterfalls scan, balance, and a faucet button.
3. **Beneficiary key**: `Keypair` from 32 random bytes, exposing `.xOnlyPublicKey` as
   `BENEFICIARY_KEY` and signing the witness with `.signSchnorr()`. Note how much simpler
   this is than the browser-extension path, where no API exposes your own pubkey and the
   key has to be recovered by BIP32-deriving a BIP-84 descriptor by hand.
4. **Compile**: the three compile-time params become `SimplicityArguments`
   (`fromU256Hex` for the pubkey, `fromU64` for the amount, `fromU16` for the distance),
   then `SimplicityProgram.load(source, args)` and `createP2trAddress(NUMS_KEY, network)`.
   The internal key is BIP341's unspendable NUMS point, so the Simplicity script path is
   the only way to spend.
5. **Fund**: `TxBuilder.addExplicitRecipient(...)`, deliberately *not*
   `addLbtcRecipient`. The contract reads its own asset and amount, so a confidential
   output to this address is unspendable forever. Dry run first, broadcast second.
6. **Live instances**, the indexer that neither library provides: `/address/{a}/utxo`
   plus `/blocks/tip/height`, classifying each UTXO independently by balance against
   `ALLOWANCE_AMOUNT` and depth against `MIN_DISTANCE`. Every UTXO at the address is its
   own allowance with its own timelock; they are never summed and can never be merged.
7. **Withdraw**: builds the PSET by hand, because the covenant input is not a wallet
   UTXO: `PsetInputBuilder.fromPrevout(...).witnessUtxo(...).sequence(TxSequence.fromHeight(MIN_DISTANCE))`,
   then explicit outputs in the order `recursive_covenant()` requires (continuation at 0,
   payout at 1, fee at 2; an Elements fee output is an explicit output with an empty
   script), then `getSighashAll` → `signSchnorr` → `run` locally → `finalizeTransaction`
   → `broadcastTx`.

## The two signing paths

The page can drive the same contract two ways, chosen in the "Who signs" section.

**This page** (default): `lwk_wasm` compiles the contract, derives the address, builds the
PSET, signs and broadcasts, all in the browser. Keys live in `localStorage`. No extension,
no manifest.

**Humid extension**: the page hands Humid `txmanifest.humid.json` plus the contract source
and it does the compiling, signing and broadcasting itself, showing its own confirmation.
This is the more realistic architecture: a real wallet that has never seen this covenant
before learns it from a manifest. Requirements and caveats:

- Humid must be installed in this browser profile and injecting into the page, so serve over
  `http://`, not `file://`.
- **It needs the relative-timelock patch** (`../allowance-humid/humid-allowance-fixes.patch`).
  Without it every withdrawal is refused with `unimplemented-construct`, because upstream
  rejects any input declaring `sequence.relative_blocks` without checking the input count.
- `FullWithdraw` is broken in *unpatched* Humid: it refuses an action whose only output is
  policy-asset change with *"pays nothing, so there is nothing to build."* The page warns
  about this in the confirmation dialog rather than letting it surprise you.
- Humid signs with its own BIP-84 key at `m/84'/1'/0'/0/0`, which it exposes nowhere in its
  UI. "Get my key from Humid" recovers it by calling `getWalletDescriptor` and doing two
  non-hardened BIP32 steps, printing the raw descriptor alongside so the derivation is
  checkable. The curve maths is a dynamic `import()` from a CDN, so the local-only path keeps
  working with no network.
- **The two paths cannot share an allowance.** Different signer means a different
  `BENEFICIARY_KEY`, which means a different compiled address. Pick the signer before
  compiling.
- The indexer is this page's either way: Humid has no address-based UTXO lookup.

On the manifest format, checked against the Humid checkout rather than assumed: this build
reads `manifest_version` but never validates it (`corpus.test.ts` deletes the field to prove
it isn't load-bearing), and `document/registry.ts` (a machine-readable list of which fields
it reads, shows, ignores or hasn't implemented) accepts both the `contract_templates` and
the older `classes` container. Every field in `txmanifest.humid.json` is one this build
reads, with no unknown or ignored keys. So the `0.1.0`-versus-`0.3.0` difference against
`tx-manifest-wallet` is cosmetic for Humid's purposes.

## Running it as two separate parties

The page carries both roles, marked on each step, and the realistic test is two browser
profiles: each profile has its own `localStorage`, so each holds exactly one party's keys.

**Beneficiary**, in their own profile: load the library (1), generate the beneficiary key
(3), send the benefactor the x-only pubkey it prints. Nothing else. No wallet, no mnemonic,
no funds, no chain scan.

**Benefactor**, in theirs: load (1), open and fund a wallet (2), paste the beneficiary's
pubkey into step 4 along with the amount and interval, compile, then **Copy terms** and send
the blob over. Fund the allowance (5).

**Beneficiary** again: paste the blob into step 4's terms field and press **Load pasted
terms**. The page re-derives the covenant address from the three params and refuses to
continue if it disagrees with the address in the blob, so altered or garbled terms are
caught before anything is paid, rather than quietly producing a different covenant. It also
says whether `BENEFICIARY_KEY` is this browser's key, i.e. whether you can actually claim or
are only watching. Then step 6: paste a payout address (or open a wallet and use its own),
refresh, and claim whichever instances are ready.

Claiming genuinely requires only the beneficiary key, the terms, and somewhere to be paid.
In particular the beneficiary never needs a file the benefactor created: the covenant
address is derived from public values, and the current UTXO is found by looking at the chain.
That property is what makes the role separation real rather than cosmetic.

## Blinded vs. explicit: which outputs must be which

Liquid blinds by default, and that default is the right one in the abstract: confidential
transactions hide asset and amount from everyone but the parties. But a covenant that
*reasons about its own value* cannot read a blinded amount, so some smart-contract
functionality is incompatible with the thing the network is trying to encourage.

How much that *hurts* depends heavily on the library, and this demo picked the harder one.
LWK is a confidential descriptor-wallet library at heart, and its defaults sit on the
confidential side of the tension at every turn: `wpkhSlip77Descriptor()` means
`wollet.address()` is confidential by construction, `addLbtcRecipient` blinds while
`addExplicitRecipient` is the separate, less obvious call, and `Address.parse` refuses a
non-blinded address outright. This contract wants the opposite shape (explicit outputs, and
an input no wallet owns), so the page is crossing the library's grain throughout, and both
of the real bugs found while getting it working were at that crossing.

A covenant-first library has much less of this. `txmanifest-wallet` treats an explicit
covenant output as the default rather than an opt-out, and states output shapes
declaratively; `smplx-wasm` makes `addCovenantInput` a first-class concept with no
descriptor-wallet machinery insisting on blinding; Humid removes the choice entirely by
forcing `unblindable: "covenant"` for any covenant-destined output, *before* it reads the
manifest's declared value, so an author who forgets the field still cannot strand funds.
So do not read this page as "what Liquid covenant development feels like"; read it as what
it feels like through a general-purpose confidential wallet library.

The asymmetry of the two mistakes is worth internalising whichever library you use:
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

## Details that will bite you if you change it

- **Builder methods move the builder.** `PsetBuilder.addInput`/`addOutput`, and the
  `PsetInputBuilder`/`PsetOutputBuilder` setters, are `mut self` in Rust, so each call frees
  the JS handle it was called on and returns a new one. Chaining is fine; saving a handle and
  calling two methods on it is not, and the symptom is `null pointer passed to rust` raised
  from somewhere unrelated. Unlike the `TxOut` hazard below, nothing in the typings warns
  about this: it is only visible in the Rust signatures.
- **The `TxOut` arrays are consumed.** `getSighashAll`, `run` and `finalizeTransaction`
  each destroy the array passed to them; the upstream typings say so outright. The page
  rebuilds a fresh `TxOut` for every call via `freshWitnessUtxo()`; reusing one is a
  use-after-free that will look like a nonsense error somewhere else.
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
- **Wallet scans use Waterfalls, not `blockstream.info`.** A plain Esplora scan costs one
  request per address, 40 or more per sync, sent back to back. The page scans through
  `https://waterfalls.liquidwebwallet.org/liquidtestnet/api` with
  `new EsploraClient(network, url, true, 1, false)`, where `true` enables Waterfalls: one
  request returns the whole descriptor's history, so a sync costs about 3 requests plus one
  per new transaction. That server sends CORS headers but is not a full Esplora (it has no
  `/blocks/tip/height`), so covenant lookups and broadcasts stay on `blockstream.info`.
  The Waterfalls operator learns the wallet's addresses.
- **One claim at a time, page-wide.** Every Claim button disables while any claim is in
  flight, and status refreshes guard against overlapping. A per-row lock alone is not
  enough: a single click could submit twice, and the second attempt came back as
  `insufficient fee, rejecting replacement`. That error means
  the *first* attempt already reached the mempool, so the withdrawal probably succeeded;
  the page says so rather than reporting a bare failure.
- **Fees come out of the allowance.** `enforce_single_input()` permits exactly one input,
  so nothing outside the covenant can contribute to the fee. `WITHDRAW_FEE_SATS` is a
  flat constant taken from the payout; real fee estimation is left out on purpose.

## Known unknowns

Specific things most likely to be wrong, in rough order of suspicion:

1. **The witness value encoding.** `SIGNATURE` is built with
   `SimplicityTypedValue.fromByteArrayHex(sig)`. If the compiler wants a declared
   `Signature` type instead, the alternative is
   `SimplicityTypedValue.parse(sig, SimplicityType.fromString("Signature"))`.
2. **`program_public_key`.** `getSighashAll`, `run` and `finalizeTransaction` all take
   one; the page passes the NUMS internal key, on the reasoning that it is needed to
   rebuild the control block. If it means something else, every withdrawal fails here.
3. **The fee output.** Built as an explicit output with `Script.empty()`. If `PsetBuilder`
   wants the fee expressed another way, `recursive_covenant()`'s `output_is_fee(2)` check
   will fail even though the amounts are right.
4. **`extractTx()` on a hand-built v2 PSET** with only explicit outputs, with no blinding
   pass; plausible, unconfirmed.
5. **Whether `TxBuilder.finish(wollet)` tolerates an explicit recipient** alongside its
   normal blinded change, in step 5.
