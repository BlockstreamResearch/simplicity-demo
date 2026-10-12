"""Allowance web app backend.

Drives the real `tx-manifest-wallet` engine (see tmw.py) for every contract-touching
operation instead of reimplementing PSET/signing logic. One local wallet per running
instance (see wallet_store.py for why). Serves the static frontend from ../frontend.

Run: python3 app.py [--port 8787] [--data-dir ./data]
"""

import argparse
import json
import os
import re
import secrets
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

sys.path.insert(0, os.path.dirname(__file__))
import faucet
import indexer
import tmw
import wallet_store

HERE = os.path.dirname(os.path.abspath(__file__))
MANIFEST_DIR = os.path.abspath(os.path.join(HERE, "..", "..", "examples", "allowance"))
MANIFEST_PATH = os.path.join(MANIFEST_DIR, "txmanifest.json")
FRONTEND_DIR = os.path.abspath(os.path.join(HERE, "..", "frontend"))

PUBKEY_RE = re.compile(r"^[0-9a-fA-F]{64}$")

_lock = threading.Lock()  # tx-manifest-wallet touches shared wallet state files; serialize


class ApiError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status
        self.message = message


def data_dir():
    return app_config["data_dir"]


def wallet_path():
    return os.path.join(data_dir(), "wallet.json")


def contracts_dir():
    d = os.path.join(data_dir(), "contracts")
    os.makedirs(d, exist_ok=True)
    return d


def require_positive_int(body, key, max_value=None):
    if key not in body:
        raise ApiError(400, f"missing '{key}'")
    try:
        value = int(body[key])
    except (TypeError, ValueError):
        raise ApiError(400, f"'{key}' must be an integer")
    if value <= 0:
        raise ApiError(400, f"'{key}' must be positive")
    if max_value is not None and value > max_value:
        raise ApiError(400, f"'{key}' must be <= {max_value}")
    return value


def require_pubkey(body, key):
    value = body.get(key, "")
    if not PUBKEY_RE.match(value or ""):
        raise ApiError(400, f"'{key}' must be 64 hex characters (BIP340 x-only pubkey)")
    return value.lower()


def handle_get_wallet():
    with _lock:
        info = wallet_store.ensure_wallet(wallet_path())
        bal = wallet_store.sync(wallet_path())
    return {"network": "liquidtestnet", **info, **bal}


def handle_post_wallet_faucet():
    with _lock:
        info = wallet_store.ensure_wallet(wallet_path())
    result, err = faucet.request_faucet(info["receive_address"])
    if err:
        raise ApiError(502, err)
    return {"txid": result.get("txid"), "raw": result}


def handle_post_wallet_sync():
    with _lock:
        return wallet_store.sync(wallet_path())


def handle_post_contracts(body):
    allowance_amount = require_positive_int(body, "allowance_amount_sats")
    min_distance = require_positive_int(body, "min_distance_blocks", max_value=65535)
    beneficiary_pubkey = require_pubkey(body, "beneficiary_pubkey_hex")
    fund_amount = require_positive_int(body, "fund_amount_sats")

    with _lock:
        wallet_store.ensure_wallet(wallet_path())

        contract_id = secrets.token_hex(4)
        cdir = os.path.join(contracts_dir(), contract_id)
        os.makedirs(cdir, exist_ok=True)

        params_path = os.path.join(cdir, "params.json")
        with open(params_path, "w") as f:
            json.dump(
                {
                    "BENEFICIARY_KEY": beneficiary_pubkey,
                    "ALLOWANCE_AMOUNT": str(allowance_amount),
                    "MIN_DISTANCE": str(min_distance),
                    "amount_sat": str(fund_amount),
                },
                f,
            )

        instance_out = os.path.join(cdir, "allowance.instance.json")
        state_out = os.path.join(cdir, "allowance.state.json")

        result = tmw.run_action(
            manifest_path=MANIFEST_PATH,
            action="Fund",
            wallet_path=os.path.abspath(wallet_path()),
            cwd=cdir,
            params_path=params_path,
            instance_out=instance_out,
            state_out=state_out,
        )

    if not result["ok"]:
        sys.stderr.write("\n----- run_action raw output (Fund failure) -----\n")
        sys.stderr.write(result["raw"])
        sys.stderr.write("\n----- end raw output -----\n")
        raise ApiError(502, f"Fund action failed: {result['error']}")

    meta = {
        "contract_id": contract_id,
        "address": result["address"],
        "fund_txid": result["txid"],
        "params": {
            "allowance_amount_sats": allowance_amount,
            "min_distance_blocks": min_distance,
            "beneficiary_pubkey_hex": beneficiary_pubkey,
        },
        "fund_amount_sats": fund_amount,
    }
    with open(os.path.join(cdir, "meta.json"), "w") as f:
        json.dump(meta, f, indent=2)
    return meta


