# Allowance: Simplex integration tests (drop-in files)

Two files meant to be copied into a fresh `simplex init` project, not a
self-contained project of their own; see Step 2 of [the HOWTO](../../HOWTO.md) for
the walkthrough (install Simplex, scaffold a project, copy these two files
in, build, test).

- `simf/allowance.simf`: this repo's own `contracts/allowance.simf`, without the
  `// (N)!` annotation markers that docs.simplicity-lang.org renders.
- `tests/allowance_test.rs`: two integration tests against a real local Elements
  regtest node, exercising the contract through Simplex's real signing/broadcast
  API rather than reimplementing any of it by hand. Chosen to show two
  *different* things rather than to cover every branch. "Tests to write
  yourself" below describes more, with what each one would prove.

## Where this came from

Written by calling directly into the real Simplex SDK rather than reimplementing
any of its signing/broadcast logic by hand, which is worth knowing, since the SDK's own API
surface is close to undocumented, which makes code that doesn't do this easy to get
subtly wrong.

**The test file itself is left exactly as written**: it's a real, passing
artifact, not something to re-derive. What follows here is orientation for reading
it, not a replacement for reading it.

## The two tests included here

1. `allowance_drains_over_successive_withdrawals` walks the full success lifecycle:
   fund an allowance, draw it down over four partial withdrawals, then take the
   remainder in one full withdrawal that ends the covenant, checking the balance
   at every step. Shows most of the everyday API surface: signing, broadcasting,
   waiting for confirmations, tracking a UTXO across several transactions, mining
   past a timelock.
2. `withdrawing_before_the_interval_is_rejected` is an otherwise-valid withdrawal
   attempted before `MIN_DISTANCE` blocks have passed. Passes the contract's own
   local execution (the declared sequence number is correctly formed) and is
   rejected by the *node* instead, under BIP68, a different kind of test than
   #1: asserting a failure, and specifically one that happens at the network
   layer rather than inside the contract, which is worth seeing distinctly from
   a contract-level rejection (below).

## Tests to write yourself

Each of these is a concrete next test, roughly in order of how much new ground it
covers. Everything they need is already in `tests/allowance_test.rs`, so writing them
is a good way to learn the Simplex API from it. Start with these four:

- `partial_withdrawal_returns_the_remainder_to_the_covenant`: the simplest
  possible valid spend: one partial withdrawal, checked directly rather than as
  part of a longer sequence like test #1 above.
- `a_stranger_cannot_withdraw`: identical to a valid withdrawal except signed by
  a random key instead of `BENEFICIARY_KEY`. Rejected *inside the contract*
  (`checksig`), the contract-level counterpart to test #2's network-level
  rejection above.
- `withdrawing_more_than_the_allowance_is_rejected`: retains one satoshi less
  than the contract's `difference <= retained_amount` check allows. Rejected
  inside the contract.
- `the_remainder_must_return_to_the_same_covenant`: pays the remainder into a
  *different*, equally valid allowance covenant (same beneficiary, different
  `MIN_DISTANCE`, hence a different script) instead of this one. Rejected inside
  the contract: `recursive_covenant()` compares script hashes, not trusting the
  witness, so the beneficiary can't move funds onto different terms under cover
  of a routine withdrawal. Also the one test here that builds its transaction by
  hand rather than through the `partial_withdrawal`/`full_withdrawal` helpers.

One short illustrative piece, the part every spend of this covenant shares (see the
real file for the rest; this is not the whole test, just the shared skeleton both
tests build on):

```rust
fn spend_skeleton(program: &AllowanceProgram, utxo: UTXO) -> FinalTransaction {
    let mut ft = FinalTransaction::new();
    ft.set_sequence(Sequence::from_height(MIN_DISTANCE));
    ft.add_program_input(
        PartialInput::new(utxo),
        ProgramInput::new(
            Box::new(program.as_ref().clone()),
            AllowanceWitness { signature: DUMMY_SIGNATURE },
        ),
        RequiredSignature::Witness("SIGNATURE".to_string()),
    );
    ft
}
```

`set_sequence` is the manifest-free, hands-on-the-wheel equivalent of the manifest's
`sequence.relative_blocks`: the same BIP68 mechanism, just set directly on the
transaction builder instead of resolved from a manifest field. `DUMMY_SIGNATURE` is
a placeholder the signer overwrites with a real one over this transaction's actual
`sig_all_hash`, so nothing about the contract's real signature check is being
skipped.

## Traps worth knowing before reading further

- **The fee comes out of the allowance's own balance, always.** `enforce_single_input`
  leaves no room for a separate fee-paying input. `Signer::broadcast` will add a
  wallet input to cover a shortfall if one exists, which `enforce_single_input`
  would then make the contract reject, but it never happens in this suite, since a
  withdrawal always leaves a whole `ALLOWANCE_AMOUNT` unassigned, far more than the
  fee. A spend with a tighter margin should use `Signer::finalize_strict` instead,
  which selects no coins at all.
- **`MIN_DISTANCE` is 1440 blocks (a real day at Liquid's ~1-minute block time) on
  purpose, not shrunk for test speed.** On regtest the whole interval mines in one
  `generate_blocks` call, so a realistic value costs almost nothing, except in one
  test (`allowance_drains_over_successive_withdrawals`), which waits out the
  interval five times and takes on the order of 20 seconds.
- **There's a real ceiling on how long an interval `mine_until_height` can jump in
  one call**: it gives Esplora a flat five seconds to index whatever it just mined,
  regardless of how many blocks that was. 1440 is comfortable; something like
  25920 (what some real covenants use) is not; mine it in bounded batches instead
  of one call to the final target if you need a longer interval than this.

## More tests to write

These go further than the four above:

- **Two allowances cannot be co-spent.** Pay the covenant's address twice (two
  independent allowances), then build one transaction spending both.
  `enforce_single_input` should reject it, the subtlest safety property in the
  contract, and the one most worth proving.
- **A full withdrawal needs no continuation output.** A standalone test, rather than
  only reaching this path at the end of the drain test.
- **The fee output must be at index 2.** Swap the beneficiary and fee outputs;
  `jet::output_is_fee(2)` should fail since the check is position-specific. This one
  needs listing all three outputs by hand and signing with `Signer::sign_tx`
  (Simplex's own output-appending helpers always put fee last).
- **A withdrawal of less than the full allowance**: retaining *more* than
  `balance - ALLOWANCE_AMOUNT` is allowed; the contract sets a ceiling, not a floor.
- **A duration-based sequence is rejected**: `enforce_relative_distance` expects a
  block-count encoding specifically (`unwrap_left::<Duration>`); a time-based
  relative lock should panic rather than being read as one.
- **A fourth output is rejected**: `jet::eq_32(jet::num_outputs(), 3)` is an
  equality, not a minimum.

## What this directory leaves out

There is no `Cargo.toml` or other project scaffolding here. Step 2 of the HOWTO gets
them from `simplex init`, which declares the published `smplx-std` crate.
