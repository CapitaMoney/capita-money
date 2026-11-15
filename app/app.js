// SPDX-License-Identifier: AGPL-3.0-only
// Capita prototype: screens, navigation, storage. Rules live in logic.js.
// Battery: no timers, no polling. Money is collected when the app comes to the screen.
import * as L from "./logic.js";
import qrcode from "./vendor/qrcode.mjs";
import * as D from "./device.js";
import { install } from "./screens.js";

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
  const before = ["welcome", "name", "link"]; // only without an account
  if (!state && !before.includes(want) && want !== "network") return go("welcome", true);
  if (state && before.includes(want)) return go("home", true);
  if (!document.querySelector(`[data-screen="${want}"]`)) return go(state ? "home" : "welcome", true);
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
  state = L.newAccount(name, now());
  save();
  go("gift", true);
  // On a test network the device makes its key now and opens the account.
  if (D.on() && !D.hasKey()) {
    D.makeKey(name).then(() => D.ensureOpen()).catch(() => toast("Not connected yet – Capita tries again when you verify."));
  }
});

// ------------------------------------------------------------ screens
const render = {
  welcome() {
    $("#welcome-link").hidden = !D.on();
  },
  gift() {
    if (!state) return go("welcome", true);
    $("#gift-hello").textContent = `Welcome, ${state.name}`;
  },
  home() {
    collectNow();
    const t = now();
    $("#balance").textContent = L.formatCents(state.balanceCents);
    const card = $("#next-card");
    if (!L.isVerified(state, t)) {
      $("#balance-sub").textContent = state.reservedCents > 0
        ? (state.reservedCents === state.balanceCents ? "Kept for your verification" : `${L.formatCents(state.reservedCents)} of it kept for your verification`)
        : "";
      const renew = state.verifiedAt !== null;
      card.innerHTML = `<p class="title">${renew ? "Verify again" : "Receive your age in Capita"}</p>
        <p>${renew ? "Your year is over. Verify again to receive Capita on your next birthday." : "Verify once – about two minutes. Your welcome gift pays for it. Then every birthday brings twice your age."}</p>
        <button class="btn primary" data-go="verify">Verify</button>`;
    } else {
      $("#balance-sub").textContent = "";
      const next = L.nextBirthday(state, t);
      // Built from text nodes: nothing of a person's data is ever read as markup.
      const title = document.createElement("p");
      title.className = "title";
      const text = document.createElement("p");
      if (next) {
        title.textContent = `${L.formatCents(next.cents)} Capita on your birthday`;
        text.textContent = `Twice your age, all at once, on ${birthdayText(next.t)}. Open Capita within two weeks of that day to receive it.`;
      } else {
        title.textContent = "You are verified";
        text.textContent = "Capita arrive on your birthdays once your date of birth is known: verify with your passport.";
      }
      card.replaceChildren(title, text);
    }
    list($("#recent"), state.history.slice(-4).reverse());
  },
  verify() {
    screens.render.verify();
  },
  // The moment after a verification: the age in Capita, what comes on
  // birthdays, and the ask to allow notifications (rules diary 4).
  verified() {
    const t = now();
    if (!L.isVerified(state, t)) return go("home", true);
    const first = [...state.history].reverse().find((h) => h.kind === "age" && h.t === state.verifiedAt);
    $("#verified-amount").hidden = !first;
    $("#verified-cents").textContent = L.formatCents(first ? first.cents : 0);
    $("#verified-lead").textContent = first ? "Your age in Capita. It is yours now." : "You are verified for a year.";
    const next = L.nextBirthday(state, t);
    $("#verified-next").textContent = next
      ? `From now on every birthday brings twice your age, all at once – next: ${L.formatCents(next.cents)} Capita on ${birthdayText(next.t)}.`
      : "";
    const ask = canAskNotifications();
    $("#verified-notify").hidden = !ask;
    $("#verified-notify-note").hidden = !ask;
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
    $("#settings-net").hidden = !D.on();
    $("#settings-lost").hidden = D.on();
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

/** A birthday as a day: it begins at 00:00 UTC, so the day is read in UTC. */
function birthdayText(t) {
  return new Date(t).toLocaleDateString("en-GB", { day: "numeric", month: "long", timeZone: "UTC" });
}

// ------------------------------------------------------------ notifications
// Asked once, right after a verification, and only by a tap (rules diary 4).
// Not built: the reminder itself – nothing is sent yet.
function canAskNotifications() {
  return typeof Notification !== "undefined" && Notification.permission === "default";
}
$("#verified-notify").addEventListener("click", async () => {
  try { await Notification.requestPermission(); } catch { /* the device does not offer them */ }
  render.verified();
});

// ------------------------------------------------------------ collecting
function collectNow() {
  if (!state) return;
  const r = L.collect(state, now());
  if (r.state === state) return;
  state = r.state;
  save();
  if (r.lapsed > 0) toast(`${L.formatCents(r.lapsed)} birthday Capita lapsed – open Capita within two weeks of your birthday.`);
  if (r.collected > 0) toast(`Happy birthday! +${L.formatCents(r.collected)} Capita`);
}
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && state && location.hash === "#home") render.home();
});

// ------------------------------------------------------------ verification, helpers, devices (screens.js)
// The demo's money follows a real verification: the start grant pays for it.
// `born`: the date of birth in the identity service's claim; the demo
// without a network has none and takes a person of 30.
function onVerified(real, born = null) {
  const t = now();
  try {
    state = L.verify(state, t, real ? born : L.demoBorn(t));
  } catch {
    if (!real) return false;
    state = L.verified(state, t, born);
  }
  save();
  return true;
}
D.load();
const screens = install({ go, toast, getState: () => state, onVerified });
screens.onLinked = (name) => {
  state = L.newAccount(name || "Me", now());
  save();
  toast("This device joined your account.");
  go("home", true);
};
for (const [name, fn] of Object.entries(screens.render)) if (!render[name]) render[name] = fn;

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
// A link with ?node=…&service=… fills in the test network (for testers).
{
  const q = new URLSearchParams(location.search);
  if (q.get("node") && q.get("service") && !D.on()) {
    $("#net-node").value = q.get("node");
    $("#net-service").value = q.get("service");
    history.replaceState(null, "", location.pathname + "#network");
  }
}

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
