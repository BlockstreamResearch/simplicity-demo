use ansi_term::Color::Yellow;
use ansi_term::Style;

use crate::modules::utils::derive_keypair;

use crate::commands::NETWORK;
use crate::explorer::{broadcast_tx, fetch_utxo};
use crate::faucet::faucet_txid;

use clap::Subcommand;
use simplicityhl::CompiledProgram;
use simplicityhl::elements::pset::serialize::Serialize;
use simplicityhl::elements::schnorr::Keypair;
use simplicityhl::simplicity::elements::{Address, OutPoint, Transaction, TxOut};
use simplicityhl::simplicity::hex::DisplayHex;

use simplicityhl::tracker::TrackerLogLevel;
use simplicityhl_core::{
    LastWillAction, LastWillKeys, NUMS_INTERNAL_KEY, create_last_will_signature, finalize_transaction,
    get_last_will_program, last_will_address, last_will_witness_values,
};

fn embolden(s: &str) -> String {
    Style::new().bold().paint(s).to_string()
}

fn yellow(s: &str) -> String {
    Yellow.paint(s).to_string()
}

/// Key-derivation indices and timelock length shared by every `LastWill` subcommand.
///
/// All three roles are derived from the same seed used by `derive_keypair` (see the
/// P2PK demo), at different indices, purely so a single wallet seed can play every role
/// in this walkthrough. In a real last-will contract the hot, cold, and inheritor keys
/// would belong to different people and never share a seed.
#[derive(clap::Args, Debug, Clone, Copy)]
pub struct LastWillKeyArgs {
    /// Derivation index for the owner's hot key (proves they're alive)
    #[arg(long = "hot-index", default_value_t = 0)]
    hot_index: u32,
    /// Derivation index for the owner's cold key (not exercised by this walkthrough)
    #[arg(long = "cold-index", default_value_t = 1)]
    cold_index: u32,
    /// Derivation index for the inheritor's key
    #[arg(long = "inheritor-index", default_value_t = 2)]
    inheritor_index: u32,
    /// Minimum number of blocks the inheritor must wait, after the contract is funded
    /// or last refreshed, before they can claim the coins
    #[arg(long = "min-distance-blocks", default_value_t = 3)]
    min_distance_blocks: u16,
}

