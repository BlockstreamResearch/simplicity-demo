const API = "/api";

document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.getElementById("tab-" + btn.dataset.tab).classList.add("active");
  });
});

async function api(path, opts) {
  const res = await fetch(API + path, opts);
  let body;
  try { body = await res.json(); } catch { body = null; }
  if (!res.ok) {
    const msg = (body && body.error) ? body.error : `HTTP ${res.status}`;
    throw new Error(msg);
  }
  return body;
}

function walletBoxHtml(w) {
  return `<dl>
    <dt>Receive address</dt><dd><code>${w.receive_address}</code></dd>
    <dt>Public key</dt><dd><code>${w.signing_pubkey}</code></dd>
    <dt>Balance</dt><dd>${w.balance_sat != null ? w.balance_sat.toLocaleString() + " sat (" + w.utxo_count + " UTXO)" : "unknown"}</dd>
  </dl>`;
}

// ---------- benefactor ----------
let benefactorWallet = null;

async function loadBenefactorWallet() {
  const box = document.getElementById("benefactor-wallet");
  try {
    benefactorWallet = await api("/wallet");
    box.innerHTML = walletBoxHtml(benefactorWallet);
  } catch (err) {
    box.textContent = "Could not load wallet: " + err.message;
  }
}
loadBenefactorWallet();

document.getElementById("benefactor-faucet-btn").addEventListener("click", async () => {
  const out = document.getElementById("benefactor-faucet-result");
  out.textContent = "Requesting…";
  try {
    const r = await api("/wallet/faucet", { method: "POST" });
    out.textContent = "Sent. Faucet txid: " + r.txid + ". Click Refresh balance once it confirms (~1 min).";
  } catch (err) {
    out.textContent = "Faucet request failed: " + err.message;
  }
});

document.getElementById("benefactor-sync-btn").addEventListener("click", async () => {
  const box = document.getElementById("benefactor-wallet");
  box.textContent = "Syncing…";
  try {
    const bal = await api("/wallet/sync", { method: "POST" });
    benefactorWallet = { ...benefactorWallet, ...bal };
    box.innerHTML = walletBoxHtml(benefactorWallet);
  } catch (err) {
    box.textContent = "Sync failed: " + err.message;
  }
});

const amountInput = document.getElementById("amount-sats");
const amountBtc = document.getElementById("amount-btc");
function updateAmountHint() {
  const sats = Number(amountInput.value || 0);
  amountBtc.textContent = "≈ " + (sats / 1e8).toFixed(8) + " tLBTC";
}
amountInput.addEventListener("input", updateAmountHint);
updateAmountHint();

const distanceInput = document.getElementById("min-distance");
const distanceMinutes = document.getElementById("distance-minutes");
function updateDistanceHint() {
  const blocks = Number(distanceInput.value || 0);
  distanceMinutes.textContent = "≈ " + blocks + " min on testnet (~1 block/min)";
}
distanceInput.addEventListener("input", updateDistanceHint);
updateDistanceHint();

document.getElementById("create-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const submitBtn = e.target.querySelector("button[type=submit]");
  const status = document.getElementById("create-status");
  submitBtn.disabled = true;
  status.textContent = "Compiling contract and broadcasting the funding transaction. This takes a few seconds…";
  try {
    const payload = {
      allowance_amount_sats: Number(amountInput.value),
      min_distance_blocks: Number(distanceInput.value),
      beneficiary_pubkey_hex: document.getElementById("beneficiary-pubkey").value.trim().toLowerCase(),
      fund_amount_sats: Number(document.getElementById("fund-amount").value),
    };
    const result = await api("/contracts", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    status.textContent = "Done.";
    renderCreateResult(result);
    loadBenefactorWallet();
  } catch (err) {
    status.textContent = "Could not create contract: " + err.message;
  } finally {
    submitBtn.disabled = false;
  }
});

function renderCreateResult(result) {
  document.getElementById("create-result").classList.remove("hidden");
  document.getElementById("out-address").textContent = result.address;
  document.getElementById("out-txid").textContent = result.fund_txid;
  document.getElementById("out-txid-link").href = "https://blockstream.info/liquidtestnet/tx/" + result.fund_txid;

  const handoff = { address: result.address, params: result.params };
  document.getElementById("handoff-blob").value = JSON.stringify(handoff, null, 2);
}

document.getElementById("copy-handoff").addEventListener("click", () => {
  const ta = document.getElementById("handoff-blob");
  ta.select();
  navigator.clipboard.writeText(ta.value).catch(() => {});
});

// ---------- beneficiary ----------
let beneficiaryWallet = null;
let loadedContract = null; // {address, params}

async function loadBeneficiaryWallet() {
  const box = document.getElementById("beneficiary-wallet");
  try {
    beneficiaryWallet = await api("/wallet");
    box.innerHTML = walletBoxHtml(beneficiaryWallet);
  } catch (err) {
    box.textContent = "Could not load wallet: " + err.message;
  }
}
loadBeneficiaryWallet();

document.getElementById("copy-beneficiary-pubkey").addEventListener("click", () => {
  if (beneficiaryWallet) navigator.clipboard.writeText(beneficiaryWallet.signing_pubkey).catch(() => {});
});

