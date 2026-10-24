# Allowance covenant, Humid only: no wasm, no build step

The same contract as `../allowance-lwk/`, driven entirely through the Humid browser
extension. This exists to answer one question: **how much does a developer actually have
to build if a manifest-reading wallet does the contract work?**

## Setup

```
cd examples/allowance-humid-only
python3 -m http.server 8099
```

You also need a browser that can load the Humid extension, currently Chromium or Google
Chrome; Firefox cannot. Open <http://localhost:8099/> in the browser profile Humid is
installed in, rather than opening the file directly: ES module imports and `fetch()` of
the contract both need a real origin.

That's it. No `wasm-pack`, no Rust, no `--features simplicity`, no `vendor/` directory,
no `npm install`, no bundler.

One requirement on the Humid side, which will otherwise waste your time:

- **The patch** (`../allowance-humid/humid-allowance-fixes.patch`), applied to your Humid
  checkout and rebuilt. It fixes two upstream bugs: without the first, every withdrawal is
  refused as `unimplemented-construct`; without the second, `FullWithdraw` fails as
  "pays nothing, so there is nothing to build".

Every action also shows Humid's own "Perform a contract action?" confirmation. That is
where you approve each transaction, not a sign of a missing permission.

The whole lifecycle is confirmed working through a real patched Humid, with real
transactions on Liquid testnet: `Fund`, `PartialWithdraw`, and `FullWithdraw` closing an
allowance out. `FullWithdraw` needs the second of the patch's two fixes; without the patch
at all, nothing past `Fund` works.

## The comparison this is for

| | `allowance-humid-only` | `allowance-lwk` |
|---|---|---|
| Lines of HTML + JS | **989** | 2159 |
| Directory size | **80 KB** | 144 KB + **15 MB** of `vendor/` |
| Build step | none | `wasm-pack build --release --target web -- --features simplicity` |
| Toolchain needed | none | Rust, `wasm-pack`, LLVM with a wasm32 backend |
| External dependencies | 3 CDN imports, used once | one 14 MB wasm module |
| Runtime requirement | Humid installed **and patched** | nothing |

So the wallet-driven path is a little under half the code and none of the toolchain. The
build step is the sharper difference of the two: more interface work would close the gap in
line count, and would still leave this page with nothing to compile. The trade is that it
is easier to *build* and harder to *run*, since it needs an extension that is installed,
patched, and granted the right method.

## What this page still has to do itself

Handing the contract work to a wallet does not hand over the application work. Of the 989
lines, roughly:

- **The indexer, ~95 lines**: list the UTXOs at the covenant address, read the chain tip,
  and classify each one by balance against `ALLOWANCE_AMOUNT` and confirmation depth
  against `MIN_DISTANCE`. Neither Humid nor LWK offers an address-based UTXO lookup, so
  this is yours on *every* architecture. It is the largest single piece here and it does
  not shrink by choosing a different wallet.
- **The BIP32 pubkey derivation, ~75 lines plus the three imports**: Humid shows your own
  public key nowhere, so it has to be recovered from `getWalletDescriptor`. This is the
  only cryptography in the file, it is needed exactly once per beneficiary, and it would
  disappear entirely if Humid shipped a "copy my pubkey" button.
- **The RPC plumbing, ~55 lines**: a session helper plus `wallet_invokeMethod`.
- **Terms handoff and state**: which values the beneficiary needs, and where to keep them.

What it does *not* do: compile the contract, derive the covenant address, build a PSET,
compute a sighash, produce a signature, estimate a fee, or decide blinding. All of that is
Humid's, from the manifest.

## Two details worth copying if you write your own

**Don't create a session per call.** `wallet_createSession` always shows a connect prompt
*and revokes any existing session first*, so calling it before each action, which
`../allowance-humid/dapp-demo.html` does, destroys a working 30-day authorization and
re-approves it every time. Every connect prompt in that older demo was self-inflicted.
This page tries `wallet_invokeMethod` first and only creates a session if the call is
actually refused.

**`instance` is double-nested, `state` is not.** `instance: {instance: {fields: {...}}}`,
because `normaliseInstance()` reads `raw.instance.fields`. Getting it wrong fails silently
and resurfaces much later as "X is neither a parameter nor an argument of this action".
`state: {utxos: [...]}` is single-level.

## What dropping the wasm actually costs

One thing, and it is worth understanding before using this shape for anything real: **this
page cannot verify that the terms it was given produce the address it is watching.**

The `allowance-lwk` sibling compiles the contract locally, re-derives the covenant address
from the three parameters, and refuses to continue if it disagrees with the address in the
handoff blob. That check needs a compiler. Here, the address is either read off output 0 of
the funding transaction or pasted from the terms, in both cases taken on the benefactor's
word.

Humid itself does compile, so it is not signing blind: it verifies that the source it was
handed produces the address it is spending from. But the *page* cannot independently check
anyone's claims. That is the same shape of gap as the manifest format's `attestation_version`
being, in the reference implementation's own words, "a signature slot that does not exist,
and read by no implementation including the reference one": the pieces that would let one
party verify another's assertions are the pieces that are optional or missing.
