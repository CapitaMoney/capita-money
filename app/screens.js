// SPDX-License-Identifier: AGPL-3.0-only
// The screens of verification, helpers and devices (docs/app-full.md, build
// plan step 5). They are real on a test network (device.js) and say so
// plainly where this device cannot do a step. Words only – no keys,
// addresses or codes are ever shown; QR codes carry them.

import * as D from "./device.js";
import qrcode from "./vendor/qrcode.mjs";
import { scan } from "./scan.js";
import { chipReader, cancelChip } from "./native/chip.js";
import { phoneCheckPossible } from "./native/phone.js";
import { verifyWithDocument } from "./native/verify.js";
import { openCamera, close as closeCamera } from "./native/film.js";

const $ = (sel) => document.querySelector(sel);
const platform = () => globalThis.Capacitor?.getPlatform?.() ?? "web";
const day = (t) => new Date(t).toLocaleDateString("en-GB", { day: "numeric", month: "short" });

function drawQr(el, text) {
  const qr = qrcode(0, "M");
  qr.addData(text);
  qr.make();
  el.innerHTML = qr.createSvgTag({ cellSize: 6, margin: 0, scalable: true });
}

function button(text, primary, onClick) {
  const b = document.createElement("button");
  b.className = "btn" + (primary ? " primary" : "");
  b.textContent = text;
  b.addEventListener("click", onClick);
  return b;
}
function para(text, cls) {
  const p = document.createElement("p");
  if (cls) p.className = cls;
  p.textContent = text;
  return p;
}

/** Words for what went wrong, without codes. */
function plain(e) {
  if (e?.name === "AbortError") return null;
  if (e?.name === "NotAllowedError") return "Not confirmed. Nothing was sent.";
  if (e instanceof TypeError && /fetch/i.test(e.message)) return "No connection to the test network.";
  return e?.message || "Something went wrong.";
}