def handle_get_contracts():
    out = []
    for entry in sorted(os.listdir(contracts_dir())):
        meta_path = os.path.join(contracts_dir(), entry, "meta.json")
        if os.path.isfile(meta_path):
            with open(meta_path) as f:
                out.append(json.load(f))
    return {"contracts": out}


def handle_get_contract(contract_id):
    meta_path = os.path.join(contracts_dir(), contract_id, "meta.json")
    if not os.path.isfile(meta_path):
        raise ApiError(404, "unknown contract_id")
    with open(meta_path) as f:
        return json.load(f)


def handle_get_lookup(qs):
    address = (qs.get("address") or [None])[0]
    if not address:
        raise ApiError(400, "missing 'address'")
    allowance_amount = int((qs.get("allowance_amount_sats") or [0])[0] or 0)
    min_distance = int((qs.get("min_distance_blocks") or [0])[0] or 0)
    if allowance_amount <= 0 or min_distance <= 0:
        raise ApiError(400, "missing or invalid allowance_amount_sats/min_distance_blocks")
    result = indexer.instances_for(address, allowance_amount, min_distance)
    return {"address": address, **result}


def handle_post_claim(body):
    address = body.get("address")
    if not address:
        raise ApiError(400, "missing 'address'")
    allowance_amount = require_positive_int(body, "allowance_amount_sats")
    min_distance = require_positive_int(body, "min_distance_blocks", max_value=65535)
    beneficiary_pubkey = require_pubkey(body, "beneficiary_pubkey_hex")
    txid = body.get("txid")
    vout = body.get("vout")
    action = body.get("action")
    if action not in ("PartialWithdraw", "FullWithdraw"):
        raise ApiError(400, "'action' must be 'PartialWithdraw' or 'FullWithdraw'")
    if not txid or vout is None:
        raise ApiError(400, "missing 'txid'/'vout'")

    with _lock:
        info = wallet_store.ensure_wallet(wallet_path())
        if info["signing_pubkey"] != beneficiary_pubkey:
            raise ApiError(
                400,
                "This app instance's own wallet key does not match this contract's "
                "beneficiary key. Run your own local instance with your own wallet "
                "to claim as this beneficiary (see logs/LOG.md Step 5 for why keys "
                "are never shared across instances).",
            )

        # Re-derive eligibility server-side rather than trusting the client's claimed action.
        check = indexer.instances_for(address, allowance_amount, min_distance)
        match = next(
            (i for i in check["instances"] if i["txid"] == txid and i["vout"] == int(vout)),
            None,
        )
        if match is None:
            raise ApiError(400, "that UTXO is not currently a live, unspent instance at this address")
        if match["blinded"]:
            raise ApiError(
                400,
                "that output is confidential, so the contract cannot read its own asset "
                "and amount; no withdrawal of it can ever be built",
            )
        if not match["withdrawable_now"]:
            remaining = match["blocks_required"] - match["blocks_since_confirm"]
            raise ApiError(400, f"timelock not yet satisfied: wait {remaining} more block(s)")
        if match["action"] != action:
            raise ApiError(
                400,
                f"'{action}' is not valid for this UTXO's current balance; use "
                f"'{match['action']}' instead",
            )

        claim_dir = os.path.join(data_dir(), "claims", secrets.token_hex(4))
        os.makedirs(claim_dir, exist_ok=True)
        instance_path = os.path.join(claim_dir, "allowance.instance.json")
        tmw.write_instance_file(
            instance_path,
            allowance_amount_sats=allowance_amount,
            min_distance_blocks=min_distance,
            beneficiary_pubkey_hex=beneficiary_pubkey,
        )

        result = tmw.run_action(
            manifest_path=MANIFEST_PATH,
            action=action,
            wallet_path=os.path.abspath(wallet_path()),
            cwd=claim_dir,
            instance_path=instance_path,
            inputs={"allowance_in": f"{txid}:{vout}"},
            state_out=os.path.join(claim_dir, "allowance.state.json"),
        )

    if not result["ok"]:
        sys.stderr.write("\n----- run_action raw output (claim failure) -----\n")
        sys.stderr.write(result["raw"])
        sys.stderr.write("\n----- end raw output -----\n")
        raise ApiError(502, f"{action} failed: {result['error']}")
    return {"kind": action, "txid": result["txid"], "broadcasted": True}


