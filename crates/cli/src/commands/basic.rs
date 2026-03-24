use ansi_term::Style;
use ansi_term::Color::Yellow;

use crate::modules::utils::derive_keypair;

use crate::commands::NETWORK;
use crate::explorer::{broadcast_tx, fetch_utxo};
use crate::faucet::faucet_txid;

use clap::Subcommand;
use simplicityhl::elements::pset::serialize::Serialize;
use simplicityhl::simplicity::elements::{Address, OutPoint};
use simplicityhl::simplicity::hex::DisplayHex;

use simplicityhl::tracker::TrackerLogLevel;
use simplicityhl_core::{
    create_p2pk_signature, finalize_p2pk_transaction, get_p2pk_address, hash_script,
};

fn embolden(s: &str) -> String {
    Style::new().bold().paint(s).to_string()
}

fn yellow(s: &str) -> String {
    Yellow.paint(s).to_string()
}

#[derive(Subcommand, Debug)]
pub enum P2pk {
    /// Derive a Liquid Testnet address for P2PK program using specified pubkey
    CompileToTestnetAddress {
        /// Address index (0-based)
        #[arg(long = "index", default_value_t = 0)]
        index: u32,
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
    /// Build tx sending LBTC from P2PK contract instance to specified recipient
    SpendFromP2pkContract {
        /// Transaction id (hex) and output index (vout) of the UTXO you will spend
        #[arg(long = "utxo")]
        utxo_outpoint: OutPoint,
        /// Recipient address (Liquid testnet bech32m)
        #[arg(long = "to-address")]
        to_address: Address,
        /// Amount to send to the recipient in satoshis (LBTC)
        #[arg(long = "send-sats")]
        amount_to_send: u64,
        /// Miner fee in satoshis (LBTC)
        #[arg(long = "fee-sats")]
        fee_amount: u64,
        /// Account that will pay for transaction fees and that owns a token to send
        #[arg(long = "account-index", default_value_t = 0)]
        account_index: u32,
        /// When set, broadcast the built transaction via Esplora and print txid
        #[arg(long = "broadcast")]
        broadcast: bool,
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
}

impl P2pk {
    /// Handle P2PK CLI subcommand execution.
    ///
    /// # Errors
    /// Returns error if the subcommand operation fails.
    #[expect(clippy::too_many_lines)]
    pub async fn handle(&self) -> anyhow::Result<()> {
        match self {
            Self::CompileToTestnetAddress { index, verbose } => {
                print!("{}", Style::new().bold().paint("# Deriving keypair from seed"));
                if *verbose > 0
                    { println!("{}", Style::new().bold().paint(format!(" with key index {}.", index))); }
                else
                    { println!("{}", Style::new().bold().paint(".")); }
                let keypair = derive_keypair(*index);
                if *verbose > 0 {
                    println!("{}", Style::new().bold().paint("# Derived public/private keypair."));
                    println!("{}: {}", yellow("---> Public key"), keypair.x_only_public_key().0);
                } // VERBOSE

                let public_key = keypair.x_only_public_key().0;
                let address = get_p2pk_address(&public_key, NETWORK)?;

                let mut script_hash: [u8; 32] = hash_script(&address.script_pubkey());
                script_hash.reverse();

                if *verbose > 0 { println!("{}: {}", yellow("---> Script hash"), hex::encode(script_hash)); } // VERBOSE
                println!("\n{}: {}", embolden("Contract's Liquid Testnet address"), address);

                Ok(())
            }

            Self::FundFromFaucet { address, verbose } => {
                println!("Connecting to faucet API...");
                let txid = faucet_txid(&address.to_string(), verbose).expect("Faucet request failed");
                println!("Sent tLBTC to {}.", address);
                println!("Transaction ID: {}", txid);
                Ok(())
            }

            Self::SpendFromP2pkContract {
                utxo_outpoint,
                to_address,
                amount_to_send,
                fee_amount,
                account_index,
                broadcast,
                verbose,
            } => {
                print!("{}", Style::new().bold().paint("# Deriving keypair from seed"));
                if *verbose > 0
                    { println!("{}", embolden(&format!(" with key index {}.", account_index))); }
                else
                    { println!("{}", embolden(".")); }
                let keypair = derive_keypair(*account_index);
                if *verbose > 0 { println!("{} Public key: {}", yellow("---> Derived public/private keypair."), keypair.x_only_public_key().0); } // VERBOSE

                let tx_out = fetch_utxo(*utxo_outpoint).await?;

                let pst = contracts::sdk::transfer_native(
                    (*utxo_outpoint, tx_out.clone()),
                    to_address,
                    *amount_to_send,
                    *fee_amount,
                    verbose,
                )?;

                let tx = pst.extract_tx()?;
                let utxos = &[tx_out];

                let x_only_public_key = keypair.x_only_public_key().0;
                println!("{}", embolden("# Signing transaction with private key."));
                let signature = create_p2pk_signature(&tx, utxos, &keypair, 0, NETWORK, verbose)?;
                if *verbose > 0 { println!("{}: {}", yellow("---> Signature"), signature); } // VERBOSE

                let tx = finalize_p2pk_transaction(
                    tx,
                    utxos,
                    &x_only_public_key,
                    &signature,
                    0,
                    NETWORK,
                    TrackerLogLevel::None,
                    &verbose,
                )?;

                if *broadcast {
                    let txid = broadcast_tx(&tx).await?;
                    println!("\n{}: {}", embolden("Broadcasted txid"), txid);
                    println!("{}: https://blockstream.info/liquidtestnet/tx/{}?expand", embolden("View details at"), txid);
                } else {
                    println!("\n{}:\n{}", embolden("Transaction"), tx.serialize().to_lower_hex_string());
                }

                Ok(())
            }
        }
    }
}
