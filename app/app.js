// SPDX-License-Identifier: AGPL-3.0-only
// Capita prototype: screens, navigation, storage.
// On a test network the money is the ledger's (ADR 0027): balance, payments
// and birthdays come from the chain, every payment is signed by this
// device's key. Without one the app is a demo; its rules live in logic.js.
// Battery: no timers, no polling – money is read and collected when the app
// comes to the screen. One exception: the Receive screen looks for the
// payment it waits for, while it is open.
import * as L from "./logic.js";
import * as M from "./money.js";
import qrcode from "./vendor/qrcode.mjs";
import * as D from "./device.js";
import { install, plain } from "./screens.js";

const KEY = "capita-proto-v1";
const BOOK = "capita-money-v1";
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
D.load();
/** Money on the ledger: this device is on a test network and has its key. */
const real = () => D.on() && D.hasKey();
// The demo can jump ahead in time; the ledger's time is the world's.
const now = () => Date.now() + (real() ? 0 : Number(store.get(OFFSET) || 0));
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
  watching?.abort();
  watching = null;
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
    D.makeKey(name).then(() => D.ensureOpen()).then(() => { if (onHome()) refreshHome(); })
      .catch(() => toast("Not connected yet – Capita tries again when you verify."));
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
    if (real()) refreshHome(); else collectNow();
    paintHome();
  },
  verify() {
    screens.render.verify();
  },
  // The moment after a verification: the age in Capita, what comes on
  // birthdays, and the ask to allow notifications (rules diary 4).
  verified() {
    const f = face();
    if (!f.verified) return go("home", true);
    const first = firstCents > 0;
    $("#verified-amount").hidden = !first;
    $("#verified-cents").textContent = L.formatCents(firstCents);
    $("#verified-lead").textContent = first ? "Your age in Capita. It is yours now." : "You are verified for a year.";
    const next = f.next;
    $("#verified-next").textContent = next
      ? `From now on every birthday brings twice your age, all at once – next: ${L.formatCents(next.cents)} Capita on ${birthdayText(next.t)}.`
      : "";
    const ask = canAskNotifications();
    $("#verified-notify").hidden = !ask;
    $("#verified-notify-note").hidden = !ask;
  },
  receive() {
    $("#receive-name").textContent = state.name;
    $("#receive-state").textContent = "";
    drawQr();
    if (real()) watchReceive();
  },
  send() {
    // On the ledger: the people this device paid before. Demo: made-up people.
    const recent = [...new Set(state.history.filter((h) => h.who).map((h) => h.who).reverse())];
    const people = real()
      ? theBook().people
      : [...recent, ...DEMO_PEOPLE.filter((p) => !recent.includes(p))].slice(0, 6).map((name) => ({ name }));
    const ul = $("#people");
    if (!people.length) {
      const li = document.createElement("li");
      li.className = "empty";
      li.textContent = "People you pay appear here.";
      return ul.replaceChildren(li);
    }
    ul.replaceChildren(...people.map((p) => {
      const li = document.createElement("li");
      li.textContent = p.name;
      li.tabIndex = 0;
      li.addEventListener("click", () => pickPerson(p));
      li.addEventListener("keydown", (e) => { if (e.key === "Enter") pickPerson(p); });
      return li;
    }));
  },
  amount() {
    if (!sendTo) return go("send", true);
    $("#amount-to").textContent = `To ${sendTo.name}`;
    entry = asked ? (asked / 100).toFixed(2) : "";
    asked = null;
    showEntry();
    // What may be sent is the ledger's word, read afresh.
    if (real()) sync().then(() => { if (location.hash === "#amount") showEntry(); }).catch(() => {});
  },
  history() {
    list($("#all"), [...face().history].reverse());
  },
  settings() {
    $("#settings-net").hidden = !D.on();
    $("#settings-lost").hidden = D.on();
    $("#settings-demo").hidden = real();
    $("#rename").value = state.name;
    const d = real() ? M.daysLeft(view(), now()) : L.daysLeft(state, now());
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

// ------------------------------------------------------------ what the screens show
// One face for both kinds of money: the ledger's on a test network, else the demo's.
function face() {
  const t = now();
  if (!real()) {
    return {
      cents: state.balanceCents, reserved: state.reservedCents, spendable: L.spendableCents(state),
      verified: L.isVerified(state, t), renew: state.verifiedAt !== null, next: L.nextBirthday(state, t),
      history: state.history, due: 0, note: "",
    };
  }
  const v = view();
  return {
    cents: v?.cents ?? 0, reserved: v?.account?.reservedCents ?? 0, spendable: M.spendableCents(v),
    verified: M.isVerified(v, t), renew: M.wasVerified(v), next: M.nextBirthday(v, t),
    history: theBook().history, due: waiting ? M.claimableCents(v, t - CLOCK_SLACK) : 0,
    note: note || M.statusNote(v) || (v && !v.account ? "Capita opens your account when you verify." : ""),
  };
}

function paintHome() {
  const f = face();
  $("#balance").textContent = L.formatCents(f.cents);
  const card = $("#next-card");
  // On the ledger the welcome gift stays earmarked after the verification,
  // until the identity service is paid from it (not built yet, ADR 0027).
  const kept = f.reserved <= 0 ? ""
    : f.verified ? `${L.formatCents(f.reserved)} of it pays for your verification`
    : f.reserved === f.cents ? "Kept for your verification" : `${L.formatCents(f.reserved)} of it kept for your verification`;
  $("#balance-sub").textContent = f.note || kept;
  // Built from text nodes: nothing of a person's data is ever read as markup.
  const title = document.createElement("p");
  title.className = "title";
  const text = document.createElement("p");
  const parts = [title, text];
  if (!f.verified) {
    title.textContent = f.renew ? "Verify again" : "Receive your age in Capita";
    text.textContent = f.renew
      ? "Your year is over. Verify again to receive Capita on your next birthday."
      : "Verify once – about two minutes. Your welcome gift pays for it. Then every birthday brings twice your age.";
    const b = document.createElement("button");
    b.className = "btn primary";
    b.dataset.go = "verify";
    b.textContent = "Verify";
    parts.push(b);
  } else if (f.due > 0) {
    // The birthday Capita wait for a confirmation with this device's key.
    title.textContent = `${L.formatCents(f.due)} Capita for your birthday`;
    text.textContent = "Happy birthday! Confirm on this device to receive them.";
    const b = document.createElement("button");
    b.className = "btn primary";
    b.textContent = "Receive";
    b.addEventListener("click", async () => { b.disabled = true; await birthdays(true); paintHome(); });
    parts.push(b);
  } else if (f.next) {
    title.textContent = `${L.formatCents(f.next.cents)} Capita on your birthday`;
    text.textContent = `Twice your age, all at once, on ${birthdayText(f.next.t)}. Open Capita within two weeks of that day to receive it.`;
  } else {
    title.textContent = "You are verified";
    text.textContent = "Capita arrive on your birthdays once your date of birth is known: verify with your passport.";
  }
  card.replaceChildren(...parts);
  list($("#recent"), f.history.slice(-4).reverse());
}

// ------------------------------------------------------------ money on the ledger (ADR 0027)
// The book of this device: what it last read from the ledger and the lines
// of its history (money.js). The ledger is asked when a screen needs it.
let book = null;
let note = ""; // said under the balance when the ledger cannot be reached
let waiting = false; // birthday Capita wait for a tap (the confirmation did not come by itself)
let quietUntil = 0; // after a claim that brought nothing: do not ask again at once
let busy = false;
let firstCents = 0; // what the verification just made brought, for the Verified screen
/** The ledger's clock follows its blocks: a birthday counts a minute after it began. */
const CLOCK_SLACK = 60000;

function theBook() {
  const account = D.current().account;
  if (book?.account === account) return book;
  try {
    const saved = JSON.parse(store.get(BOOK));
    book = saved && saved.v === 1 && saved.account === account ? saved : M.newBook(account);
  } catch { book = M.newBook(account); }
  return book;
}
function keepBook(next) { book = next; store.set(BOOK, JSON.stringify(book)); }
const view = () => theBook().view;

/** Reads the ledger and writes down what changed; `as` names what this device just did. */
async function sync(as = null) {
  const seen = await D.money();
  keepBook(M.settle(theBook(), seen, Date.now(), as));
  return seen;
}

const onHome = () => location.hash === "#home";
async function refreshHome() {
  if (busy) return;
  busy = true;
  try {
    await sync();
    note = "";
    if (onHome()) paintHome();
    await birthdays(false);
  } catch (e) {
    note = plain(e) ? "No connection – this is what Capita saw last." : "";
  } finally {
    busy = false;
    if (onHome()) paintHome();
  }
}

/**
 * Claims the birthdays that fell due – by itself when the app comes to the
 * screen. The device's key signs: a phone asks for fingerprint or face. If
 * that is not given (or the browser wants a tap first), the home screen
 * offers it.
 */
async function birthdays(byTap) {
  const due = M.claimableCents(view(), Date.now() - CLOCK_SLACK);
  if (due <= 0) { waiting = false; return; }
  if (!byTap && (waiting || Date.now() < quietUntil)) return;
  try {
    const before = theBook().cents ?? 0;
    const after = await D.claimDue("Receive your birthday Capita");
    waiting = false;
    if (!after) { quietUntil = Date.now() + 3600000; return; }
    keepBook(M.settle(theBook(), after, Date.now(), { kind: "birthday" }));
    toast(`Happy birthday! +${L.formatCents(after.cents - before)} Capita`);
  } catch (e) {
    waiting = true;
    const why = plain(e);
    if (byTap && why) toast(why);
  }
}

// ------------------------------------------------------------ collecting (demo)
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
// On a test network the ledger pays the age in the transaction that takes
// the claim: the book notes it. The demo verifies at a tap, a person of 30,
// and its start grant pays.
async function onVerified(onLedger) {
  if (onLedger) {
    const before = theBook().cents;
    const seen = await sync({ kind: "age" });
    firstCents = Math.max(0, seen.cents - (before ?? seen.account?.reservedCents ?? 0));
    return true;
  }
  const t = now();
  try { state = L.verify(state, t, L.demoBorn(t)); } catch { return false; }
  save();
  firstCents = [...state.history].reverse().find((h) => h.kind === "age" && h.t === state.verifiedAt)?.cents ?? 0;
  return true;
}
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
  qr.addData(real() ? D.payCode(state.name, cents) : `capita:demo/${encodeURIComponent(state.name)}${cents ? `?amount=${cents}` : ""}`);
  qr.make();
  $("#qr").innerHTML = qr.createSvgTag({ cellSize: 6, margin: 0, scalable: true });
}
$("#receive-amount").addEventListener("input", drawQr);