impl LastWillKeyArgs {
    /// Derive all three public keys, for compiling or recompiling the contract.
    fn derive_keys(self) -> LastWillKeys {
        LastWillKeys {
            hot_public_key: derive_keypair(self.hot_index).x_only_public_key().0,
            cold_public_key: derive_keypair(self.cold_index).x_only_public_key().0,
            inheritor_public_key: derive_keypair(self.inheritor_index).x_only_public_key().0,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum LastWill {
    /// Derive a Liquid Testnet address for a last-will contract instance
    CompileToTestnetAddress {
        #[command(flatten)]
        keys: LastWillKeyArgs,
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
    /// Fund a Liquid Testnet address with tLBTC from the Liquid Testnet Faucet
    FundFromFaucet {
        #[arg(long = "address")]
        address: Address,
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
    /// Prove the owner is still alive: sign with the hot key to re-create the covenant,
    /// resetting the inheritor's timelock
    ProveAlive {
        /// Transaction id (hex) and output index (vout) of the last-will UTXO to refresh
        #[arg(long = "utxo")]
        utxo_outpoint: OutPoint,
        /// Miner fee in satoshis (LBTC)
        #[arg(long = "fee-sats")]
        fee_amount: u64,
        #[command(flatten)]
        keys: LastWillKeyArgs,
        /// When set, broadcast the built transaction via Esplora and print txid
        #[arg(long = "broadcast")]
        broadcast: bool,
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
    /// Spend the coins as the inheritor.
    ///
    /// Run this before the covenant's timelock has genuinely elapsed and the contract's
    /// local check still passes (since it only compares against the *declared* sequence
    /// number), but the node itself rejects the broadcast with a `non-BIP68-final` error
    /// because not enough blocks have actually passed since the contract was last
    /// funded or refreshed. Run the identical command again once that many blocks have
    /// passed and it succeeds.
    SpendAsInheritor {
        /// Transaction id (hex) and output index (vout) of the UTXO you will spend
        #[arg(long = "utxo")]
        utxo_outpoint: OutPoint,
        /// Recipient address (Liquid testnet bech32m)
        #[arg(long = "to-address")]
        to_address: Address,
        /// Miner fee in satoshis (LBTC)
        #[arg(long = "fee-sats")]
        fee_amount: u64,
        #[command(flatten)]
        keys: LastWillKeyArgs,
        /// When set, broadcast the built transaction via Esplora and print txid
        #[arg(long = "broadcast")]
        broadcast: bool,
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
}

impl LastWill {
    /// Handle last-will CLI subcommand execution.
    ///
    /// # Errors
    /// Returns error if the subcommand operation fails.
    pub async fn handle(&self) -> anyhow::Result<()> {
        match self {
            Self::CompileToTestnetAddress { keys: key_args, verbose } => {
                println!("{}", embolden("# Deriving keypairs from seed."));
                let last_will_keys = key_args.derive_keys();

                let program = get_last_will_program(last_will_keys, key_args.min_distance_blocks)?;
                let address = last_will_address(&program, NETWORK);

                if *verbose > 0 {
                    println!("{}: {}", yellow("---> Inheritor public key"), last_will_keys.inheritor_public_key);
                    println!("{}: {}", yellow("---> Cold public key"), last_will_keys.cold_public_key);
                    println!("{}: {}", yellow("---> Hot public key"), last_will_keys.hot_public_key);
                } // VERBOSE

                println!("\n{}: {}", embolden("Contract's Liquid Testnet address"), address);

                Ok(())
            }

            Self::FundFromFaucet { address, verbose } => {
                println!("Connecting to faucet API...");
                let txid = faucet_txid(&address.to_string(), verbose).expect("Faucet request failed");
                println!("Sent tLBTC to {address}.");
                println!("Transaction ID: {txid}");
                Ok(())
            }

            Self::ProveAlive { utxo_outpoint, fee_amount, keys: key_args, broadcast, verbose } => {
                println!("{}", embolden("# Deriving keypairs from seed."));
                // Derive the hot key once and read the other two roles' public keys off of
                // it/their own keypairs, rather than deriving the hot key a second time.
                let hot_keypair = derive_keypair(key_args.hot_index);
                let last_will_keys = LastWillKeys {
                    hot_public_key: hot_keypair.x_only_public_key().0,
                    cold_public_key: derive_keypair(key_args.cold_index).x_only_public_key().0,
                    inheritor_public_key: derive_keypair(key_args.inheritor_index).x_only_public_key().0,
                };

                let program = get_last_will_program(last_will_keys, key_args.min_distance_blocks)?;
                let contract_address = last_will_address(&program, NETWORK);

                let tx_out = fetch_utxo(*utxo_outpoint).await?;

                let pst = contracts::sdk::refresh_last_will(
                    (*utxo_outpoint, tx_out.clone()),
                    &contract_address,
                    *fee_amount,
                    verbose,
                )?;

                let tx = pst.extract_tx()?;
                let utxos = &[tx_out];

                println!("{}", embolden("# Signing transaction with hot key."));
                let tx = sign_and_finalize(
                    tx,
                    utxos,
                    &program,
                    &hot_keypair,
                    LastWillAction::HotSpend,
                    verbose,
                )?;

                broadcast_or_print(&tx, *broadcast).await
            }

            Self::SpendAsInheritor { utxo_outpoint, to_address, fee_amount, keys: key_args, broadcast, verbose } => {
                println!("{}", embolden("# Deriving keypairs from seed."));
                let inheritor_keypair = derive_keypair(key_args.inheritor_index);
                let last_will_keys = LastWillKeys {
                    hot_public_key: derive_keypair(key_args.hot_index).x_only_public_key().0,
                    cold_public_key: derive_keypair(key_args.cold_index).x_only_public_key().0,
                    inheritor_public_key: inheritor_keypair.x_only_public_key().0,
                };

                let program = get_last_will_program(last_will_keys, key_args.min_distance_blocks)?;

                let tx_out = fetch_utxo(*utxo_outpoint).await?;

                let pst = contracts::sdk::spend_as_inheritor(
                    (*utxo_outpoint, tx_out.clone()),
                    to_address,
                    *fee_amount,
                    key_args.min_distance_blocks,
                    verbose,
                )?;

                let tx = pst.extract_tx()?;
                let utxos = &[tx_out];

                println!("{}", embolden("# Signing transaction with inheritor key."));
                let tx = sign_and_finalize(
                    tx,
                    utxos,
                    &program,
                    &inheritor_keypair,
                    LastWillAction::Inherit,
                    verbose,
                )?;

                broadcast_or_print(&tx, *broadcast).await
            }
        }
    }
}

/// Sign the given transaction's spend of a last-will input, build its `ACTION` witness,
/// and attach both to the transaction -- the common tail shared by every spend of this
/// contract, regardless of which of the three actions is being exercised.
fn sign_and_finalize(
    tx: Transaction,
    utxos: &[TxOut],
    program: &CompiledProgram,
    keypair: &Keypair,
    action: LastWillAction,
    verbose: &u8,
) -> anyhow::Result<Transaction> {
    let signature = create_last_will_signature(&tx, utxos, program, keypair, 0, NETWORK, verbose)?;
    let witness_values = last_will_witness_values(program, action, &signature)?;

    Ok(finalize_transaction(
        tx,
        program,
        &NUMS_INTERNAL_KEY,
        utxos,
        0,
        witness_values,
        NETWORK,
        TrackerLogLevel::None,
        verbose,
    )?)
}

async fn broadcast_or_print(
    tx: &simplicityhl::simplicity::elements::Transaction,
    broadcast: bool,
) -> anyhow::Result<()> {
    if broadcast {
        let txid = broadcast_tx(tx).await?;
        println!("\n{}: {}", embolden("Broadcasted txid"), txid);
        println!(
            "{}: https://blockstream.info/liquidtestnet/tx/{}?expand",
            embolden("View details at"),
            txid
        );
    } else {
        println!("\n{}:\n{}", embolden("Transaction"), tx.serialize().to_lower_hex_string());
    }
    Ok(())
}