document.getElementById("load-blob-btn").addEventListener("click", () => {
  const raw = document.getElementById("load-blob").value.trim();
  if (!raw) return;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed.address || !parsed.params) throw new Error("missing 'address' or 'params' field");
    loadedContract = { address: parsed.address, params: parsed.params };
    afterLoad();
  } catch (err) {
    alert("Could not parse that block: " + err.message);
  }
});

document.getElementById("manual-load-form").addEventListener("submit", (e) => {
  e.preventDefault();
  loadedContract = {
    address: document.getElementById("manual-address").value.trim(),
    params: {
      allowance_amount_sats: Number(document.getElementById("manual-amount").value),
      min_distance_blocks: Number(document.getElementById("manual-distance").value),
      beneficiary_pubkey_hex: document.getElementById("manual-pubkey").value.trim().toLowerCase(),
    },
  };
  afterLoad();
});

function afterLoad() {
  document.getElementById("status-section").classList.remove("hidden");
  document.getElementById("status-meta").textContent = "Contract: " + loadedContract.address;

  const warning = document.getElementById("beneficiary-key-warning");
  if (beneficiaryWallet && loadedContract.params.beneficiary_pubkey_hex !== beneficiaryWallet.signing_pubkey) {
    warning.textContent =
      "This contract's beneficiary key doesn't match this installation's own wallet " +
      "key. You can still see its status, but claiming will fail here; the person " +
      "who holds the matching key needs to run their own instance of this app.";
    warning.classList.remove("hidden");
  } else {
    warning.classList.add("hidden");
  }

  refreshStatus();
}

document.getElementById("refresh-status").addEventListener("click", refreshStatus);

async function refreshStatus() {
  if (!loadedContract) return;
  const container = document.getElementById("instances");
  container.textContent = "Loading...";
  try {
    const p = loadedContract.params;
    const qs = new URLSearchParams({
      address: loadedContract.address,
      allowance_amount_sats: p.allowance_amount_sats,
      min_distance_blocks: p.min_distance_blocks,
    });
    const result = await api("/lookup?" + qs.toString());
    renderInstances(result);
  } catch (err) {
    container.textContent = "Could not load status: " + err.message;
  }
}

function renderInstances(result) {
  const container = document.getElementById("instances");
  container.innerHTML = "";
  if (!result.instances.length) {
    container.textContent = "No funds at this address yet (or the funding transaction hasn't confirmed). Tip height: " + result.tip_height;
    return;
  }
  result.instances.forEach((inst) => {
    const card = document.createElement("div");
    card.className = "instance-card";

    let pillClass = "waiting", pillText = "waiting for confirmations";
    if (!inst.confirmed) { pillClass = "unconfirmed"; pillText = "unconfirmed"; }
    else if (inst.withdrawable_now) { pillClass = "ready"; pillText = "ready to claim"; }
    else { pillText = `wait ${inst.blocks_required - inst.blocks_since_confirm} more block(s)`; }

    const left = document.createElement("div");
    left.innerHTML = `
      <div><strong>${inst.value_sats.toLocaleString()} sats</strong>
        ${inst.action === "FullWithdraw" ? "(final withdrawal: empties this instance)" : "(partial: remainder recreates the contract)"}
      </div>
      <div class="meta">UTXO ${inst.txid.slice(0, 10)}...:${inst.vout}
        ${inst.confirmed ? `· confirmed at height ${inst.block_height} (${inst.blocks_since_confirm}/${inst.blocks_required} blocks elapsed)` : "· not yet confirmed"}
      </div>`;

    const right = document.createElement("div");
    const pill = document.createElement("span");
    pill.className = "pill " + pillClass;
    pill.textContent = pillText;
    right.appendChild(pill);

    if (inst.withdrawable_now) {
      const claimBtn = document.createElement("button");
      claimBtn.textContent = "Claim this";
      claimBtn.style.marginLeft = "0.6rem";
      claimBtn.onclick = () => doClaim(inst);
      right.appendChild(claimBtn);
    }

    card.appendChild(left);
    card.appendChild(right);
    container.appendChild(card);
  });
}

async function doClaim(inst) {
  const out = document.getElementById("claim-result");
  out.textContent = "Building, signing, and broadcasting the withdrawal. This takes a few seconds…";
  try {
    const p = loadedContract.params;
    const result = await api("/claim", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        address: loadedContract.address,
        allowance_amount_sats: p.allowance_amount_sats,
        min_distance_blocks: p.min_distance_blocks,
        beneficiary_pubkey_hex: p.beneficiary_pubkey_hex,
        txid: inst.txid,
        vout: inst.vout,
        action: inst.action,
      }),
    });
    out.innerHTML = `<strong>${result.kind === "FullWithdraw" ? "Final" : "Partial"} withdrawal broadcast.</strong><br>
      Txid: <code>${result.txid}</code><br>
      <a href="https://blockstream.info/liquidtestnet/tx/${result.txid}" target="_blank">View on explorer</a>`;
    refreshStatus();
    loadBeneficiaryWallet();
  } catch (err) {
    out.textContent = "Claim failed: " + err.message;
  }
}
