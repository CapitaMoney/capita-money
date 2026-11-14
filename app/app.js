// SPDX-License-Identifier: AGPL-3.0-only
// Capita prototype: screens, navigation, storage. Rules live in logic.js.
// Battery: no timers, no polling. Money is collected when the app comes to the screen.
import * as L from "./logic.js";
import qrcode from "./vendor/qrcode.mjs";

const KEY = "capita-proto-v1";
const OFFSET = "capita-proto-offset";
const $ = (sel) => document.querySelector(sel);
const DEMO_PEOPLE = ["Anna", "Ben", "Chiara", "Dev", "Ewa"];

// ------------------------------------------------------------ storage
// Browser storage can be missing or throw (private mode, some hosts): fall back to memory.
const mem = {};
const store = {
  get(k) { try { return localStorage.getItem(k); } catch { return mem[k] ?? null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { mem[k] = v; } },
  del(k) { try { localStorage.removeItem(k); } catch { delete mem[k]; } },
};
const now = () => Date.now() + Number(store.get(OFFSET) || 0);
let state = load();
function load() {
  try { const s = JSON.parse(store.get(KEY)); return s && s.v === 1 ? s : null; } catch { return null; }
}
function save() { store.set(KEY, JSON.stringify(state)); }

// ------------------------------------------------------------ navigation
// Hash routes, so the phone's back button works.
function show(name) {
  document.querySelectorAll(".screen").forEach((el) => el.classList.toggle("on", el.dataset.screen === name));
  render[name]?.();
  window.scrollTo(0, 0);
  const first = document.querySelector(`.screen.on input`);
  if (first && name === "name") setTimeout(() => first.focus(), 250);
}
function go(name, replace = false) {
  if (replace) location.replace(`#${name}`); else location.hash = name;
}
function route() {
  const want = location.hash.slice(1) || (state ? "home" : "welcome");
  const open = ["welcome", "name"];
  if (!state && !open.includes(want)) return go("welcome", true);
  if (state && open.includes(want)) return go("home", true);
  show(want);
}
window.addEventListener("hashchange", route);
document.addEventListener("click", (e) => {
  const t = e.target.closest("[data-go],[data-back]");
  if (!t) return;
  if (t.hasAttribute("data-back")) history.length > 1 ? history.back() : go("home");
  else go(t.dataset.go);
});

// ------------------------------------------------------------ first minute
$("#name-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const name = L.cleanName($("#name-input").value);
  if (!name) return $("#name-input").focus();
  // Here the real app creates the key on the device, silently.
  state = L.newAccount(name, now());
  save();
  go("gift", true);
});

// ------------------------------------------------------------ screens
const render = {
  gift() {
    if (!state) return go("welcome", true);
    $("#gift-hello").textContent = `Welcome, ${state.name}`;
  },
  home() {
    collectNow();
    const t = now();
    $("#balance").textContent = L.formatCents(state.balanceCents);
    const card = $("#next-card");
    const rate = L.formatCents(L.RULES.verifiedMonthlyCents);
    if (!L.isVerified(state, t)) {
      $("#balance-sub").textContent = state.reservedCents > 0
        ? (state.reservedCents === state.balanceCents ? "Kept for your verification" : `${L.formatCents(state.reservedCents)} of it kept for your verification`)
        : "";
      const renew = state.verifiedAt !== null;
      card.innerHTML = `<p class="title">${renew ? "Verify again" : `Receive ${rate} Capita every month`}</p>
        <p>${renew ? "Your year is over. Verify again to keep receiving Capita." : "Verify once – about two minutes. Your welcome gift pays for it."}</p>
        <button class="btn primary" data-go="verify">Verify</button>`;
    } else {
      $("#balance-sub").textContent = "";
      card.innerHTML = `<p class="title">You receive ${rate} Capita a month</p>
        <p>It arrives by itself whenever you open Capita.</p>`;
    }
    list($("#recent"), state.history.slice(-4).reverse());
  },
  verify() {
    $("#verify-rate").textContent = L.formatCents(L.RULES.verifiedMonthlyCents);
  },
  receive() {
    $("#receive-name").textContent = state.name;
    drawQr();
  },
  send() {
    const recent = [...new Set(state.history.filter((h) => h.who).map((h) => h.who).reverse())];
    const people = [...recent, ...DEMO_PEOPLE.filter((p) => !recent.includes(p))].slice(0, 6);
    const ul = $("#people");
    ul.replaceChildren(...people.map((p) => {
      const li = document.createElement("li");
      li.textContent = p;
      li.tabIndex = 0;
      li.addEventListener("click", () => pickPerson(p));
      li.addEventListener("keydown", (e) => { if (e.key === "Enter") pickPerson(p); });
      return li;
    }));
  },
  amount() {
    if (!sendTo) return go("send", true);
    $("#amount-to").textContent = `To ${sendTo}`;
    entry = "";
    showEntry();
  },
  history() {
    list($("#all"), [...state.history].reverse());
  },
  settings() {
    $("#rename").value = state.name;
    const d = L.daysLeft(state, now());
    $("#verified-until").textContent = d === null ? "Not verified yet." : d > 0 ? `Verified – ${d} days left this year.` : "Verification has run out.";
  },
};

