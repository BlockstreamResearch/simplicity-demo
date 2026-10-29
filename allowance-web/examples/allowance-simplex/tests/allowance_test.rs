// Integration tests for `allowance.simf`, a draw-down covenant.
//
// Two tests, chosen to show two different things rather than to cover every
// branch of the contract: one full success lifecycle (funding through to the
// covenant ending), and one rejection that happens at the network layer rather
// than inside the contract itself. `examples/allowance-simplex/README.md`
// describes four more already-written tests (not included here, to keep this
// file small) and several genuinely unwritten ones, if you want more.

use simplex_allowance_example::artifacts::allowance::AllowanceProgram;
use simplex_allowance_example::artifacts::allowance::derived_allowance::{AllowanceArguments, AllowanceWitness};

use simplex::constants::DUMMY_SIGNATURE;
use simplex::provider::{ProviderError, SimplicityNetwork};
use simplex::signer::SignerError;
use simplex::simplicityhl::elements::{Script, Sequence, Txid};
use simplex::transaction::{
    ChangeOutput, FinalTransaction, PartialInput, PartialOutput, ProgramInput, RequiredSignature, UTXO,
};

// The beneficiary is an ordinary Simplex wallet built from a BIP-39 mnemonic,
// and the contract takes its public key as `param::BENEFICIARY_KEY`. This is a
// standard BIP-39 test vector: its key is public knowledge and must never hold
// real funds.
const BENEFICIARY_MNEMONIC: &str =
    "legal winner thank year wave sausage worth useful legal winner thank yellow";

// Passed to the contract as `param::ALLOWANCE_AMOUNT`: the most the
// beneficiary may take in any one withdrawal.
const ALLOWANCE_AMOUNT: u64 = 100_000;

// Passed to the contract as `param::MIN_DISTANCE`: how many blocks must pass
// between withdrawals.
//
// 1440 blocks is a day at Liquid's one-minute block time, which is a plausible
// figure for a real allowance rather than one picked to keep the tests quick.
// It costs them almost nothing: every spend of this covenant has to wait out
// the interval, but on regtest the wait is one `generate_blocks` call.
const MIN_DISTANCE: u16 = 1440;

// Five withdrawals' worth, which `allowance_drains_over_successive_withdrawals`
// spends out exactly: four partial withdrawals and then one full one.
const FUNDING_AMOUNT: u64 = 5 * ALLOWANCE_AMOUNT;

/// The beneficiary's wallet and the covenant their key is bound to.
struct Allowance {
    beneficiary: simplex::signer::Signer,
    program: AllowanceProgram,
}

impl Allowance {
    fn new(context: &simplex::TestContext) -> Self {
        let beneficiary = context.create_signer(BENEFICIARY_MNEMONIC);

        // Parameters are compiled into the program, so these three values
        // determine the covenant's script, and therefore its address.
        let arguments = AllowanceArguments {
            allowance_amount: ALLOWANCE_AMOUNT,
            min_distance: MIN_DISTANCE,
            beneficiary_key: beneficiary.get_schnorr_public_key().serialize(),
        };

        // `Program` caches its compilation, so holding one instance here
        // compiles the contract once per test rather than once per spend.
        Self {
            beneficiary,
            program: AllowanceProgram::new(&arguments),
        }
    }

    /// The covenant's script, and so its address.
    fn script(&self, context: &simplex::TestContext) -> Script {
        self.program.get_script_pubkey(context.get_network())
    }

    /// Where the beneficiary's own withdrawals go.
    fn beneficiary_script(&self) -> Script {
        self.beneficiary.get_address().script_pubkey()
    }
}

