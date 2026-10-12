"""Liquid testnet faucet: a third-party service (liquidtestnet.com), not operated
by Blockstream or the Simplicity project. See logs/LOG.md Step 2 for how this URL was
actually found (not printed anywhere on docs.simplicity-lang.org itself)."""

import json
import urllib.error
import urllib.request


def request_faucet(address):
    url = f"https://liquidtestnet.com/api/faucet?address={address}&action=lbtc"
    try:
        with urllib.request.urlopen(url, timeout=30) as r:
            return json.load(r), None
    except urllib.error.HTTPError as e:
        return None, f"faucet HTTP {e.code}: {e.read().decode(errors='replace')}"
    except Exception as e:
        return None, f"faucet request failed: {e}"