// While the code is shown, the screen looks for the payment (every 3 s, at
// most 5 minutes) and says when it has come. Leaving the screen stops it.
let watching = null;
async function watchReceive() {
  const signal = (watching = new AbortController()).signal;
  try {
    await sync();
    const got = await D.waitFor(async () => {
      const before = theBook().cents ?? 0;
      const seen = await sync();
      return seen.cents > before ? seen.cents - before : null;
    }, 300, signal, 3000);
    if (got) $("#receive-state").textContent = `✓ ${L.formatCents(got)} Capita received`;
  } catch { /* left the screen, or no connection: the home screen reads again */ }
}

// ------------------------------------------------------------ send
let sendTo = null; // { name, account } – the demo's people have no account
let asked = null; // cents a scanned code asks for
let entry = "";
function pickPerson(p) { sendTo = p; asked = null; go("amount"); }
$("#scan").addEventListener("click", () => {
  if (!real()) return toast("Here the camera opens. Demo: choose a person below.");
  screens.scanToPay((code) => {
    if (code.account === D.current().account) { toast("That is your own code."); return go("send", true); }
    sendTo = { name: L.cleanName(code.name) || "Someone", account: code.account };
    asked = code.cents;
    go("amount", true);
  });
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
  const can = face().spendable;
  const ok = cents > 0 && cents <= can;
  $("#send-go").disabled = !ok;
  $("#send-go").textContent = cents > 0 ? `Send ${L.formatCents(cents)} Capita` : "Send";
  $("#amount-hint").textContent = cents > can
    ? `You have ${L.formatCents(can)} to send.`
    : `Available: ${L.formatCents(can)}`;
}
$("#send-go").addEventListener("click", async () => {
  const cents = L.parseAmount(entry);
  const to = sendTo;
  if (!real()) {
    try {
      state = L.send(state, to.name, cents, now());
      save();
    } catch {
      return showEntry();
    }
  } else {
    // The ledger makes the payment; this device's key signs it (a phone
    // asks for fingerprint or face, and names amount and person).
    const amount = L.formatCents(cents);
    $("#send-go").disabled = true;
    $("#send-go").textContent = "Sending …";
    try {
      if (!(await D.canReceive(to.account))) throw new Error(`${to.name} has no Capita account on this network.`);
      await sync();
      const after = await D.pay(to.account, cents, `Send ${amount} Capita to ${to.name}`);
      keepBook(M.remember(M.settle(theBook(), after, Date.now(), { kind: "out", who: to.name }), to.name, to.account));
    } catch (e) {
      showEntry();
      const why = plain(e);
      if (why) $("#amount-hint").textContent = why;
      return;
    }
  }
  $("#sent-text").textContent = `${L.formatCents(cents)} Capita to ${to.name}`;
  sendTo = null;
  go("sent", true);
});

// ------------------------------------------------------------ settings + demo
$("#rename").addEventListener("change", (e) => {
  const n = L.cleanName(e.target.value);
  if (n) { state = { ...state, name: n }; save(); toast("Name changed"); } else e.target.value = state.name;
});
$("#demo-pay").addEventListener("click", () => {
  if (real()) return;
  const who = DEMO_PEOPLE[state.history.length % DEMO_PEOPLE.length];
  state = L.receive(state, who, 200, now());
  save();
  toast(`${who} sent you 2.00 Capita`);
});
$("#demo-week").addEventListener("click", () => {
  if (real()) return;
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
