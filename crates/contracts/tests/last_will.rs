//! Local (no-network) checks that the last-will covenant's corrected relative-timelock
//! check behaves as intended, without needing a live Liquid testnet round-trip.
//!
//! These exercise the same local Simplicity execution that `simplicityhl_core::finalize_transaction`
//! runs during signing -- they do not, and cannot, confirm the network's own independent
//! BIP-68 enforcement (that a spend declaring a satisfied distance is still rejected by
//! the node until that many blocks have genuinely passed). That property is a standard
//! Bitcoin/Elements consensus rule, not something this contract implements itself, and is
//! covered upstream by https://github.com/BlockstreamResearch/simplicityhl-std/pull/45.

use simplicityhl::elements::secp256k1_zkp::{self as secp256k1, Keypair};
use simplicityhl::simplicity::elements::{Address, OutPoint, Sequence, Transaction, TxOut, confidential};
use simplicityhl::tracker::TrackerLogLevel;
use simplicityhl_core::{
    LastWillAction, LastWillKeys, NUMS_INTERNAL_KEY, SimplicityNetwork, create_last_will_signature,
    finalize_transaction, get_last_will_address, get_last_will_program, last_will_witness_values,
};

const NETWORK: SimplicityNetwork = SimplicityNetwork::LiquidTestnet;
const MIN_DISTANCE_BLOCKS: u16 = 3;
const FUNDING_VALUE: u64 = 100_000;
const FEE: u64 = 100;

struct Fixture {
    keys: LastWillKeys,
    hot_keypair: Keypair,
    inheritor_keypair: Keypair,
    contract_address: Address,
}

fn setup() -> Fixture {
    let hot_keypair = Keypair::from_seckey_slice(secp256k1::SECP256K1, &[3; 32]).unwrap();
    let cold_keypair = Keypair::from_seckey_slice(secp256k1::SECP256K1, &[2; 32]).unwrap();
    let inheritor_keypair = Keypair::from_seckey_slice(secp256k1::SECP256K1, &[1; 32]).unwrap();

    let keys = LastWillKeys {
        hot_public_key: hot_keypair.x_only_public_key().0,
        cold_public_key: cold_keypair.x_only_public_key().0,
        inheritor_public_key: inheritor_keypair.x_only_public_key().0,
    };

    let contract_address =
        get_last_will_address(keys, MIN_DISTANCE_BLOCKS, NETWORK).expect("program compiles");

    Fixture { keys, hot_keypair, inheritor_keypair, contract_address }
}

/// A synthetic funding UTXO sitting at the contract's own address: not a real on-chain
/// UTXO, just enough to build and locally execute a spending transaction against.
fn funding_utxo(contract_address: &Address) -> (OutPoint, TxOut) {
    let txid = "1111111111111111111111111111111111111111111111111111111111111111".parse().expect("valid txid hex");
    let outpoint = OutPoint::new(txid, 0);
    let tx_out = TxOut {
        asset: confidential::Asset::Explicit(NETWORK.policy_asset()),
        value: confidential::Value::Explicit(FUNDING_VALUE),
        nonce: confidential::Nonce::Null,
        script_pubkey: contract_address.script_pubkey(),
        witness: Default::default(),
    };
    (outpoint, tx_out)
}

fn finalize(
    fx: &Fixture,
    action: LastWillAction,
    keypair: &Keypair,
    tx: Transaction,
    utxos: &[TxOut],
) -> anyhow::Result<Transaction> {
    let program = get_last_will_program(fx.keys, MIN_DISTANCE_BLOCKS)?;
    let signature = create_last_will_signature(&tx, utxos, &program, keypair, 0, NETWORK, &0)?;
    let witness_values = last_will_witness_values(&program, action, &signature)?;
    Ok(finalize_transaction(
        tx,
        &program,
        &NUMS_INTERNAL_KEY,
        utxos,
        0,
        witness_values,
        NETWORK,
        TrackerLogLevel::None,
        &0,
    )?)
}

#[test]
fn hot_spend_recreates_the_covenant() {
    let fx = setup();
    let utxo = funding_utxo(&fx.contract_address);

    let pst = contracts::sdk::refresh_last_will(utxo.clone(), &fx.contract_address, FEE, &0)
        .expect("refresh PSET builds");
    let tx = pst.extract_tx().expect("tx extracts");
    let utxos = [utxo.1];

    let result = finalize(&fx, LastWillAction::HotSpend, &fx.hot_keypair, tx, &utxos);
    assert!(result.is_ok(), "hot spend should locally validate: {result:?}");
}

#[test]
fn inherit_with_declared_distance_meeting_the_minimum_locally_validates() {
    let fx = setup();
    let utxo = funding_utxo(&fx.contract_address);
    let to_address = fx.contract_address.clone(); // destination is irrelevant to this check

    let pst =
        contracts::sdk::spend_as_inheritor(utxo.clone(), &to_address, FEE, MIN_DISTANCE_BLOCKS, &0)
            .expect("inherit PSET builds");
    assert_eq!(pst.inputs()[0].sequence, Some(Sequence::from_height(MIN_DISTANCE_BLOCKS)));
    let tx = pst.extract_tx().expect("tx extracts");
    let utxos = [utxo.1];

    let result = finalize(&fx, LastWillAction::Inherit, &fx.inheritor_keypair, tx, &utxos);
    assert!(
        result.is_ok(),
        "a declared distance equal to the minimum should locally validate \
         (the node's own separate BIP-68 check is what actually enforces real elapsed time): {result:?}"
    );
}

#[test]
fn inherit_with_declared_distance_below_the_minimum_is_rejected_locally() {
    let fx = setup();
    let utxo = funding_utxo(&fx.contract_address);
    let to_address = fx.contract_address.clone();

    let pst = contracts::sdk::spend_as_inheritor(
        utxo.clone(),
        &to_address,
        FEE,
        MIN_DISTANCE_BLOCKS - 1,
        &0,
    )
    .expect("inherit PSET builds");
    let tx = pst.extract_tx().expect("tx extracts");
    let utxos = [utxo.1];

    let result = finalize(&fx, LastWillAction::Inherit, &fx.inheritor_keypair, tx, &utxos);
    assert!(
        result.is_err(),
        "a declared distance below the minimum must fail the covenant's own check"
    );
}

#[test]
fn inherit_with_disabled_sequence_is_rejected_locally() {
    let fx = setup();
    let utxo = funding_utxo(&fx.contract_address);
    let to_address = fx.contract_address.clone();

    let mut pst = contracts::sdk::spend_as_inheritor(utxo.clone(), &to_address, FEE, MIN_DISTANCE_BLOCKS, &0)
        .expect("inherit PSET builds");
    pst.inputs_mut()[0].sequence = Some(Sequence::MAX); // relative-locktime disable flag set
    let tx = pst.extract_tx().expect("tx extracts");
    let utxos = [utxo.1];

    let result = finalize(&fx, LastWillAction::Inherit, &fx.inheritor_keypair, tx, &utxos);
    assert!(result.is_err(), "a disabled sequence must fail the covenant's own check");
}