function list(ul, items) {
  if (!items.length) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = "Nothing yet";
    return ul.replaceChildren(li);
  }
  ul.replaceChildren(...items.map((h) => {
    const li = document.createElement("li");
    const left = document.createElement("span");
    left.textContent = L.describe(h);
    const when = document.createElement("span");
    when.className = "when";
    when.textContent = day(h.t);
    left.append(when);
    const c = document.createElement("span");
    c.className = "c" + (h.cents > 0 ? " plus" : "");
    c.textContent = (h.cents > 0 ? "+" : "") + L.formatCents(h.cents);
    li.append(left, c);
    return li;
  }));
}
function day(t) {
  const n = now();
  if (L.sameDay(t, n)) return "Today";
  if (L.sameDay(t, n - 86400000)) return "Yesterday";
  return new Date(t).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

// ------------------------------------------------------------ collecting
function collectNow() {
  if (!state) return;
  const r = L.collect(state, now());
  if (r.state === state) return;
  state = r.state;
  save();
  if (r.lapsed > 0) toast(`+${L.formatCents(r.collected)} Capita. ${L.formatCents(r.lapsed)} lapsed – open Capita at least every two weeks.`);
  else if (r.collected >= 1) toast(`+${L.formatCents(r.collected)} Capita arrived`);
}
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && state && location.hash === "#home") render.home();
});

$("#verify-go").addEventListener("click", () => {
  try {
    state = L.verify(state, now());
    save();
    toast("Verified. Capita now arrives every month.");
    go("home", true);
  } catch {
    toast("Not enough Capita to pay the verification.");
  }
});

// ------------------------------------------------------------ receive
function drawQr() {
  const cents = L.parseAmount($("#receive-amount").value);
  // The code carries the address; the screen never shows it.
  const qr = qrcode(0, "M");
  qr.addData(`capita:demo/${encodeURIComponent(state.name)}${cents ? `?amount=${cents}` : ""}`);
  qr.make();
  $("#qr").innerHTML = qr.createSvgTag({ cellSize: 6, margin: 0, scalable: true });
}
$("#receive-amount").addEventListener("input", drawQr);

// ------------------------------------------------------------ send
let sendTo = null;
let entry = "";
function pickPerson(p) { sendTo = p; go("amount"); }
$("#scan").addEventListener("click", () => {
  toast("Here the camera opens. Demo: choose a person below.");
});
$("#keys").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  const k = b.textContent;
  if (k === "⌫") entry = entry.slice(0, -1);
  else if (k === ".") { if (!entry.includes(".")) entry = (entry || "0") + "."; }
  else if (/^\d$/.test(k)) {
    const [, dec] = entry.split(".");
    if (dec !== undefined && dec.length >= 2) return;
    if (entry === "0") entry = "";
    if (entry.replace(".", "").length >= 7) return;
    entry += k;
  }
  showEntry();
});
function showEntry() {
  $("#amount-show").textContent = entry || "0";
  const cents = L.parseAmount(entry || "0") ?? 0;
  const can = L.spendableCents(state);
  const ok = cents > 0 && cents <= can;
  $("#send-go").disabled = !ok;
  $("#send-go").textContent = cents > 0 ? `Send ${L.formatCents(cents)} Capita` : "Send";
  $("#amount-hint").textContent = cents > can
    ? `You have ${L.formatCents(can)} to send.`
    : `Available: ${L.formatCents(can)}`;
}
$("#send-go").addEventListener("click", () => {
  const cents = L.parseAmount(entry);
  try {
    state = L.send(state, sendTo, cents, now());
    save();
    $("#sent-text").textContent = `${L.formatCents(cents)} Capita to ${sendTo}`;
    sendTo = null;
    go("sent", true);
  } catch {
    showEntry();
  }
});

// ------------------------------------------------------------ settings + demo
$("#rename").addEventListener("change", (e) => {
  const n = L.cleanName(e.target.value);
  if (n) { state = { ...state, name: n }; save(); toast("Name changed"); } else e.target.value = state.name;
});
$("#demo-pay").addEventListener("click", () => {
  const who = DEMO_PEOPLE[state.history.length % DEMO_PEOPLE.length];
  state = L.receive(state, who, 200, now());
  save();
  toast(`${who} sent you 2.00 Capita`);
});
$("#demo-week").addEventListener("click", () => {
  store.set(OFFSET, String(Number(store.get(OFFSET) || 0) + 7 * 86400000));
  go("home");
});
$("#demo-reset").addEventListener("click", () => {
  store.del(KEY); store.del(OFFSET); state = null;
  go("welcome", true);
});

// ------------------------------------------------------------ toast
let toastTimer = 0;
function toast(text) {
  const t = $("#toast");
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 3200);
}

// ------------------------------------------------------------ start
route();
if ("serviceWorker" in navigator && location.protocol === "https:") {
  navigator.serviceWorker.register("sw.js").catch(() => {});
}
