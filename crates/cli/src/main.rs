#![warn(clippy::all, clippy::pedantic)]

//! Simplicity helper CLI for Liquid testnet.
//!
//! This binary exposes multiple subcommand groups to work with Liquid testnet:
//! - `basic`: P2PK utilities such as deriving addresses and building simple transfers.
//! - `options`: Utilities for the options contract.

mod commands;
mod explorer;
mod modules;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands::basic::P2pk;
// use crate::commands::options::Options;
// use crate::commands::smt_storage::SMTStorage;

/// Command-line entrypoint for the Simplicity helper CLI.
#[derive(Parser, Debug)]
#[command(
    name = "simplicity-demo",
    version,
    about = "SimplicityHL demo application for Liquid Testnet"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Top-level subcommand groups.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Pay to Public Key (P2PK) contract demo utilities
    P2pk {
        #[command(subcommand)]
        basic: Box<P2pk>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::P2pk { basic } => basic.handle().await,
    }
}
