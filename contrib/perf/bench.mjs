// Drive the real Tauri WebView against the seeded local tuwunel and time daily operations.
// Usage: PATH=/tmp/koushi-desktop-local-qa-bin:$HOME/.cargo/bin:$PATH \
//   node contrib/perf/bench.mjs <seedDir> <appDataDir> [--fresh] [--only=switch,paginate,send,space]
// See README.md in this directory. This is a measurement tool, not a gate.
import { mkdirSync, readFileSync, rmSync, writeFileSync, appendFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const mod = (path) => import(pathToFileURL(join(REPO, path)).href);
const Q = "scripts/desktop-linux-gui-qa";
const { startHomeserver, stopProcess, waitForHomeserver, freePort } = await mod("scripts/lib/local-homeserver-qa.mjs");
const { childEnvironment } = await mod(`${Q}/redaction.mjs`);
const { ensureDbusSession, startXvfb, spawnLogged, terminateProcessGroup, settleChild, waitForPort, sleep } = await mod(`${Q}/runtime.mjs`);
const { importDesktopWebdriverio, webdriverCapabilities, safeDeleteSession } = await mod(`${Q}/webdriver.mjs`);
const { writeLocalLoginPipe, waitForLocalLoginReady, createNamedPipe } = await mod(`${Q}/local-session.mjs`);
const { parseQaTitle } = await mod(`${Q}/evidence.mjs`);

if (!process.argv[2] || !process.argv[3]) {
  console.error("usage: node contrib/perf/bench.mjs <seedDir> <appDataDir> [--fresh] [--only=switch,paginate,send,space]");
  process.exit(2);
}
const seedDir = resolve(process.argv[2]);
const appDataDir = resolve(process.argv[3]);
const fresh = process.argv.includes("--fresh");
const only = (process.argv.find((a) => a.startsWith("--only=")) ?? "").slice(7);
const appBinary = resolve(process.env.PERF_APP ?? join(REPO, "target/debug/koushi-desktop"));
const seed = JSON.parse(readFileSync(join(seedDir, "state.json"), "utf8"));
const runDir = join(appDataDir, "..", `run-${Date.now()}`);
mkdirSync(runDir, { recursive: true });
const logPath = join(runDir, "run.log");
const results = [];
function rec(name, data) {
  results.push({ name, ...data });
  console.log(name, JSON.stringify(data));
}

if (fresh) rmSync(appDataDir, { recursive: true, force: true });
mkdirSync(appDataDir, { recursive: true });

const hs = startHomeserver("tuwunel", join(seedDir, "tuwunel.toml"), join(runDir, "tuwunel.log"));
await waitForHomeserver(seed.homeserver, hs, 60000, logPath);

const loginPipe = join(appDataDir, "qa-login.pipe");
rmSync(loginPipe, { force: true });
createNamedPipe(loginPipe);
const baseEnv = childEnvironment(appDataDir, loginPipe, null);
baseEnv.KOUSHI_RESTORE_SESSION = "1";
for (const k of Object.keys(process.env)) if (k.startsWith("PERF_ENV_")) baseEnv[k.slice(9)] = process.env[k];
const dbus = ensureDbusSession(logPath, baseEnv);
const env = { ...baseEnv, ...dbus.env };
const xvfb = await startXvfb(logPath, env);
const driverPort = await freePort();
const nativePort = await freePort();
const driver = spawnLogged("tauri-driver", ["--port", String(driverPort), "--native-port", String(nativePort)], {
  cwd: REPO, env: { ...env, DISPLAY: `:${xvfb.display}` }, detached: true, logPath, label: "tauri-driver"
});
await waitForPort("127.0.0.1", driverPort, 30000);
const { remote } = await importDesktopWebdriverio();

let browser = null;
async function cleanup() {
  if (browser) await safeDeleteSession(browser);
  try { process.kill(dbus.pid, "SIGTERM"); } catch {}
  terminateProcessGroup(driver, "SIGTERM"); await settleChild(driver);
  terminateProcessGroup(xvfb.child, "SIGTERM"); await settleChild(xvfb.child);
  await stopProcess(hs);
}

const title = () => browser.execute(() => document.title);

// ---------- in-page probes ----------
// Startup milestone recorder: checked every animation frame from install on.
async function installStartupRecorder() {
  await browser.execute(() => {
    if (window.__perfStartup) return;
    const s = (window.__perfStartup = { installedAt: performance.now(), m: {}, maxGap: 0, gaps50: 0 });
    let last = performance.now();
    const tick = () => {
      const now = performance.now();
      const gap = now - last; last = now;
      s.maxGap = Math.max(s.maxGap, gap); if (gap > 50) s.gaps50++;
      const mark = (k, cond) => { if (!(k in s.m) && cond) s.m[k] = Math.round(now); };
      mark("authScreen", !!document.querySelector('[data-testid="auth-screen"]'));
      mark("roomRow", !!document.querySelector('button[data-testid="room-item"]'));
      mark("header", !!document.querySelector(".channel-title > span")?.textContent.trim());
      mark("messageRow", !!document.querySelector(".message[data-event-id]"));
      mark("composer", !!document.querySelector('.composer-inline-editor[role="textbox"]'));
      const t = document.title;
      mark("titleSessionReady", /session=ready/.test(t));
      mark("titleTimelineSubscribed", /timeline_subscribed=true/.test(t));
      if (Object.keys(s.m).length < 7 && now < 120000) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

// Find a room button, expanding bounded "Show more" sections if needed. Not timed.
async function revealRoom(name) {
  return browser.executeAsync((roomName, done) => {
    const find = () => [...document.querySelectorAll('button[data-testid="room-item"]')]
      .find((b) => (b.querySelector(".room-name")?.textContent ?? "").replace(/\s+/g, " ").trim() === roomName);
    const started = performance.now();
    const loop = () => {
      const btn = find();
      if (btn) { btn.scrollIntoView({ block: "nearest" }); return done(true); }
      document.querySelectorAll('.workspace-home-button:not(.is-active)').forEach((h) => h.click());
      document.querySelectorAll('.room-section-bound[aria-expanded="false"]').forEach((m) => m.click());
      if (performance.now() - started > 8000) return done(false);
      setTimeout(loop, 200);
    };
    loop();
  }, name);
}

// Click a room and record: header switched, any row of the target room, the
// newest message visible, and timeline settled (no DOM row/scroll change for 400ms).
async function timeRoomSwitch(name, tag, marker) {
  if (!(await revealRoom(name))) return { error: "room-not-found" };
  await sleep(300);
  return browser.executeAsync((roomName, tag, marker, done) => {
    const btn = [...document.querySelectorAll('button[data-testid="room-item"]')]
      .find((b) => (b.querySelector(".room-name")?.textContent ?? "").replace(/\s+/g, " ").trim() === roomName);
    if (!btn) return done({ error: "button-vanished" });
    // Clicking the room that is already open changes nothing to time.
    if (btn.classList.contains("is-active")) return done({ skipped: "already-active" });
    const m = {}; let maxGap = 0, gaps50 = 0, frames = 0;
    let lastSig = "", lastChange = 0, sigChanges = 0, scrollWrites = 0, lastTop = null;
    let mutations = 0;
    const mo = new MutationObserver((l) => { mutations += l.length; });
    mo.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    const t0 = performance.now(); let last = t0;
    const finish = (o) => { mo.disconnect(); done({ ...o, mutations, sigChanges, scrollWrites }); };
    btn.click();
    const tick = () => {
      const now = performance.now(); const gap = now - last; last = now; frames++;
      maxGap = Math.max(maxGap, gap); if (gap > 50) gaps50++;
      const el = (k) => Math.round(now - t0);
      const header = (document.querySelector(".channel-title > span")?.textContent ?? "").trim();
      if (!("header" in m) && header === roomName) m.header = el();
      const rows = document.querySelectorAll(".message[data-event-id]");
      const tail = [...rows].slice(-8);
      if (!("anyRow" in m) && tail.some((r) => r.textContent.includes(tag))) m.anyRow = el();
      if (!("newest" in m) && tail.some((r) => r.textContent.includes(marker + " "))) m.newest = el();
      const tl = document.querySelector('[data-testid="timeline-view"]');
      const sig = `${rows.length}|${tl?.scrollHeight}|${rows[0]?.getAttribute("data-event-id")}`;
      if (sig !== lastSig) { lastSig = sig; lastChange = now; sigChanges++; }
      if (tl && tl.scrollTop !== lastTop) { lastTop = tl.scrollTop; scrollWrites++; }
      if ("newest" in m && now - lastChange > 400) {
        return finish({ ...m, settled: Math.round(lastChange - t0), rows: rows.length, maxGap: Math.round(maxGap), gaps50, frames });
      }
      if (now - t0 > 12000) return finish({ ...m, timeout: true, rows: rows.length, maxGap: Math.round(maxGap), gaps50, header, lastSig });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }, name, tag, marker);
}

// Scroll the timeline to the top and time until older rows appear and settle.
async function timeBackPaginate() {
  return browser.executeAsync((done) => {
    const tl = document.querySelector('[data-testid="timeline-view"]');
    if (!tl) return done({ error: "no-timeline" });
    const firstId = () => document.querySelector(".message[data-event-id]")?.getAttribute("data-event-id");
    const before = firstId(); const beforeRows = document.querySelectorAll(".message[data-event-id]").length;
    const m = {}; let maxGap = 0, gaps50 = 0, lastSig = "", lastChange = 0;
    const t0 = performance.now(); let last = t0;
    tl.scrollTop = 0; tl.dispatchEvent(new Event("scroll", { bubbles: true }));
    const tick = () => {
      const now = performance.now(); const gap = now - last; last = now;
      maxGap = Math.max(maxGap, gap); if (gap > 50) gaps50++;
      const rows = document.querySelectorAll(".message[data-event-id]");
      if (!("older" in m) && firstId() !== before) m.older = Math.round(now - t0);
      const sig = `${rows.length}|${tl.scrollHeight}`;
      if (sig !== lastSig) { lastSig = sig; lastChange = now; }
      if ("older" in m && now - lastChange > 500) return done({ ...m, settled: Math.round(lastChange - t0), rowsBefore: beforeRows, rowsAfter: rows.length, scrollTopAfter: Math.round(tl.scrollTop), maxGap: Math.round(maxGap), gaps50 });
      if (now - t0 > 15000) return done({ ...m, timeout: true, rowsBefore: beforeRows, rowsAfter: rows.length, maxGap: Math.round(maxGap), gaps50 });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

// Keystroke-to-next-frame latency in the composer, using real WebDriver key input.
async function timeTyping(text) {
  await browser.execute(() => {
    const ed = document.querySelector('.composer-inline-editor[role="textbox"]');
    ed?.focus();
    const s = (window.__perfType = { lat: [], maxGap: 0 });
    let last = performance.now();
    const loop = () => { const n = performance.now(); s.maxGap = Math.max(s.maxGap, n - last); last = n; if (!s.stop) requestAnimationFrame(loop); };
    requestAnimationFrame(loop);
    ed?.addEventListener("keydown", (e) => {
      const t = e.timeStamp;
      requestAnimationFrame(() => setTimeout(() => s.lat.push(performance.now() - t), 0));
    });
  });
  for (const ch of text) await browser.keys(ch);
  await sleep(500);
  return browser.execute(() => {
    const s = window.__perfType; s.stop = true;
    const a = [...s.lat].sort((x, y) => x - y);
    const q = (p) => Math.round(a[Math.min(a.length - 1, Math.floor(p * a.length))] ?? -1);
    return { keys: a.length, p50: q(0.5), p95: q(0.95), max: Math.round(a[a.length - 1] ?? -1), maxGap: Math.round(s.maxGap) };
  });
}

async function timeSend(body) {
  await timeTyping(body);
  await browser.execute((body) => {
    const w = (window.__perfSend = { m: {} });
    document.addEventListener("keydown", (e) => { if (e.key === "Enter" && !w.t0) w.t0 = e.timeStamp; }, { capture: true, once: true });
    const ed = document.querySelector('.composer-inline-editor[role="textbox"]');
    const tick = () => {
      const now = performance.now();
      if (w.t0) {
        const rows = [...document.querySelectorAll(".message")].slice(-4);
        const row = rows.find((r) => r.textContent.includes(body));
        if (!("echo" in w.m) && row) w.m.echo = Math.round(now - w.t0);
        if (!("cleared" in w.m) && !(ed?.textContent ?? "").includes(body)) w.m.cleared = Math.round(now - w.t0);
        if (row?.hasAttribute("data-event-id") && !("eventId" in w.m)) w.m.eventId = Math.round(now - w.t0);
        if ("echo" in w.m && "eventId" in w.m) { w.done = true; return; }
        if (now - w.t0 > 15000) { w.m.timeout = true; w.done = true; return; }
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }, body);
  await browser.keys(["Enter"]);
  for (let i = 0; i < 80; i++) {
    const w = await browser.execute(() => window.__perfSend);
    if (w.done) return w.m;
    await sleep(200);
  }
  return browser.execute(() => window.__perfSend.m);
}

async function timeWorkspace(label) {
  return browser.executeAsync((label, done) => {
    const btn = label === "Home" ? document.querySelector(".workspace-home-button") : [...document.querySelectorAll(".workspace-rail button")].find((b) => b.getAttribute("aria-label") === label);
    if (!btn) return done({ error: "no-workspace-button" });
    const names = () => [...document.querySelectorAll('button[data-testid="room-item"] .room-name')].map((n) => n.textContent.trim()).join("|");
    const before = names();
    const t0 = performance.now(); let maxGap = 0, gaps50 = 0, last = t0, lastSig = before, lastChange = t0; const m = {};
    btn.click();
    const tick = () => {
      const now = performance.now(); const g = now - last; last = now; maxGap = Math.max(maxGap, g); if (g > 50) gaps50++;
      if (!("active" in m) && btn.classList.contains("is-active")) m.active = Math.round(now - t0);
      const sig = names();
      if (sig !== lastSig) { lastSig = sig; lastChange = now; if (!("list" in m)) m.list = Math.round(now - t0); }
      if ("list" in m && now - lastChange > 400) return done({ ...m, settled: Math.round(lastChange - t0), rows: sig.split("|").length, maxGap: Math.round(maxGap), gaps50 });
      if (now - t0 > 10000) return done({ ...m, timeout: true, maxGap: Math.round(maxGap), gaps50 });
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }, label);
}

async function waitStableRoomCount(maxMs) {
  const start = Date.now(); let lastRooms = -1, stableSince = Date.now();
  while (Date.now() - start < maxMs) {
    const st = parseQaTitle(await title());
    if (st.rooms !== lastRooms) { lastRooms = st.rooms; stableSince = Date.now(); }
    if (Date.now() - stableSince > 5000 && lastRooms > 0) return { rooms: lastRooms, ms: stableSince - start };
    await sleep(250);
  }
  return { rooms: lastRooms, timeout: true };
}

async function diagnosticsSnapshot() {
  return browser.executeAsync((done) => {
    const inv = window.__TAURI_INTERNALS__?.invoke;
    if (!inv) return done({ error: "no-invoke" });
    inv("get_diagnostic_snapshot", {}).then((s) => done(s), (e) => done({ error: String(e) }));
  });
}

async function probe() {
  return browser.execute(() => {
    const t = (sel) => (document.querySelector(sel)?.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 80);
    const rows = [...document.querySelectorAll('button[data-testid="room-item"]')];
    return {
      header: t(".channel-title > span"),
      activeRow: t('button[data-testid="room-item"].is-active .room-name'),
      firstRows: rows.slice(0, 5).map((r) => (r.querySelector(".room-name")?.textContent ?? "").trim()),
      roomButtons: rows.length,
      timelineView: !!document.querySelector('[data-testid="timeline-view"]'),
      composer: !!document.querySelector('.composer-inline-editor[role="textbox"]'),
      messages: document.querySelectorAll(".message").length,
      mainText: t("main").slice(0, 160)
    };
  });
}

// ---------- scenario ----------
const byKind = (k) => seed.rooms.filter((r) => r.kind === k);
// Space children are hidden from Home, so the room list cannot reveal them by name.
const spaceChildren = new Set(seed.spaces.flatMap((sp) => sp.children));
const small = byKind("small").filter((r) => !spaceChildren.has(r.name)).sort((a, b) => a.name.localeCompare(b.name));
const big = byKind("big").sort((a, b) => a.name.localeCompare(b.name));
const dms = byKind("dm").filter((d) => d.helper >= 8).sort((a, b) => a.helper - b.helper);
if (small.length < 8 || big.length < 2) throw new Error("seed has too few Home rooms; reseed with the defaults");
const pickSmall = [0.05, 0.2, 0.4, 0.6, 0.8, 0.98].map((f) => small[Math.floor(f * (small.length - 1))]);
// Sends go to a room the switch suite never times, so its newest-message marker stays valid across runs.
const sendRoom = small.find((r) => !pickSmall.includes(r));
const targets = [];
for (let i = 0; i < Math.max(pickSmall.length, 3); i++) {
  if (pickSmall[i]) targets.push({ ...pickSmall[i], label: pickSmall[i].name });
  if (big[i]) targets.push({ ...big[i], label: big[i].name });
  if (dms[i]) targets.push({ ...dms[i], label: `Perf Helper ${dms[i].helper}` });
}

async function switchSuite(phase) {
  for (const r of targets) {
    mark(`switch ${phase} ${r.label}`);
    const res = await timeRoomSwitch(r.label, r.tag, r.lastMarker);
    rec(`switch.${phase}`, { room: r.kind, label: r.label, ...res });
    await sleep(700);
  }
}

const { spawn } = await import("node:child_process");
const { openSync } = await import("node:fs");
// Per-thread CPU once a second. Large (about 100 MB for a full run), so opt-in.
const top = process.env.PERF_TOP
  ? spawn("top", ["-b", "-H", "-d", "1", "-w", "200"], { stdio: ["ignore", openSync(join(runDir, "top.log"), "w"), "ignore"] })
  : null;
const mark = (label) => appendFileSync(join(runDir, "marks.log"), `${Date.now()} ${label}\n`);
try {
  const tSpawn = Date.now();
  browser = await remote({ hostname: "127.0.0.1", port: driverPort, logLevel: "error", capabilities: webdriverCapabilities(appBinary) });
  const tSession = Date.now();
  await installStartupRecorder();
  const origin = await browser.execute(() => ({ timeOrigin: performance.timeOrigin, now: performance.now() }));
  rec("launch", { webdriverSessionMs: tSession - tSpawn, pageOriginAfterSpawnMs: Math.round(origin.timeOrigin - tSpawn), recorderInstalledAtPageMs: Math.round(origin.now) });

  if (fresh || (await browser.execute(() => !!document.querySelector('[data-testid="auth-screen"]')))) {
    // first login with full initial sync
    const deadline = Date.now() + 60000;
    while (!(await browser.execute(() => !!document.querySelector('[data-testid="auth-screen"]'))) && Date.now() < deadline) await sleep(100);
    const tLogin = Date.now();
    await writeLocalLoginPipe(loginPipe, { homeserver: seed.homeserver, username: seed.username, password: seed.password, deviceName: "Koushi Perf" });
    try {
      await waitForLocalLoginReady({ browser, allowNewIdentityBootstrap: true, credentials: { homeserver: seed.homeserver, username: seed.username, password: seed.password } }, 90000);
      rec("login.readyMs", { ms: Date.now() - tLogin });
    } catch (e) {
      rec("login.notReady", { afterMs: Date.now() - tLogin, ...(await probe()) });
    }
    rec("login.roomsStable", await waitStableRoomCount(120000));
  } else {
    // restored session: wait for ready+timeline, then read the in-page milestones
    const deadline = Date.now() + 120000;
    while (Date.now() < deadline) {
      const s = await browser.execute(() => window.__perfStartup);
      if (s && "messageRow" in s.m && "titleTimelineSubscribed" in s.m) break;
      await sleep(100);
    }
    const s = await browser.execute(() => window.__perfStartup);
    rec("restore.milestonesPageMs", { ...s.m, recorderInstalled: Math.round(s.installedAt), maxGap: Math.round(s.maxGap), gaps50: s.gaps50, wallSinceSpawn: Date.now() - tSpawn });
    rec("restore.roomsStable", await waitStableRoomCount(60000));
  }
  rec("title", { title: (await title()).replace(/[!@][^ ]*/g, "<id>") });

  const { execSync } = await import("node:child_process");
  let perfProcs = [];
  if (process.env.PERF_RECORD) {
    // Find this run's app (by its data dir) and its WebKit web process.
    const pids = execSync(`for p in $(pgrep -x koushi-desktop); do tr '\\0' '\\n' < /proc/$p/environ 2>/dev/null | grep -qx 'KOUSHI_DATA_DIR=${appDataDir}' && echo $p; done`, { shell: "/bin/bash" }).toString().trim().split(/\s+/).filter(Boolean);
    const appPid = pids[0];
    const web = execSync(`pgrep -P ${appPid} -x WebKitWebProces || true`, { shell: "/bin/bash" }).toString().trim().split(/\s+/)[0];
    rec("perf.pids", { app: !!appPid, web: !!web });
    for (const [label, pid] of [["web", web], ["app", appPid]]) {
      if (!pid) continue;
      perfProcs.push(spawn("perf", ["record", "-F", "1999", "-g", "-e", "cpu-clock", "-p", pid, "-o", join(runDir, `${label}.perf.data`)], { stdio: ["ignore", "ignore", openSync(join(runDir, `perf-${label}.log`), "w")] }));
    }
    await sleep(1000);
  }
  if (!only || only.includes("switch")) {
    await switchSuite("cold");
    await switchSuite("warm");
  }
  for (const p of perfProcs) p.kill("SIGINT");
  if (perfProcs.length) await sleep(3000);
  if (!only || only.includes("paginate")) {
    const b = big[0];
    await timeRoomSwitch(b.name, b.tag, b.lastMarker);
    await sleep(1500);
    for (let i = 0; i < 6; i++) {
      rec("paginate.big", { page: i, ...(await timeBackPaginate()) });
      await sleep(500);
    }
  }
  if (!only || only.includes("send")) {
    const r = sendRoom;
    rec("send.room", { label: r.name, ...(await timeRoomSwitch(r.name, r.tag, r.lastMarker)) });
    await sleep(1000);
    for (let i = 0; i < 3; i++) {
      rec("send", await timeSend(`perf send probe ${Date.now()} ${i}`));
      await sleep(800);
    }
    const b = big[1];
    await timeRoomSwitch(b.name, b.tag, b.lastMarker);
    await sleep(1000);
    rec("typing.bigRoom", await timeTyping("typing latency probe in a big room with many rows"));
  }
  if (!only || only.includes("space")) {
    for (const sp of seed.spaces) {
      rec("space.enter", { space: sp.name, ...(await timeWorkspace(sp.name)) });
      await sleep(800);
    }
    rec("space.home", await timeWorkspace("Home"));
  }
  const diag = await diagnosticsSnapshot();
  writeFileSync(join(runDir, "diagnostics.json"), JSON.stringify(diag, null, 1));
  const mem = await browser.execute(() => ({ domNodes: document.getElementsByTagName("*").length, heap: performance.memory?.usedJSHeapSize ?? null }));
  rec("end", mem);
} catch (e) {
  console.error("bench failed:", e.message);
  process.exitCode = 1;
} finally {
  writeFileSync(join(runDir, "results.json"), JSON.stringify(results, null, 1));
  console.log(`run_dir=${runDir}`);
  top?.kill();
  await cleanup();
}