/// The parts every spend of this covenant shares.
///
/// `enforce_single_input` allows exactly one input, so this really is the
/// whole input side of the transaction, and the fee has to come out of the
/// allowance's own balance: no wallet input can help pay it.
///
/// That is worth knowing before using `Signer::broadcast` on a covenant like
/// this one, because `broadcast` will add inputs from the spending wallet if
/// the transaction cannot cover its own fee, and the contract would then
/// reject the result. It never happens here: a withdrawal leaves a whole
/// `ALLOWANCE_AMOUNT` unassigned, which is far more than the fee, so coin
/// selection returns before touching the wallet. A spend with a tighter
/// margin should use `Signer::finalize_strict`, which selects no coins.
///
/// `set_sequence` fills in every input that does not declare its own, which
/// is the honest way to say that the transaction as a whole claims the
/// `MIN_DISTANCE` wait. `enforce_relative_distance` reads that declaration
/// back out with `jet::parse_sequence`, so a spend that leaves the sequence
/// at its default would fail there.
fn spend_skeleton(program: &AllowanceProgram, utxo: UTXO) -> FinalTransaction {
    let mut ft = FinalTransaction::new();

    ft.set_sequence(Sequence::from_height(MIN_DISTANCE));
    ft.add_program_input(
        PartialInput::new(utxo),
        // `DUMMY_SIGNATURE` is a placeholder. The signer overwrites it with
        // a real signature over this transaction's `sig_all_hash`.
        ProgramInput::new(
            Box::new(program.as_ref().clone()),
            AllowanceWitness {
                signature: DUMMY_SIGNATURE,
            },
        ),
        // `witness::SIGNATURE` is a bare `Signature` rather than one nested
        // inside an `Either`, so the signer needs no path into the witness.
        RequiredSignature::Witness("SIGNATURE".to_string()),
    );

    ft
}

/// Builds a partial withdrawal, retaining `retained` satoshis in a fresh copy
/// of the covenant and sending the rest to `to`.
///
/// `recursive_covenant()` wants exactly three outputs: the covenant at 0, the
/// beneficiary at 1, the fee at 2. Only output 0 is listed here. The other
/// two are left to Simplex, which appends the change output and then the
/// Elements fee output and works out both amounts, landing them at 1 and 2.
fn partial_withdrawal(
    allowance: &Allowance,
    network: &SimplicityNetwork,
    utxo: UTXO,
    retained: u64,
    to: Script,
) -> FinalTransaction {
    let script = allowance.program.get_script_pubkey(network);
    let mut ft = spend_skeleton(&allowance.program, utxo);

    ft.add_output(PartialOutput::new(script, retained, network.policy_asset()));
    ft.add_change(ChangeOutput::new(to));

    ft
}

/// Builds a full withdrawal, taking everything the covenant holds to `to`.
///
/// `full_withdrawal()` returning true short-circuits `main`, so
/// `recursive_covenant()` never runs and the outputs are unconstrained.
/// There is nothing to list: the change output and the fee output Simplex
/// appends are the whole transaction.
fn full_withdrawal(allowance: &Allowance, utxo: UTXO, to: Script) -> FinalTransaction {
    let mut ft = spend_skeleton(&allowance.program, utxo);

    ft.add_change(ChangeOutput::new(to));

    ft
}

/// Funds a fresh allowance with `amount` and returns its UTXO, once the
/// funding transaction has confirmed.
///
/// Confirmation matters here beyond tidiness: the relative timelock is
/// measured from the height the funding output was mined at, so it does not
/// start running at all while the output sits in the mempool.
fn fund(context: &simplex::TestContext, script: &Script, amount: u64) -> anyhow::Result<UTXO> {
    let receipt = context.get_default_signer().send(script.clone(), amount)?;

    receipt.wait()?;

    utxo_at(context, script, receipt.txid())
}

/// The UTXO that transaction `txid` created at `script`.
///
/// Selecting by txid rather than taking the first result keeps a test from
/// picking up some other UTXO the covenant's address happens to hold.
fn utxo_at(context: &simplex::TestContext, script: &Script, txid: Txid) -> anyhow::Result<UTXO> {
    context
        .get_default_provider()
        .fetch_scripthash_utxos(script)?
        .into_iter()
        .find(|utxo| utxo.outpoint.txid == txid)
        .ok_or_else(|| anyhow::anyhow!("transaction {txid} left no UTXO at the allowance's script"))
}

/// Mines `MIN_DISTANCE` blocks, so that the wait a withdrawal declares has
/// really elapsed.
///
/// The blocks are mined in one RPC call, so the interval costs these tests
/// very little even at its realistic length. That holds at 1440 and stops
/// holding well before 25920: `mine_until_height` gives esplora a flat five
/// seconds to index whatever it just mined. A longer interval than this one
/// needs mining in bounded batches.
///
/// The contract cannot see the chain: `enforce_relative_distance` only checks
/// the sequence number the transaction declares. The node is what compares
/// that declaration against the height the input was mined at, under BIP-68.
/// On a real network the beneficiary waits; on regtest the blocks are free.
fn wait_out_the_interval(context: &simplex::TestContext) -> anyhow::Result<()> {
    let tip = context.get_default_provider().fetch_tip_height()?;
    let target = u64::from(tip) + u64::from(MIN_DISTANCE);

    context.get_network_utils().mine_until_height(target)?;

    Ok(())
}

