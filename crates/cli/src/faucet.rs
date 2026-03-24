use serde_json::Value;
use std::error::Error;

pub fn faucet_txid(address: &str, verbose: &u8) -> Result<String, Box<dyn Error>> {
    let url = format!("https://liquidtestnet.com/api/faucet?address={}&action=lbtc", address);
    if *verbose > 0 {
        println!("Requesting Liquid Testnet faucet API URL: {}", url);
    }

    let txid = ureq::get(&url)
        .call()?
        .into_json::<Value>()?
        .get("txid")
        .and_then(|v| v.as_str())
        .ok_or("Field 'txid' not found")?
        .to_string();

    Ok(txid)
}
