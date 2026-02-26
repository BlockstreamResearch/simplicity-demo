use anyhow::{Result, anyhow};
use config::{Case, Config};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Settings {
    pub seed_hex: String,
}

impl Settings {
    /// Load settings from environment.
    ///
    /// # Errors
    /// Returns error if .env loading fails or `SEED_HEX` is not set.
    pub fn load() -> Result<Self> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_envdemo = PathBuf::from(manifest_dir).join(".env.demo");
        let cli_dir = "crates/cli";
        let cli_envdemo = PathBuf::from(cli_dir).join(".env.demo");
        // TODO: Should we also try the user's home directory?

        let _ = dotenvy::from_filename(".env.demo");
        let _ = dotenvy::from_filename(manifest_envdemo);
        let _ = dotenvy::from_filename(cli_envdemo);

        let cfg = Config::builder()
            .add_source(
                config::Environment::default()
                    .separator("__")
                    .convert_case(Case::ScreamingSnake),
            )
            .build()?;

        let seed_hex = cfg
            .get_string("SEED_HEX")
            .map_err(|_| anyhow!("Please create .env.demo file with SEED_HEX=..."))?;

        Ok(Self { seed_hex })
    }
}
