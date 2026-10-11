//! Helpers for compiling and interacting with the "last will" covenant example.
//!
//! Adapted from `SimplicityHL/examples/last_will.simf`, with its relative-timelock
//! jet replaced by a manual, non-broken check
//! (see `source_simf/last_will.simf` for details and
//! <https://github.com/BlockstreamResearch/simplicityhl-std/pull/45>).
//! Requires the `-Z enums` unstable compiler feature, which this module enables
//! automatically when compiling the program.

use std::collections::HashMap;

use ansi_term::Style;

use simplicityhl::elements::secp256k1_zkp::schnorr::Signature;
use simplicityhl::num::U256;
use simplicityhl::simplicity::bitcoin::XOnlyPublicKey;
use simplicityhl::simplicity::elements::{Address, Transaction, TxOut};
use simplicityhl::str::{Identifier, WitnessName};
use simplicityhl::value::ValueConstructible;
use simplicityhl::{
    Arguments, CompiledProgram, UnstableFeature, UnstableFeatures, Value, WitnessValues, elements,
};

use crate::constants::{NUMS_INTERNAL_KEY, SimplicityNetwork};
use crate::error::ProgramError;
use crate::get_and_verify_env;
use crate::scripts::{create_p2tr_address, load_program_with_unstable};

fn embolden(s: &str) -> String {
    Style::new().bold().paint(s).to_string()
}

/// Embedded Simplicity source for the "last will" covenant example.
pub const SOURCE_FILENAME: &str = "source_simf/last_will.simf";
pub const LAST_WILL_SOURCE: &str = include_str!("source_simf/last_will.simf");

/// The three public keys that parameterize a last-will contract instance.
#[derive(Debug, Clone, Copy)]
pub struct LastWillKeys {
    /// May spend the coins once the covenant's relative timelock has elapsed
    /// without a `HotSpend`.
    pub inheritor_public_key: XOnlyPublicKey,
    /// May spend the coins at any time, breaking out of the covenant entirely.
    pub cold_public_key: XOnlyPublicKey,
    /// May "prove they're alive" at any time, resetting the timelock by re-creating the
    /// same covenant in the first output of a new transaction.
    pub hot_public_key: XOnlyPublicKey,
}

/// Which of the contract's three spending paths a transaction is exercising.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastWillAction {
    /// The inheritor claims the coins after the timelock has elapsed.
    Inherit,
    /// The owner breaks out of the covenant with the cold key.
    ColdSpend,
    /// The owner proves they're alive with the hot key, resetting the timelock.
    HotSpend,
}

impl LastWillAction {
    const fn variant_name(self) -> &'static str {
        match self {
            Self::Inherit => "Inherit",
            Self::ColdSpend => "ColdSpend",
            Self::HotSpend => "HotSpend",
        }
    }
}

/// Compile the embedded last-will program with the given keys and minimum relative
/// locktime distance (in blocks) that the inheritor must wait, after the funding UTXO
/// confirms, before they may claim the coins.
///
/// # Errors
/// Returns error if program compilation fails.
pub fn get_last_will_program(
    keys: LastWillKeys,
    min_distance_blocks: u16,
) -> Result<CompiledProgram, ProgramError> {
    println!("{}", embolden(&format!("# Compiling SimplicityHL program {SOURCE_FILENAME}.")));
    let arguments = Arguments::from(HashMap::from([
        (
            WitnessName::from_str_unchecked("INHERITOR_PUBLIC_KEY"),
            Value::u256(U256::from_byte_array(keys.inheritor_public_key.serialize())),
        ),
        (
            WitnessName::from_str_unchecked("COLD_PUBLIC_KEY"),
            Value::u256(U256::from_byte_array(keys.cold_public_key.serialize())),
        ),
        (
            WitnessName::from_str_unchecked("HOT_PUBLIC_KEY"),
            Value::u256(U256::from_byte_array(keys.hot_public_key.serialize())),
        ),
        (
            WitnessName::from_str_unchecked("MIN_DISTANCE_BLOCKS"),
            Value::u16(min_distance_blocks),
        ),
    ]));

    load_program_with_unstable(
        LAST_WILL_SOURCE,
        arguments,
        &UnstableFeatures::new([UnstableFeature::Enums]),
    )
}

/// Construct the P2TR address for a last-will contract instance.
///
/// Uses the fixed BIP-0341 nothing-up-my-sleeve point as the Taproot internal key, so
/// the only way to spend the contract is via one of its three script-path spends --
/// there is no hidden key-path spend.
///
/// # Errors
/// Returns error if program compilation fails.
pub fn get_last_will_address(
    keys: LastWillKeys,
    min_distance_blocks: u16,
    network: SimplicityNetwork,
) -> Result<Address, ProgramError> {
    let program = get_last_will_program(keys, min_distance_blocks)?;
    Ok(last_will_address(&program, network))
}

/// Construct the P2TR address for an already-compiled last-will program.
///
/// Prefer this over [`get_last_will_address`] when the caller already has the compiled
/// `program` in hand (e.g. because it also needs it for signing) -- it avoids
/// recompiling the program a second time just to re-derive the address.
#[must_use]
pub fn last_will_address(program: &CompiledProgram, network: SimplicityNetwork) -> Address {
    println!("{}", embolden("# Deriving Liquid Testnet address."));
    create_p2tr_address(program.commit().cmr(), &NUMS_INTERNAL_KEY, network.address_params())
}

/// Build the `ACTION` witness value for the given action and Schnorr signature.
///
/// # Errors
/// Returns error if the compiled program doesn't declare an `ACTION` witness of the
/// expected `Action` enum type, or that type has no variant matching `action` (should
/// not happen for a program compiled from [`LAST_WILL_SOURCE`]).
pub fn last_will_witness_values(
    program: &CompiledProgram,
    action: LastWillAction,
    signature: &Signature,
) -> Result<WitnessValues, ProgramError> {
    let name = WitnessName::from_str_unchecked("ACTION");
    let ty = program.witness_types().get(&name).ok_or_else(|| ProgramError::MissingWitnessType {
        name: "ACTION".to_string(),
    })?;

    let variant = Identifier::from_str_unchecked(action.variant_name());
    let payload = vec![Value::byte_array(signature.serialize())];
    let value = Value::enum_variant(ty, &variant, payload).ok_or_else(|| ProgramError::InvalidWitnessValue {
        name: "ACTION".to_string(),
    })?;

    Ok(WitnessValues::from(HashMap::from([(name, value)])))
}

/// Create a Schnorr signature for a last-will spend by signing the `sighash_all` of the
/// proposed transaction with the given keypair (the hot, cold, or inheritor key,
/// depending on which spending path is being exercised).
///
/// # Errors
/// Returns error if the environment cannot be built/verified for the given UTXO (e.g.
/// its script pubkey doesn't match the address derived from `program`).
pub fn create_last_will_signature(
    tx: &Transaction,
    utxos: &[TxOut],
    program: &CompiledProgram,
    keypair: &elements::schnorr::Keypair,
    input_index: usize,
    network: SimplicityNetwork,
    verbose: &u8,
) -> Result<Signature, ProgramError> {
    use simplicityhl::simplicity::hashes::Hash as _;

    let env = get_and_verify_env(tx, program, &NUMS_INTERNAL_KEY, utxos, network, input_index, verbose)?;

    let sighash_all =
        elements::secp256k1_zkp::Message::from_digest(env.c_tx_env().sighash_all().to_byte_array());
    Ok(keypair.sign_schnorr(sighash_all))
}