export function install({ go, toast, getState, onVerified }) {
  // One task at a time runs behind a screen (a check, a wait, the camera);
  // leaving that screen stops it.
  let task = null;
  let running = null;
  const stop = () => { task?.abort(); task = null; running = null; };
  const run = (screen) => { stop(); task = new AbortController(); running = screen; return task.signal; };
  window.addEventListener("hashchange", () => {
    if (!running || location.hash.slice(1) === running) return;
    if (running === "check") cancelChip().catch(() => {});
    stop();
  });

  // ------------------------------------------------------------ verify: which ways this device has
  let target = null; // whom the next document check is for

  async function ways() {
    const box = $("#verify-ways");
    box.replaceChildren();
    if (!D.on()) {
      box.append(
        para("With your passport: your phone reads its chip and films your face – about two minutes. This demo is not connected to a network, so it verifies at a tap."),
        button("Verify (demo)", true, () => {
          if (!onVerified(false)) return toast("Not enough Capita to pay the verification.");
          go("verified", true);
        }),
      );
      return;
    }
    const reader = await chipReader().catch(() => "no");
    const canCheck = reader !== "no" && phoneCheckPossible() && (D.current().checks ?? []).includes("document");
    if (canCheck) {
      box.append(
        para("Your passport's chip and your face, on this phone – about two minutes."),
        button("Use my passport", true, () => { target = { account: D.account(), name: null, box: null }; go("doc"); }),
        button("Use a phone", false, () => go("handover")),
      );
      if (reader === "off") box.append(para("Switch on NFC in the phone's settings first.", "warn"));
    } else {
      box.append(
        para("This device cannot read a passport chip. A phone with Capita – yours or a helper's – does it for this account."),
        button("Use a phone", true, () => go("handover")),
      );
    }
  }

  // ------------------------------------------------------------ the document
  let kind = "passport";
  let useCan = platform() !== "ios"; // iPhones open a chip only with the numbers
  function docFields() {
    document.querySelectorAll("[data-doc]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.doc === kind)));
    const can = kind === "card" && useCan;
    $('[data-fields="mrz"]').hidden = can;
    $('[data-fields="can"]').hidden = !can;
    $("#doc-note").hidden = kind !== "card";
    for (const id of ["#doc-number", "#doc-born", "#doc-expiry"]) $(id).required = !can;
    $("#doc-can").required = can;
  }
  document.querySelectorAll("[data-doc]").forEach((b) => b.addEventListener("click", () => { kind = b.dataset.doc; docFields(); }));
  $("#doc-mrz").addEventListener("click", () => { useCan = false; docFields(); });
  $("#doc-form").addEventListener("submit", (e) => {
    e.preventDefault();
    const can = kind === "card" && useCan;
    const key = can
      ? { can: $("#doc-can").value.trim() }
      : { documentNumber: $("#doc-number").value, birthDate: $("#doc-born").value, expiryDate: $("#doc-expiry").value };
    if (can && !/^\d{6}$/.test(key.can)) return $("#doc-can").focus();
    // The numbers leave the form at once: nothing typed is kept.
    $("#doc-form").reset();
    useCan = platform() !== "ios";
    docFields();
    check(key);
    go("check", true);
  });

  // ------------------------------------------------------------ the check: chip, film, claim
  const say = (text, word) => {
    $("#check-say").textContent = text;
    $("#check-icon").textContent = { chip: "▭", wait: "◎" }[word] ?? "◉";
  };
  function checkView({ camera = false, done = false, again = false, cancel = true } = {}) {
    $("#check-cam").hidden = !camera;
    $("#check-icon").hidden = camera;
    $("#check-done").hidden = !done;
    $("#check-again").hidden = !again;
    $("#check-cancel").hidden = !cancel;
  }
  let lastKey = null;
  async function check(key) {
    lastKey = key;
    const signal = run("check");
    const video = $("#check-video");
    let stream = null;
    const helping = !!target.box;
    $("#check-sub").textContent = helping && target.name ? `For ${target.name}` : "";
    checkView();
    say("One moment …", "wait");
    try {
      const answer = await verifyWithDocument({
        ...D.checkFor(target.account),
        key, video, say,
        camera: async () => { stream = await openCamera(video); checkView({ camera: true }); },
      });
      closeCamera(stream); stream = null;
      if (signal.aborted) return;
      checkView({ cancel: false });
      if (helping) {
        say("Handing over …", "wait");
        await D.put(target.box, "claim", answer);
        say("Done", "ok");
        $("#check-sub").textContent = target.name ? `${target.name}'s Capita finishes by itself.` : "The other device finishes by itself.";
      } else {
        say("Almost there …", "wait");
        await D.present(answer);
        onVerified(true, answer.claim.born);
        // The ledger paid the age at once: say so, and what comes next.
        stop();
        return go("verified", true);
      }
      $("#check-icon").textContent = "✓";
      checkView({ done: true, cancel: false });
    } catch (e) {
      closeCamera(stream);
      const why = plain(e);
      if (!why || signal.aborted) return;
      checkView({ again: true });
      say("Not verified", "no");
      $("#check-sub").textContent = why;
    }
  }
  $("#check-cancel").addEventListener("click", () => { cancelChip().catch(() => {}); stop(); go(target?.box ? "helper" : "home", true); });
  $("#check-again").addEventListener("click", () => go("doc", true));
  $("#check-done").addEventListener("click", () => go("home", true));

  // ------------------------------------------------------------ hand-over: this device waits for a phone
  async function handover() {
    const signal = run("handover");
    const box = D.newBox();
    drawQr($("#handover-qr"), D.verifyCode(box, getState()?.name));
    $("#handover-state").textContent = "Waiting for the phone …";
    try {
      // The code lives ten minutes (ADR 0025).
      const answer = await D.waitFor(() => D.take(box, "claim"), 600, signal);
      if (!answer) { $("#handover-state").textContent = "The code ran out. Go back and try again."; return; }
      $("#handover-state").textContent = "Checked. Confirm on this device …";
      await D.present(answer);
      onVerified(true, answer.claim.born);
      stop();
      go("verified", true);
    } catch (e) {
      const why = plain(e);
      if (why && !signal.aborted) $("#handover-state").textContent = why;
    }
  }

  // ------------------------------------------------------------ scanning a code
  let scanned = null; // what to do with an accepted code
  async function scanner() {
    const signal = run("scan");
    try {
      const text = await scan($("#scan-video"), (t) => !!D.readCode(t), signal);
      const code = D.readCode(text);
      if (!code.sameService) { toast("This code belongs to another test network."); return go("helper", true); }
      scanned?.(code);
    } catch (e) {
      const why = plain(e);
      if (why && !signal.aborted) $("#scan-sub").textContent = e?.name === "NotAllowedError" ? "The camera is needed to read the code." : why;
    }
  }
  function startScan(title, sub, then) {
    $("#scan-title").textContent = title;
    $("#scan-sub").textContent = sub;
    scanned = then;
    go("scan");
  }
  $("#helper-scan").addEventListener("click", () => startScan("Scan their code", "The code on their Verify screen.", (code) => {
    if (code.what === "verify") {
      target = { account: D.fromHexAccount(code.account), name: code.name, box: code.box };
      go("doc", true);
    } else if (code.what === "link") {
      takeLink(code);
    } else {
      toast("That is a payment code. Ask for the code on their Verify screen.");
      go("helper", true);
    }
  }));

  // ------------------------------------------------------------ devices
  async function devices() {
    const ul = $("#devices-list");
    const claimCard = $("#devices-claim");
    const lost = $("#devices-lost");
    try {
      const [keys, claim] = await Promise.all([D.keysOf(), D.homeClaim()]);
      const mine = D.current().key.raw;
      const home = keys[0]?.raw && D.hex(keys[0].raw);
      const iAmHome = home === mine;
      const added = D.current().added ?? {};
      ul.replaceChildren(...(keys.length ? keys : [{ raw: D.keyRaw(), kind: D.current().key.kind }]).map((k, i) => {
        const id = D.hex(k.raw);
        const li = document.createElement("li");
        const left = document.createElement("span");
        const what = id === mine ? (platformName()) : k.kind === "hardware" ? "A phone" : "A laptop or browser";
        left.textContent = what;
        if (i === 0) { const tag = document.createElement("span"); tag.className = "tag"; tag.textContent = "main"; left.append(tag); }
        const when = document.createElement("span");
        when.className = "when";
        when.textContent = added[id] ? `Added ${day(added[id])}` : "";
        left.append(when);
        li.append(left);
        if (keys.length > 1 && (iAmHome || id === mine)) {
          li.append(button(id === mine ? "Leave" : "Remove", false, () => remove(k.raw, id === mine)));
          li.lastChild.classList.add("small");
        }
        return li;
      }));
      $("#devices-add").hidden = keys.length >= 3;
      claimCard.hidden = !claim;
      claimCard.replaceChildren();
      lost.replaceChildren();
      if (claim && iAmHome) {
        claimCard.append(
          para("Another of your devices asked to become the main device", "label"),
          para(`It takes over on ${day((claim.since + D.HOME_DELAY_SECS) * 1000)}. If you did not ask for this, keep this phone as the main one.`),
          button("Keep this phone as main", true, () => act(D.keepHome, "This phone stays the main device.")),
        );
      } else if (claim && D.hex(claim.key.raw) === mine) {
        const due = (claim.since + D.HOME_DELAY_SECS) * 1000;
        claimCard.append(para("This device becomes the main device", "label"));
        if (Date.now() >= due) claimCard.append(button("Become the main device now", true, () => act(D.takeHomeNow, "This is now your main device.")));
        else claimCard.append(para(`On ${day(due)} – unless your main phone says no before.`));
      }
      if (iAmHome) {
        lost.append(para("Lost this phone?", "label"), para(keys.length > 1
          ? "Your other device can become the main one after seven days. Until then, this phone can say no."
          : "Install Capita on the new phone and verify again. Your Capita move over after a week – nothing to write down."));
      } else if (!claim) {
        lost.append(
          para("Lost your main phone?", "label"),
          para("Make this device the main one. It takes seven days; if the phone is still in use, it can say no."),
          button("Make this the main device", false, () => act(D.askHome, "Asked. This device becomes the main one in seven days.")),
        );
      }
    } catch (e) {
      ul.replaceChildren(para(plain(e) ?? "", "empty"));
    }
  }
  const platformName = () => (platform() === "web" ? "This device" : "This phone");
  async function act(fn, done) {
    try { await fn(); toast(done); } catch (e) { const w = plain(e); if (w) toast(w); }
    devices();
  }
  async function remove(raw, self) {
    try {
      const gone = await D.removeKey(raw);
      toast(gone ? (self ? "This device left your account." : "Removed.") : "The network did not remove it.");
      if (gone && self) { await D.forget(); location.reload(); return; }
    } catch (e) { const w = plain(e); if (w) toast(w); }
    devices();
  }
  $("#devices-add").addEventListener("click", () => startScan("Scan the new device",
    "On the new device: open Capita, choose “I already use Capita on another device”.", (code) => {
      if (code.what !== "link") { toast("That is not the code of a new device."); return go("devices", true); }
      takeLink(code);
    }));

  // The phone's side of linking: hand the account over, wait for the key, add it.
  let handed = null;
  async function takeLink(code) {
    go("adddevice", true);
    const signal = run("adddevice");
    handed = null;
    $("#add-go").disabled = true;
    $("#add-what").textContent = "It will pay from your account, like this phone.";
    $("#add-state").textContent = "Confirm on the new device …";
    try {
      await D.put(code.box, "account", { account: D.current().account, name: getState()?.name ?? "" });
      handed = await D.waitFor(() => D.take(code.box, "key"), 300, signal);
      if (!handed) { $("#add-state").textContent = "The new device did not answer. Try again."; return; }
      $("#add-what").textContent = handed.key.startsWith("02")
        ? "A phone will pay from your account, like this one."
        : "A laptop or browser will pay from your account, like this phone.";
      $("#add-state").textContent = "";
      $("#add-go").disabled = false;
    } catch (e) {
      const why = plain(e);
      if (why && !signal.aborted) $("#add-state").textContent = why;
    }
  }
  $("#add-go").addEventListener("click", async () => {
    $("#add-go").disabled = true;
    $("#add-state").textContent = "Adding …";
    try {
      await D.addKey(handed);
      toast("Added. The new device is ready.");
      go("devices", true);
    } catch (e) {
      $("#add-state").textContent = plain(e) ?? "";
      $("#add-go").disabled = false;
    }
  });

  // The new device's side: show a code, wait for the account, sign, wait for the chain.
  async function link() {
    if (!D.on()) { toast("Connect to the test network first."); return go("network", true); }
    const signal = run("link");
    const box = D.newBox();
    drawQr($("#link-qr"), D.linkCode(box));
    $("#link-state").textContent = "Waiting for your phone …";
    try {
      const got = await D.waitFor(() => D.take(box, "account"), 600, signal);
      if (!got) { $("#link-state").textContent = "The code ran out. Go back and try again."; return; }
      $("#link-state").textContent = "Confirm on this device …";
      if (!D.hasKey() || D.current().opened) await D.makeKey(got.name, D.fromHexAccount(got.account));
      await D.handKey(box, got.account);
      $("#link-state").textContent = "Now confirm on your phone …";
      const mine = D.keyRaw();
      const ok = await D.waitFor(async () => (await D.ownerOf(mine)) === got.account, 300, signal);
      if (!ok) { $("#link-state").textContent = "Your phone did not add this device. Try again."; return; }
      D.joined(got.account);
      onLinked(got.name);
    } catch (e) {
      const why = plain(e);
      if (why && !signal.aborted) $("#link-state").textContent = why;
    }
  }
  let onLinked = () => {};

  // ------------------------------------------------------------ test network
  function network() {
    const c = D.current();
    if (D.on() || !$("#net-node").value) {
      $("#net-node").value = c?.node ?? "";
      $("#net-service").value = c?.service?.replace(/\/$/, "") ?? "";
    }
    $("#net-state").textContent = D.on() ? "Connected." : "";
    $("#net-leave").hidden = !D.on();
  }
  $("#net-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    $("#net-state").textContent = "Connecting …";
    try {
      await D.connect($("#net-node").value, $("#net-service").value);
      const s = getState();
      if (s && !D.hasKey()) {
        $("#net-state").textContent = "Making this device's key …";
        await D.makeKey(s.name);
        await D.ensureOpen().catch(() => {});
      }
      toast("Connected to the test network.");
      history.length > 1 ? history.back() : go("home");
    } catch (err) {
      $("#net-state").textContent = plain(err) ?? "";
    }
  });
  $("#net-leave").addEventListener("click", async () => {
    await D.forget();
    toast("Left the test network. This device's key is gone.");
    network();
  });

  // Screens that act on the chain need the test network and this device's key.
  const needNet = (fn) => (...args) => {
    if (D.on() && D.hasKey()) return fn(...args);
    toast(D.on() ? "Open your account first." : "Connect to the test network first.");
    go(D.on() ? "home" : "network", true);
  };

  return {
    render: {
      verify: ways,
      doc: needNet(() => {
        if (!target) target = { account: D.account(), name: null, box: null };
        $("#doc-title").textContent = target.box ? "Their document" : "Your document";
        $("#doc-for").hidden = !target.name;
        $("#doc-for").textContent = target.name ? `Verifying ${target.name}` : "";
        docFields();
      }),
      handover: needNet(handover),
      check() { if (running !== "check") go("home", true); },
      adddevice() { if (running !== "adddevice") go("devices", true); },
      helper() {
        $("#helper-note").textContent = phoneCheckPossible() ? "" : "This device cannot read passports: helpers need the Capita app on a phone with NFC.";
        $("#helper-scan").disabled = !phoneCheckPossible() || !D.on();
      },
      scan: needNet(scanner),
      devices: needNet(devices),
      link,
      network,
    },
    /** Called when a new device has joined an account. */
    set onLinked(fn) { onLinked = fn; },
  };
}
