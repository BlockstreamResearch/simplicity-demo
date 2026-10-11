use crate::error::TransactionBuildError;
use crate::sdk::basic::transfer_native::transfer_native;
use crate::sdk::validation::TxOutExt;

use simplicityhl::elements::pset::PartiallySignedTransaction;
use simplicityhl::elements::{Address, OutPoint, Sequence, TxOut};

/// Build the "prove you're alive" transaction for a last-will covenant: sends the
/// contract's current balance (minus the fee) right back to the same contract address,
/// in the two-output shape (`[contract, fee]`) that the covenant's `recursive_covenant`
/// requires. The owner signs this with their hot key to reset the inheritor's timelock.
///
/// # Errors
///
/// Returns an error if the UTXO asset or amount (fee included) validation fails.
pub fn refresh_last_will(
    utxo: (OutPoint, TxOut),
    contract_address: &Address,
    fee_amount: u64,
    verbose: &u8,
) -> Result<PartiallySignedTransaction, TransactionBuildError> {
    let amount_to_send = utxo.1.remaining_after_required(fee_amount)?;
    transfer_native(utxo, contract_address, amount_to_send, fee_amount, verbose)
}

/// Build a transaction spending a last-will covenant via its inheritor path: sends the
/// contract's current balance (minus the fee) to `to_address`, declaring a BIP-68
/// relative-locktime of `min_distance_blocks` blocks on the spent input.
///
/// This declared sequence value only satisfies the contract's *local* check
/// (`enforce_relative_distance`); the node still independently enforces BIP-68 against
/// how many blocks have actually passed since the funding UTXO confirmed. Broadcasting
/// this transaction before that real distance has elapsed is expected to be rejected by
/// the node with a `non-BIP68-final` error, even though local Simplicity execution and
/// signing succeed -- that rejection is what demonstrates the timelock is enforced by
/// consensus, not just by the covenant script.
///
/// # Errors
///
/// Returns an error if the UTXO asset or amount (fee included) validation fails.
pub fn spend_as_inheritor(
    utxo: (OutPoint, TxOut),
    to_address: &Address,
    fee_amount: u64,
    min_distance_blocks: u16,
    verbose: &u8,
) -> Result<PartiallySignedTransaction, TransactionBuildError> {
    let amount_to_send = utxo.1.remaining_after_required(fee_amount)?;
    let mut pst = transfer_native(utxo, to_address, amount_to_send, fee_amount, verbose)?;
    pst.inputs_mut()[0].sequence = Some(Sequence::from_height(min_distance_blocks));
    Ok(pst)
}
