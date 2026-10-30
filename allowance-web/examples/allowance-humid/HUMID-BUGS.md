<!-- Snapshot. The working copy of this file lives in the Humid checkout it was
written against, where it is maintained as the upstream-facing summary; this copy
travels with the patch it documents. Check the Humid repository's issues for current
status before relying on anything here. -->

# Bugs found while building an allowance-covenant demo against Humid's txmanifest support

Found while building a SimplicityHL "allowance" covenant demo (benefactor funds it,
beneficiary withdraws up to a limit no more than once per N blocks) on Liquid testnet,
driven entirely through Humid's real `txmanifest.json` integration (CAIP-25/27 session
RPC → `processConfidentialTransaction`), not hand-built transactions. The demo's own
manifest and contract are alongside this file. This file is the upstream-facing summary:
one entry per distinct Humid defect, file/line, current status.

Tally as of 2026-10-07: **6 distinct bugs found, 2 fixed (locally only, not upstream),
4 still open.** Both local fixes ship as one patch, `humid-allowance-fixes.patch` in
this directory, against base commit `7dc8535`. Three additional items are missing feature/UX gaps rather
than bugs, listed separately at the end.

---

## 1. OPEN — RPC error responses render as `[object Object]`

**Where:** `apps/extension/src/core/extension-rpc/index.ts:40`

The popup-side RPC client does:

```js
pendingRequest.reject(new Error(String(response.error)));
```

`response.error` is already a well-formed `{message, code?, data?}` object (correctly
serialized by the background's `serializeError()` in
`core/extension-background/transport/index.ts`), but `String(...)` on an object gives
the literal string `"[object Object]"` instead of the real message. Confirmed as an
inconsistency, not a fundamental gap: `content-script/provider.js`'s own `toError()`
helper (the dApp-facing path) already unwraps `.message` correctly and could be
mirrored here.

**Trigger:** any popup-side RPC call that returns a structured error — reproduced via
a wrong vault-unlock password, which should show "Password did not unlock this vault.
N attempts remaining." and instead shows `[object Object]`. Also affects the
lockout message after `MAX_UNLOCK_ATTEMPTS = 5` within `UNLOCK_COOLDOWN_MS = 30_000`.

**Suggested fix:** replace `new Error(String(response.error))` with something that
reads `response.error.message` (falling back to `String(...)` only if `.message` is
absent), matching `toError()`.

**Status:** open, not patched locally or upstream.

---

## 2. OPEN — Setup/unlock action-popup closes silently before clicks register

**Where:** `routes/Auth/pages/Create/pages/Password/index.tsx` (trigger 1);
general platform behavior of `chrome.action.default_popup` (trigger 2).

Two independent, confirmed triggers for the same symptom — the popup closes before a
click's effect (e.g. "confirm password," "I saved it") actually applies, so the user
believes a step succeeded when it didn't:

- **Trigger 1:** both password fields use `type="password"` with
  `autoComplete="new-password"`, which invites Chrome's native "Suggested strong
  password" overlay — a separate browser-level UI surface whose appearance is a
  well-documented cause of extension action-popups closing (loss of focus).
- **Trigger 2:** switching away from the browser window entirely (e.g. to paste a
  seed phrase elsewhere) and back also closes the popup. This is Chrome's actual
  intentional behavior for `chrome.action.default_popup` — it closes whenever the
  *browser itself* loses OS-level focus — not something extension JS can override.

Checked for and did not find any "open setup in a full tab" alternative anywhere in
the extension source; some wallets provide exactly this to avoid this whole class of
fragile-small-popup problem, and Humid currently doesn't.

Practical consequence observed directly: a user can go through wallet creation,
have the vault silently fail to finish being created because of this, and then get
bug #1's `[object Object]` on the resulting "wrong password" — compounding two bugs
into what looks like total failure on first run.

