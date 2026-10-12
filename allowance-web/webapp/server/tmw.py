"""Thin wrapper around the `tx-manifest-wallet` binary.

We shell out to the real, prebuilt CLI rather than reimplementing any
covenant-compilation, PSET-building, signing, or broadcasting logic here. See
logs/LOG.md Step 5/6 for why. `run` is an interactive wizard that hard-checks isatty
on stdin, so non-interactive use requires a real pseudo-terminal; see
logs/txmanifest-implementation-log.md for how that was first discovered.
"""

import json
import os
import pty
import re
import select
import subprocess
import time

ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def find_binary() -> str:
    for candidate in (
        os.environ.get("TMW_BIN"),
        os.path.join(os.path.dirname(__file__), "bin", "tx-manifest-wallet"),
        "/home/schoen/src/simplicity-dev-experiment-1/logs/bin/"
        "tx-manifest-wallet-v0.2.2-x86_64-unknown-linux-gnu/tx-manifest-wallet",
    ):
        if candidate and os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    raise RuntimeError(
        "tx-manifest-wallet binary not found. Set TMW_BIN to its path, or place it "
        "at webapp/server/bin/tx-manifest-wallet. Download from "
        "https://github.com/stringhandler/txmanifest-wallet/releases "
        "(tx-manifest-wallet-<version>-x86_64-unknown-linux-gnu.tar.gz on Linux)."
    )


class TmwError(RuntimeError):
    def __init__(self, message, raw_output=""):
        super().__init__(message)
        self.raw_output = raw_output


def _run_pty(argv, cwd, responses, quiet_seconds=1.2, max_seconds=150):
    """Run argv in a real pty, sending queued responses after quiet gaps.

    This mirrors prompts empirically observed from `run`: a "Fee rate (sat/vb)"
    prompt and a "Broadcast transaction? [y/N]" prompt, in that order, though not
    every action shows the fee prompt (e.g. it can be cached). Sending unused queued
    responses is harmless since nothing reads them once the process exits.

    Termination must be driven by EOF (the child closing its end of the pty when it
    exits) or the hard timeout, and NOT by "no output for a while, so it must be done".
    An earlier version used a quiet-period heuristic to stop reading once the
    response queue was empty; under concurrent load (this module is called from a
    multithreaded server) a child can legitimately pause for several seconds mid-task
    (e.g. waiting on the broadcast HTTP call) longer than that heuristic's threshold,
    so the loop would give up and stop reading *before* the child printed its
    success/failure line, even though the child went on to finish (and, for Fund,
    broadcast a real transaction) after we'd already stopped listening. Caught this
    empirically: a Fund call returned an unparseable-output error, but the wallet's
    balance afterward showed the funding transaction had gone out anyway. Only EOF
    (or the timeout, as a last-resort safety net) is a real completion signal.
    """
    responses = list(responses)
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        try:
            os.execvp(argv[0], argv)
        except Exception:
            os._exit(127)
    buf = b""
    last = time.time()
    start = time.time()
    while True:
        if time.time() - start > max_seconds:
            buf += b"\n[tmw.py: timed out waiting for the process]"
            break
        r, _, _ = select.select([fd], [], [], 0.3)
        if fd in r:
            try:
                chunk = os.read(fd, 4096)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
            last = time.time()
        else:
            now = time.time()
            if responses and (now - last > quiet_seconds):
                resp = responses.pop(0)
                os.write(fd, (resp + "\n").encode())
                last = now
    try:
        _, status = os.waitpid(pid, 0)
    except ChildProcessError:
        status = 0
    return buf.decode(errors="replace"), status


def write_instance_file(path, *, allowance_amount_sats, min_distance_blocks, beneficiary_pubkey_hex):
    """Build an instance.json a beneficiary can construct themselves from public
    params, without ever having run the Fund (constructor) action. The instance file
    only ever records the compiled params (public data), never a private key."""
    doc = {
        "instance": {
            "template": "allowance_contract",
            "fields": {
                "ALLOWANCE_AMOUNT": str(allowance_amount_sats),
                "BENEFICIARY_KEY": beneficiary_pubkey_hex,
                "MIN_DISTANCE": str(min_distance_blocks),
            },
        }
    }
    with open(path, "w") as f:
        json.dump(doc, f, indent=2)


def run_action(
    *,
    manifest_path,
    action,
    wallet_path,
    cwd,
    params_path=None,
    instance_path=None,
    instance_out=None,
    state_path=None,
    state_out=None,
    inputs=None,
    responses=("1.0", "y"),
):
    """Drive `tx-manifest-wallet run <manifest> <action> ...` and parse the result.

    Returns a dict: {ok, address, txid, error, raw}. `raw` is always included (with
    ANSI codes stripped) so callers/log entries can show real tool output rather than
    a lossy summary of it.
    """
    # _run_pty chdir()s the child to `cwd` before exec, so any relative path here would
    # then resolve against that new cwd, not the caller's, and silently double up
    # (e.g. "./data/contracts/x/params.json" run from cwd ".../contracts/x" looks for
    # ".../contracts/x/data/contracts/x/params.json"). Found exactly this bug via a
    # live 502 with "No such file or directory" in the raw output. Absolutize
    # everything up front so callers never have to reason about this.
    manifest_path = os.path.abspath(manifest_path)
    wallet_path = os.path.abspath(wallet_path)
    cwd = os.path.abspath(cwd)
    argv = [find_binary(), "run", manifest_path, action, "--wallet", wallet_path]
    if params_path:
        argv += ["--params", os.path.abspath(params_path)]
    if instance_path:
        argv += ["--instance", os.path.abspath(instance_path)]
    if instance_out:
        argv += ["--instance-out", os.path.abspath(instance_out)]
    if state_path:
        argv += ["--state", os.path.abspath(state_path)]
    if state_out:
        argv += ["--state-out", os.path.abspath(state_out)]
    for input_id, outpoint in (inputs or {}).items():
        argv += ["--input", f"{input_id}={outpoint}"]

    raw, _status = _run_pty(argv, cwd, list(responses))
    clean = strip_ansi(raw)

    txid_match = re.search(r"Broadcast txid:\s*([0-9a-f]{64})", clean)
    address_match = re.search(r"\[covenant\] address:\s*(\S+)", clean)
    fail_match = re.search(r"(?:\[error\]\s*|Broadcast failed:\s*)(.+)", clean)

    if txid_match:
        return {
            "ok": True,
            "txid": txid_match.group(1),
            "address": address_match.group(1) if address_match else None,
            "error": None,
            "raw": clean,
        }
    if fail_match:
        return {
            "ok": False,
            "txid": None,
            "address": address_match.group(1) if address_match else None,
            "error": fail_match.group(1).strip(),
            "raw": clean,
        }
    return {
        "ok": False,
        "txid": None,
        "address": address_match.group(1) if address_match else None,
        "error": "Could not detect success or a known failure pattern in tool "
        "output; see raw output for details.",
        "raw": clean,
    }


def cli(argv_tail, cwd=None, timeout=30):
    """Run a non-interactive subcommand (validate/info/sync/create-wallet/...)."""
    result = subprocess.run(
        [find_binary()] + argv_tail,
        cwd=cwd,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return strip_ansi(result.stdout) + strip_ansi(result.stderr), result.returncode
