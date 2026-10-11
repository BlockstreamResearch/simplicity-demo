//! Script and Taproot utilities plus minor helpers around Elements types.

use sha2::{Digest, Sha256};

use simplicityhl::ast::ElementsJetHinter;
use simplicityhl::elements::{
    Address, AddressParams, AssetId, ContractHash, OutPoint, Script, script, taproot,
};

use simplicityhl::simplicity::bitcoin::{XOnlyPublicKey, secp256k1};
use simplicityhl::simplicity::hashes::{Hash, sha256};
use simplicityhl::{Arguments, CompiledProgram, UnstableFeatures};

use base64::display::Base64Display;
use base64::engine::general_purpose::STANDARD;

use crate::error::ProgramError;

use ansi_term::Style;

fn embolden(s: &str) -> String {
    Style::new().bold().paint(s).to_string()
}

pub fn indent_four_spaces(input: &str) -> String {
    input
        .lines()
        .map(|line| format!("    {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Load program source and compile it to a Simplicity program.
///
/// # Errors
/// Returns error if the program fails to compile.
pub fn load_program(source: &str, arguments: Arguments) -> Result<CompiledProgram, ProgramError> {
    load_program_with_unstable(source, arguments, &UnstableFeatures::none())
}

/// Load program source and compile it to a Simplicity program, enabling the given
/// unstable compiler features (e.g. `-Z enums`).
///
/// # Errors
/// Returns error if the program fails to compile.
pub fn load_program_with_unstable(
    source: &str,
    arguments: Arguments,
    unstable_features: &UnstableFeatures,
) -> Result<CompiledProgram, ProgramError> {
    let compiled = CompiledProgram::new_with_unstable(
        source,
        unstable_features,
        arguments.clone(),
        true,
        Box::new(ElementsJetHinter::new()),
    )
    .map_err(ProgramError::Compilation)?;
    println!("\n{}\n{}", embolden("SimplicityHL source code:"), indent_four_spaces(source));
    println!("\n{}\n{}", embolden("Parameter arguments (compile-time):"), indent_four_spaces(&arguments.to_string()));

    let program_bytes = compiled.commit().to_vec_without_witness();
    let program_base64 = Base64Display::new(&program_bytes, &STANDARD).to_string();
    let verbose: &u8 = &0; // XXX TODO inherit from actual command line argument,
                           //          but also only display this data once per run
    if *verbose > 0 { println!("---> Compiled program base64: {}", program_base64); } // VERBOSE

    let cmr = compiled.commit().cmr();
    if *verbose > 0 { println!("---> Compiled program CMR: {}", cmr); } // VERBOSE

    Ok(compiled)
}

/// Generate a non-confidential P2TR address for the given program CMR and key.
#[must_use]
pub fn create_p2tr_address(
    cmr: simplicityhl::simplicity::Cmr,
    x_only_public_key: &XOnlyPublicKey,
    params: &'static AddressParams,
) -> Address {
    let spend_info = taproot_spending_info(cmr, *x_only_public_key);

    Address::p2tr(
        secp256k1::SECP256K1,
        spend_info.internal_key(),
        spend_info.merkle_root(),
        None,
        params,
    )
}

fn script_version(cmr: simplicityhl::simplicity::Cmr) -> (Script, taproot::LeafVersion) {
    let script = script::Script::from(cmr.as_ref().to_vec());
    (script, simplicityhl::simplicity::leaf_version())
}

fn taproot_spending_info(
    cmr: simplicityhl::simplicity::Cmr,
    internal_key: XOnlyPublicKey,
) -> taproot::TaprootSpendInfo {
    let builder = taproot::TaprootBuilder::new();
    let (script, version) = script_version(cmr);
    let builder = builder
        .add_leaf_with_ver(0, script, version)
        .expect("tap tree should be valid");
    builder
        .finalize(secp256k1::SECP256K1, internal_key)
        .expect("tap tree should be valid")
}

/// Compute the Taproot control block for script-path spending.
///
/// # Panics
/// Panics if the taproot tree is invalid (should never happen with valid CMR).
#[must_use]
pub fn control_block(
    cmr: simplicityhl::simplicity::Cmr,
    internal_key: XOnlyPublicKey,
) -> taproot::ControlBlock {
    let info = taproot_spending_info(cmr, internal_key);
    let script_ver = script_version(cmr);
    info.control_block(&script_ver)
        .expect("control block should exist")
}

/// SHA256 hash of an address's scriptPubKey bytes.
#[must_use]
pub fn hash_script(script: &Script) -> [u8; 32] {
    let mut hasher = Sha256::new();
    sha2::digest::Update::update(&mut hasher, script.as_bytes());
    hasher.finalize().into()
}

/// Compute issuance entropy for a new asset given an outpoint and contract hash entropy.
#[must_use]
pub fn get_new_asset_entropy(outpoint: &OutPoint, entropy: [u8; 32]) -> sha256::Midstate {
    let contract_hash = ContractHash::from_byte_array(entropy);
    AssetId::generate_asset_entropy(*outpoint, contract_hash)
}