**Workaround that needs no fix:** the seed phrase is already shown directly on screen
(`CreateSeedTab.tsx`'s `MnemonicDisplayGrid`); neither "I saved it" nor the following
verify-by-retyping step is gated on using the Copy button. Reading the words directly
off the popup and writing them down by hand avoids the window-switch that triggers
trigger 2 entirely.

**Suggested fix:** either an "open setup in a full tab" mode, or dropping
`autoComplete="new-password"` on the two password fields (addresses trigger 1 only;
trigger 2 is a platform-level constraint that only a non-popup surface removes).

**Status:** open, not patched. Given a broken first-run flow blocks all downstream
testing, this is high severity despite being "just" a UX bug.

---

## 3. OPEN — Malformed `instance` is silently accepted, producing a far-removed, unhelpful error

**Where:** `packages/tx-manifest/src/document/normalise.ts` (`normaliseInstance`)

`normaliseInstance()` reads a request's `instance` value as `raw` and expects
`raw.instance.fields` — **double-nested**. A caller sending the single-nested shape
(`instance: {fields: {...}}}`, the natural reading of the manifest-review API and
exactly how `tx-manifest-wallet`'s own `instance.json` *file* is shaped) resolves
silently to `{fields: {}}` — no validation error at the Zod layer, since `instance`
is typed there as a generic `Record<string, unknown>`.

The actual failure surfaces several layers downstream, in `covenants/compileParams.ts`,
as:

```
Compile parameter BENEFICIARY_KEY: "BENEFICIARY_KEY" is neither a parameter nor an
argument of this action.
{ "reject": "document-fault", "reason": "invalid_manifest_request" }
```

— a message that never mentions "instance" at all. A dApp developer hitting this
without source access to grep through has very little to go on.

**Suggested fix:** validate `instance` against a real schema (reject unknown/missing
shapes with a message naming "instance" directly) instead of treating it as an
opaque `Record<string, unknown>`, regardless of whether the double-nesting itself is
kept as the intended shape.

**Status:** open. (The double-nesting itself may be intentional design, not a bug —
the bug is the silent acceptance and the resulting unhelpful error.)

---

## 4. OPEN — `fee` keyword used in Humid's own shipped fixture, but unimplemented

**Where:** `packages/tx-manifest/src/evaluation/{evaluate,plan,references}.ts`

A bare `fee` reference in an `amount_sat` expression (e.g.
`"instance.ALLOWANCE_AMOUNT - fee"`) fails with:

```
Output beneficiary_payout cannot be paid: "fee" is neither a parameter nor an
argument of this action. The expression was "instance.ALLOWANCE_AMOUNT - fee".
```

This is not a wrong guess on the calling manifest's part — the identical expression
pattern (`will_in.amount_sat - fee`) appears in Humid's own
`packages/tx-manifest/src/__fixtures__/current/last_will.manifest.json`, which
`corpus.test.ts` itself expects to "read in full." Grepped the whole
`packages/tx-manifest` source outside tests/fixtures for the literal keyword `fee`:
zero hits in the expression evaluator. It is referenced in a published fixture but
never implemented.

**Workaround (manifest-side, not general):** `destination: "change"` with no
`amount_sat` resolves "whatever's left after other named outputs and the fee" later,
in the wallet's own PSET-building step, never touching the expression evaluator —
safe for a contract whose covenant doesn't pin that specific output's identity or
amount, but not a substitute for a real `fee` keyword for a contract that does.

**Suggested fix:** implement `fee` as a reference in the expression evaluator
(presumably resolved from the same `estimatedFeeSats`/`feeRateSatsPerKvb` values
already computed elsewhere in `review/index.ts`), or remove it from the shipped
`last_will.manifest.json` fixture if it's not meant to work yet.

**Status:** open upstream; demo works around it manifest-side for this specific
contract.

---

## 5. FIXED LOCALLY (not upstream) — Relative timelocks categorically refused

**Where:** `packages/tx-manifest/src/evaluation/inputRules.ts`, `transactionSequence()`

Any transaction with an enabled BIP68 relative-timelock `sequence.relative_blocks` on
any input was refused outright:

```
Input allowance_in asks for a relative timelock, and this wallet sets one sequence
for the whole transaction — which would time-lock the outputs funding it as well,
against their own age rather than this input's.
{ "reject": "unimplemented-construct", "reason": "invalid_manifest_request" }
```

The stated rationale is real in general (one shared `nSequence` would wrongly
time-lock a separate fee-paying input against its own confirmation age), but the
check fired on `declared.length >= 1` without ever inspecting the actual input count
of the transaction being built — so a genuinely single-input transaction (this
contract's `PartialWithdraw`/`FullWithdraw`, both enforcing exactly one input via
`allowance.simf`'s `enforce_single_input()`) was refused for no reason that applied
to it.

**Fix applied:** a parallel session added `relativeTimelockPlacement()` to
`inputRules.ts` and wired it into `review/index.ts`, checking actual input placement
before refusing rather than refusing unconditionally on any declared sequence. Full
writeup: `RELATIVE-TIMELOCKS.md` in this repo. Verified end-to-end against this demo
after a rebuild + extension reload: `PartialWithdraw` completed and broadcast for
real, confirmed on the Liquid testnet explorer
(`59346abd2ae1820aae87bd5262d4d7e67ba5efca62694a6b2832e9f33d11f3f8`, height 2640562,
input sequence `10` — exactly `MIN_DISTANCE`, confirming a real BIP68 timelock was
set, not merely not-refused).

**Status:** fixed in the local working tree only (`git status` shows
`packages/tx-manifest/src/evaluation/inputRules.ts` and `review/index.ts` modified,
uncommitted). Not upstream.

---

## 6. FIXED LOCALLY (not upstream) — Actions whose only output is policy-asset "change" are wrongly refused as "pays nothing"

**Where:** `packages/tx-manifest/src/review/index.ts` (output-construction loop, the
`surplus` calculation) and
`apps/extension/src/core/chains/liquid/adapters/smplx/assembleReviewedTransaction.ts`
(the `review.outputs.length === 0` refusal).

`FullWithdraw` — an action whose sole output is `destination: "change"` for the
policy asset (the same `fee`-keyword workaround from bug #4) — fails on a real
instance with real balance remaining (5000 sats; this project's own observed fees are
27-42 sats, ruling out dust):

```
"FullWithdraw" pays nothing, so there is nothing to build.
```

Root cause, confirmed by reading both files directly:

```ts
// review/index.ts, output-construction loop
const surplus =
    asset === policyAsset || changePaid.has(asset)
        ? 0n
        : (fundedFor.get(asset)?.changeSats ?? 0n);
if (surplus <= 0n) { continue; }   // output dropped from review.outputs
```

`surplus` is hardcoded to `0n` for **any** `"change"`-targeted output whose asset is
the policy asset, regardless of actual remaining balance, so the output is dropped
from `review.outputs` unconditionally. For `FullWithdraw`, that's its *only* output,
so `review.outputs` ends up completely empty.

```ts
// assembleReviewedTransaction.ts
if (review.outputs.length === 0) {
    return { ok: false, reason: `"${review.action}" pays nothing, so there is nothing to build.`, reject: "document-fault" };
}
for (const output of review.outputs) { builder.addOutput(...); }
builder.addChange(input.changeScriptPubKeyHex, ...);   // unconditional, real change computation
```

The refusal fires on the empty list **before** `builder.addChange(...)` is ever
reached — and `addChange()` is the LWK builder's own separate, correct mechanism for
computing and adding real leftover value, entirely independent of the buggy
`surplus` calculation above it. The real payout mechanism works fine; the
`review.outputs.length === 0` check just never gives it the chance to run, on the
mistaken premise that an empty `review.outputs` always means the action pays
nothing. It doesn't, for this specific shape.

This also explains why `PartialWithdraw` is unaffected: it has a second, non-change
explicit output (`allowance_again`), which alone keeps `review.outputs.length > 0`
and lets the check pass, with `addChange()` then adding the real beneficiary payout
exactly as it would have for `FullWithdraw` too (verified: bug #5's confirmed
on-chain `PartialWithdraw` transaction has a real, correct change-computed payout
output).

**Suggested fix:** either don't drop a policy-asset `"change"` output from
`review.outputs` when real balance would remain for it (compute real surplus instead
of hardcoding 0), or change `assembleReviewedTransaction.ts`'s refusal to also check
whether `addChange()` would produce something, not just `review.outputs.length`.

**Fix applied:** at the refusal, not at the `surplus` calculation. `reviewManifestAction()`
now records `changeDeferred` when it leaves a policy-asset `"change"` output out of
`review.outputs` for the builder, and `assembleReviewedTransaction()` refuses only when
`review.outputs` is empty *and* nothing was deferred. The field is optional, so every
existing `ManifestReview` literal — tests included — still type-checks unchanged.

Deliberately *not* fixed by computing a real surplus in the review: that would risk both
the review and `addChange()` adding an output for the same value, which is a worse bug
than the one being fixed. Fixing *when* the refusal fires leaves the correct payment
mechanism untouched.

**Verified end to end** against a patched build on 2026-10-07: `FullWithdraw` built,
signed and broadcast, closing out an allowance instance —
`27f08dcac9faf0bf01a5a30f6a8ba809d9c26d333cb699d94ad76390b5402c53` on Liquid testnet. A
`PartialWithdraw` on a separate instance in the same session
(`e1bee73eb719e66edd4e7afecd525826790fddeeef90cf1e73624685dc294126`) confirms the change
did not disturb the path that already worked.

**Status:** fixed in the local working tree only, shipped as the second half of
`examples/allowance-humid/humid-allowance-fixes.patch` in the demo repo. Not upstream.
Both bugs confirmed still present at `cb1b71c` ("release: 1.1.2"), whose five relevant
files are byte-identical to `7dc8535`.
Same category as #5 — an overly strict precondition that doesn't account for something
correct happening later in the same area of code — and as with #5 there was no
manifest-side workaround, since the defect is in the refusal rather than in anything
`txmanifest.json` can express.

---

## Not a bug: missing UI to view your own pubkey

Not counted in the tally above since it's a missing feature, not a defect. No screen
anywhere in Humid shows a raw public key, which a beneficiary needs to hand to a
benefactor out of band for any pubkey-gated covenant. Workaround: the already-public
`getWalletDescriptor` RPC returns the account xpub (`elwpkh([fingerprint/84h/1h/0h]
xpub.../0/*)`, confirming the account is plain BIP84 segwit v0, not Taproot), from
which the concrete child pubkey can be derived with a standard non-hardened BIP32
`CKDpub` (`/0/0` for the first receive address). Decoding Humid's own receive address
client-side is *not* a shortcut here — P2WPKH's witness program is `HASH160(pubkey)`,
not reversible, unlike P2TR.

**Suggested feature:** a "copy my pubkey" (or "copy my descriptor") action somewhere
in the UI, so a dApp integration doesn't require a developer to reimplement BIP32
derivation by hand.

---

## Not a bug: no way for a dApp to ask where to send funds

Not counted in the tally — a missing method, not a defect. `LIQUID_RPC_METHODS`
(`core/chains/liquid/application/liquidRpcMethods.ts`) registers ten methods:
`getBalance`, `getIdentityPublicKey`, `getIdentitySharedKey`, `getUTXOs`,
`getWalletDescriptor`, `processConfidentialTransaction`, `sendTransfer`, `signIdentity`,
`signMessage`, `signPset`. There is no `getAddress` or `getNewAddress`.

So a dApp can spend from the wallet, sign for it, and read its balance and UTXOs — but
cannot ask it for an address to be paid at. That matters for any flow where the wallet
has to be funded before it can act, which on testnet is every first run: the dApp can
tell the user to go to a faucet but cannot tell them where to point it.

Workaround, used in the demo: `getWalletDescriptor` returns
`elwpkh([fingerprint/84h/1h/0h]xpub…/0/*)`, so the first receive address can be derived
as P2WPKH of `HASH160(pubkey)` at `/0/0` — bech32 (witness v0), `tex` HRP on testnet.
That is the same derivation the pubkey workaround already needs (see the item below), so
a dApp that has done one gets this one almost free. But it produces the *unconfidential*
form, while Humid's own receive screen shows the confidential (`tlq1…`) form of the same
wallet, which is likely to confuse a user comparing the two.

**Suggested feature:** a `getAddress` method returning the account's next receive address
in both forms, with the confidential one marked as preferred. Together with the "copy my
pubkey" item below it would remove the need for any dApp to implement BIP32 derivation
at all.

---

## Not a bug: the manifest's descriptive vocabulary is almost entirely unused for UI

Not counted in the tally — nothing is broken. But automatically constructing a
human-readable description of an unfamiliar action is a core purpose of txmanifest, and
Humid currently surfaces close to none of what it is handed. Observed across this
project's demo rounds and confirmed in the source.

**What reaches the signing screen.** `confirmation/index.ts`'s `confirmationModel()`
builds a fixed structure: account, action, covenant rows (address, CMR, tapleaf hash,
utxo type, verified flag), fee asset and sats, blinded amounts, net effect, protocol,
published amounts, and one optional `summary`. That `summary` is the only prose, and
`actionSummary()` (line 137) is a single expression: `action.ui.action ??
action.description`. The confirmation's own headline is
`A dapp wants to perform "${review.action}" on the ${review.protocol} protocol`
(`processConfidentialTransaction/index.ts:96`) — an action name and a protocol name,
both dApp-claimed, which is the least informative subset of the document.

**What the format offers and the model never reads.** `document/registry.ts` marks
`description` as SHOWN on *manifest, action, input, output, param, utxo_type, witness
and validation*, plus `errors`, `lifecycle`, and the whole `ui` vocabulary (`label`,
`group`, `role`, `hide`). None of the per-input, per-output, per-param, per-witness or
per-validation descriptions appear in `ConfirmationModel`, and neither does any `ui`
field other than `ui.action`. A manifest can explain that output 0 returns the
remainder to the covenant so it can be drawn down again, and the user sees an output
id and a number instead. (`ui.hide` appears to be read by nothing at all, so the
format's presentation-suppression lever is declared but not honoured.)

**Why this is a smaller change than it looks.** The obvious objection is that these
fields are unsigned, attacker-authored text — manifests have no attestation
(`attestation_version` is `unread`: "a signature slot that does not exist, and read by
no implementation including the reference one"), so displaying more of one prominently
is a real phishing surface. But the machinery for showing untrusted prose honestly is
already built and already wired into this very screen. `confirmation/provenance.ts`
brands every value with `Origin = verified | chain | computed | dapp`, `combine`/
`weaker` degrade derived values to their weakest input, `describeOrigin()` renders
plain-English labels ("read from the network", "worked out by this wallet", "claimed by
the dapp"), and `ProcessCtConfirmation.tsx` prints that label beside each value while
`isProcessCtConfirmationData` *refuses to render* anything whose provenance it cannot
name — with tests for "refuses a value that lost its origin on the way here" and
"refuses an origin this wallet has no word for".

So richer manifest-driven UI does not require trusting manifests: every added field
would arrive beside the computed numbers, labelled "claimed by the dapp". The gap is
that `ConfirmationModel` has a hardcoded shape that drops the vocabulary, not that the
renderer lacks a safe way to show it.

**Suggested feature:** carry the per-construct `description` and `ui` fields through
`confirmationModel()` into `ShownConfirmation` as `fromDapp(...)` values, and render
them against the inputs and outputs they belong to. The provenance labelling that makes
this safe to do already exists and is currently only exercised on a handful of fields.