/// The full lifecycle: fund an allowance, draw it down over several partial
/// withdrawals, then take the remainder in one full withdrawal that ends the
/// covenant. Shows the ordinary success path and most of the everyday API
/// surface (signing, broadcasting, waiting for confirmations, tracking a
/// UTXO across transactions, mining past a timelock).
#[simplex::test]
fn allowance_drains_over_successive_withdrawals(context: simplex::TestContext) -> anyhow::Result<()> {
    let allowance = Allowance::new(&context);
    let script = allowance.script(&context);
    let to = allowance.beneficiary_script();

    let mut utxo = fund(&context, &script, FUNDING_AMOUNT)?;

    // Four partial withdrawals take the balance 500_000 -> 100_000. Each hop
    // spends the UTXO the previous one created, so the covenant persists
    // across transactions, and each hop has to wait out the interval again
    // from the block that created the UTXO it is spending.
    for withdrawal in 1..=4 {
        wait_out_the_interval(&context)?;

        let retained = utxo.amount() - ALLOWANCE_AMOUNT;
        let ft = partial_withdrawal(&allowance, context.get_network(), utxo, retained, to.clone());

        let receipt = allowance.beneficiary.broadcast(&ft)?;
        receipt.wait()?;

        utxo = utxo_at(&context, &script, receipt.txid())?;
        anyhow::ensure!(
            utxo.amount() == retained,
            "after withdrawal {withdrawal} the allowance holds {} sats, expected {retained}",
            utxo.amount()
        );
    }

    // The balance is down to `ALLOWANCE_AMOUNT`, so `full_withdrawal()` is
    // now true and the beneficiary may take all of it without re-creating
    // the covenant.
    anyhow::ensure!(
        utxo.amount() == ALLOWANCE_AMOUNT,
        "expected {ALLOWANCE_AMOUNT} sats left, found {}",
        utxo.amount()
    );
    wait_out_the_interval(&context)?;

    let ft = full_withdrawal(&allowance, utxo, to);
    allowance.beneficiary.broadcast(&ft)?.wait()?;

    // Nothing is left at the covenant's address. The allowance is spent out
    // and the covenant has ended.
    let remaining = context.get_default_provider().fetch_scripthash_utxos(&script)?;
    anyhow::ensure!(
        remaining.is_empty(),
        "expected the allowance to be spent out, found {} UTXOs left at its address",
        remaining.len()
    );

    Ok(())
}

/// A withdrawal attempted before `MIN_DISTANCE` blocks have passed. Shows a
/// different kind of failure than a broken contract rule: the transaction
/// declares the full wait in its sequence number, so the contract's own
/// local execution is satisfied and this spend is stopped by the *node*
/// instead, under BIP-68, not by `assert!` inside the covenant.
#[simplex::test]
fn withdrawing_before_the_interval_is_rejected(context: simplex::TestContext) -> anyhow::Result<()> {
    let allowance = Allowance::new(&context);
    let script = allowance.script(&context);

    // Note the missing `wait_out_the_interval`: only the funding
    // transaction's own block has been mined.
    let funded = fund(&context, &script, FUNDING_AMOUNT)?;

    let retained = funded.amount() - ALLOWANCE_AMOUNT;
    let ft = partial_withdrawal(
        &allowance,
        context.get_network(),
        funded,
        retained,
        allowance.beneficiary_script(),
    );

    // The transaction declares the full `MIN_DISTANCE` wait in its sequence
    // number, and that declaration is all `enforce_relative_distance` checks,
    // so the contract is satisfied and local execution succeeds. This spend
    // is stopped by the network, not by the covenant.
    let Err(err) = allowance.beneficiary.broadcast(&ft) else {
        anyhow::bail!("the node accepted a withdrawal made before the interval had passed")
    };

    // `BroadcastRejected` carries the node's own response body. Matching that
    // variant keeps the test specific to a network rejection: a failed
    // `assert!` inside the contract would arrive as `CovenantExecution` and
    // must not pass here.
    let SignerError::Provider(ProviderError::BroadcastRejected { message, .. }) = &err else {
        anyhow::bail!("expected the node to reject the broadcast, got: {err}")
    };
    anyhow::ensure!(
        message.contains("non-BIP68-final"),
        "expected a `non-BIP68-final` rejection specifically, got: {message}"
    );

    Ok(())
}
