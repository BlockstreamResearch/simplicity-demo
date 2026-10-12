"""This installation's own local wallet.

Design decision (discussed with the user, see logs/LOG.md Step 5): each party runs
their own local instance of this app with its own wallet.json, rather than one shared
server holding every user's keys. So there is exactly one wallet per running
instance, not a multi-tenant wallet directory.
"""

import re

import tmw


def ensure_wallet(path):
    import os

    if not os.path.isfile(path):
        out, code = tmw.cli(["create-wallet", "--out", path])
        if code != 0:
            raise RuntimeError(f"create-wallet failed: {out}")
    return get_info(path)


def get_info(path):
    out, code = tmw.cli(["info", "--wallet", path])
    if code != 0:
        raise RuntimeError(f"info failed: {out}")
    pubkeys = re.findall(r"Pubkey\s*:\s*([0-9a-f]{64})", out)
    address_match = re.search(r"Receive Address \(index 0\)\s*\n\s*(\S+)", out)
    fp_match = re.search(r"Fingerprint\s*:\s*(\S+)", out)
    return {
        "fingerprint": fp_match.group(1) if fp_match else None,
        "receive_address": address_match.group(1) if address_match else None,
        "signing_pubkey": pubkeys[0] if len(pubkeys) > 0 else None,
        "oracle_pubkey": pubkeys[1] if len(pubkeys) > 1 else None,
    }


def sync(path):
    out, code = tmw.cli(["sync", "--wallet", path], timeout=60)
    if code != 0:
        raise RuntimeError(f"sync failed: {out}")
    if "(empty)" in out:
        return {"balance_sat": 0, "utxo_count": 0}
    m = re.search(r"(\d+)\s*sat\s*\((\d+)\s*UTXO", out)
    if m:
        return {"balance_sat": int(m.group(1)), "utxo_count": int(m.group(2))}
    return {"balance_sat": None, "utxo_count": None, "raw": out}
