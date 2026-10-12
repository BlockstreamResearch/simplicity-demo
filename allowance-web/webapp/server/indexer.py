"""Liquid testnet indexer.

The reference `simplicity-demo` repo had no address -> current-UTXO lookup at all
(every one of its commands takes an already-known txid:vout). See
Step 3(A). `tx-manifest-wallet` tracks state per contract instance it created itself,
but a beneficiary who didn't create the instance has no such state file. This module
is the missing piece: given just a contract address, find its live, unspent
allowance instances directly from Esplora, independent of any wallet or state file.
"""

import json
import os
import urllib.error
import urllib.request

# blockstream.info/liquidtestnet rate-limited this project outright (HTTP 429, no
# Retry-After) under moderate use.
# liquid.network/liquidtestnet is the same kind of public Esplora instance and
# responded fine when tried as an alternative; used as the default here for that
# reason, not because it's more official. ESPLORA_BASE lets an operator point at
# whatever instance they actually trust/control (see docs-notes/mainnet-notes.md,
# "Indexer robustness").
BASE = os.environ.get("ESPLORA_BASE", "https://liquid.network/liquidtestnet/api")


def _get(path):
    with urllib.request.urlopen(BASE + path, timeout=20) as r:
        return json.load(r)


def get_tip_height():
    with urllib.request.urlopen(BASE + "/blocks/tip/height", timeout=20) as r:
        return int(r.read().decode().strip())


def get_address_utxos(address):
    return _get(f"/address/{address}/utxo")


def instances_for(address, allowance_amount_sats, min_distance_blocks):
    tip = get_tip_height()
    utxos = get_address_utxos(address)
    instances = []
    for u in utxos:
        status = u.get("status", {})
        confirmed = bool(status.get("confirmed"))
        block_height = status.get("block_height")
        # Depth counts the funding block itself, matching BIP68's "distance" and the
        # two browser examples. Without the +1 this reports one block more waiting
        # than the network actually requires.
        blocks_since = (tip - block_height + 1) if confirmed and block_height else 0
        # A confidential output at this address has `valuecommitment` and no `value`.
        # Nobody can spend it, but it must not take the whole lookup down either:
        # u["value"] raised KeyError and hid every legitimate instance here too.
        blinded = "value" not in u
        value = None if blinded else u["value"]
        withdrawable_now = (
            not blinded and confirmed and blocks_since >= min_distance_blocks
        )
        instances.append(
            {
                "txid": u["txid"],
                "vout": u["vout"],
                "value_sats": value,
                "blinded": blinded,
                "confirmed": confirmed,
                "block_height": block_height,
                "blocks_since_confirm": blocks_since,
                "blocks_required": min_distance_blocks,
                "withdrawable_now": withdrawable_now,
                "action": None
                if blinded
                else ("FullWithdraw" if value <= allowance_amount_sats else "PartialWithdraw"),
            }
        )
    return {"tip_height": tip, "instances": instances}