def handle_get_manifest():
    with open(MANIFEST_PATH) as f:
        manifest = json.load(f)
    return {
        "manifest": manifest,
        "status": "real: this is the actual manifest driving this app's own "
        "backend via tx-manifest-wallet, not a speculative/illustrative document.",
    }


ROUTES = [
    ("GET", re.compile(r"^/api/wallet$"), lambda m, qs, body: handle_get_wallet()),
    ("POST", re.compile(r"^/api/wallet/faucet$"), lambda m, qs, body: handle_post_wallet_faucet()),
    ("POST", re.compile(r"^/api/wallet/sync$"), lambda m, qs, body: handle_post_wallet_sync()),
    ("POST", re.compile(r"^/api/contracts$"), lambda m, qs, body: handle_post_contracts(body)),
    ("GET", re.compile(r"^/api/contracts$"), lambda m, qs, body: handle_get_contracts()),
    ("GET", re.compile(r"^/api/contracts/([0-9a-f]+)$"), lambda m, qs, body: handle_get_contract(m.group(1))),
    ("GET", re.compile(r"^/api/lookup$"), lambda m, qs, body: handle_get_lookup(qs)),
    ("POST", re.compile(r"^/api/claim$"), lambda m, qs, body: handle_post_claim(body)),
    ("GET", re.compile(r"^/api/manifest$"), lambda m, qs, body: handle_get_manifest()),
]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _dispatch(self, method):
        parsed = urlparse(self.path)
        if parsed.path.startswith("/api/"):
            qs = parse_qs(parsed.query)
            body = {}
            if method == "POST":
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b"{}"
                try:
                    body = json.loads(raw or b"{}")
                except json.JSONDecodeError:
                    return self._send_json(400, {"error": "invalid JSON body"})
            for m, pattern, fn in ROUTES:
                if m != method:
                    continue
                match = pattern.match(parsed.path)
                if match:
                    try:
                        return self._send_json(200, fn(match, qs, body))
                    except ApiError as e:
                        return self._send_json(e.status, {"error": e.message})
                    except Exception as e:
                        return self._send_json(500, {"error": f"{type(e).__name__}: {e}"})
            return self._send_json(404, {"error": "no such API route"})
        if method == "GET":
            return self._serve_static(parsed.path)
        return self._send_json(405, {"error": "method not allowed"})

    def _serve_static(self, path):
        if path == "/":
            path = "/index.html"
        safe = os.path.normpath(path).lstrip("/")
        full = os.path.join(FRONTEND_DIR, safe)
        if not full.startswith(FRONTEND_DIR) or not os.path.isfile(full):
            self.send_response(404)
            self.end_headers()
            return
        ctype = {
            ".html": "text/html",
            ".js": "application/javascript",
            ".css": "text/css",
            ".json": "application/json",
        }.get(os.path.splitext(full)[1], "application/octet-stream")
        with open(full, "rb") as f:
            content = f.read()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def do_GET(self):
        self._dispatch("GET")

    def do_POST(self):
        self._dispatch("POST")


app_config = {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=int(os.environ.get("PORT", 8787)))
    parser.add_argument("--data-dir", default=os.environ.get("DATA_DIR", os.path.join(HERE, "data")))
    args = parser.parse_args()
    os.makedirs(args.data_dir, exist_ok=True)
    app_config["data_dir"] = os.path.abspath(args.data_dir)

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Allowance app listening on http://127.0.0.1:{args.port}  (data dir: {args.data_dir})")
    server.serve_forever()


if __name__ == "__main__":
    main()
