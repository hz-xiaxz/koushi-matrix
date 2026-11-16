// Focused repro: does back-pagination in a big room keep loading older messages?
// Usage (from the repo root): node contrib/perf/repro-paginate.mjs . <seedDir> <appDataDir> [--fresh] [--mode=wheel|key|scrolltop] [--room=0]
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [repoArg, seedArg, appArg] = process.argv.slice(2);
const REPO = resolve(repoArg);
const mod = (p) => import(pathToFileURL(join(REPO, p)).href);
const Q = "scripts/desktop-linux-gui-qa";
const { startHomeserver, stopProcess, waitForHomeserver, freePort } = await mod("scripts/lib/local-homeserver-qa.mjs");
const { childEnvironment } = await mod(`${Q}/redaction.mjs`);
const { ensureDbusSession, startXvfb, spawnLogged, terminateProcessGroup, settleChild, waitForPort, sleep } = await mod(`${Q}/runtime.mjs`);
const { importDesktopWebdriverio, webdriverCapabilities, safeDeleteSession } = await mod(`${Q}/webdriver.mjs`);
const { writeLocalLoginPipe, waitForLocalLoginReady, createNamedPipe } = await mod(`${Q}/local-session.mjs`);

const seedDir = resolve(seedArg);
const appDataDir = resolve(appArg);
const fresh = process.argv.includes("--fresh");
const mode = (process.argv.find((a) => a.startsWith("--mode=")) ?? "--mode=wheel").slice(7);
const roomIdx = Number((process.argv.find((a) => a.startsWith("--room=")) ?? "--room=0").slice(7));
const appBinary = resolve(process.env.PERF_APP ?? join(REPO, "target/debug/koushi-desktop"));
const seed = JSON.parse(readFileSync(join(seedDir, "state.json"), "utf8"));
const runDir = join(appDataDir, "..", `repro-${mode}-${Date.now()}`);
mkdirSync(runDir, { recursive: true });
const logPath = join(runDir, "run.log");
const out = [];
const rec = (name, data) => { out.push({ name, ...data }); console.log(name, JSON.stringify(data)); };

if (fresh) rmSync(appDataDir, { recursive: true, force: true });
mkdirSync(appDataDir, { recursive: true });
const hs = startHomeserver("tuwunel", join(seedDir, "tuwunel.toml"), join(runDir, "tuwunel.log"));
await waitForHomeserver(seed.homeserver, hs, 60000, logPath);
const loginPipe = join(appDataDir, "qa-login.pipe");
rmSync(loginPipe, { force: true });
createNamedPipe(loginPipe);
const baseEnv = childEnvironment(appDataDir, loginPipe, null);
baseEnv.KOUSHI_RESTORE_SESSION = "1";
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

const big = seed.rooms.filter((r) => r.kind === "big").sort((a, b) => a.name.localeCompare(b.name));
const room = big[roomIdx];

// Snapshot of the timeline: oldest/newest seeded message index among mounted rows, geometry, QA title.
async function snap() {
  return browser.execute((tag) => {
    const tl = document.querySelector('[data-testid="timeline-view"]');
    const rows = [...document.querySelectorAll(".message[data-event-id]")];
    const idx = rows.map((r) => {
      const m = r.textContent.match(new RegExp(tag.replace(/[[\]]/g, "\\$&") + " m(\\d+)"));
      return m ? Number(m[1]) : null;
    }).filter((x) => x !== null);
    const t = document.title;
    const tok = (k) => (t.match(new RegExp(`${k}=(\\S+)`)) ?? [])[1];
    return {
      minIdx: idx.length ? Math.min(...idx) : null, maxIdx: idx.length ? Math.max(...idx) : null, rows: rows.length,
      scrollTop: Math.round(tl?.scrollTop ?? -1), scrollHeight: tl?.scrollHeight ?? -1, clientHeight: tl?.clientHeight ?? -1,
      startOfConversation: /Start of conversation|会話の始まり/.test(tl?.textContent ?? ""),
      spinner: !!tl?.querySelector('[role="progressbar"], .timeline-loading, .spinner'),
      items: tok("timeline_visible"), backfill: tok("timeline_backfill")
    };
  }, room.tag);
}

async function openRoom() {
  return browser.executeAsync((name, done) => {
    const started = performance.now();
    const loop = () => {
      const btn = [...document.querySelectorAll('button[data-testid="room-item"]')]
        .find((b) => (b.querySelector(".room-name")?.textContent ?? "").replace(/\s+/g, " ").trim() === name);
      if (btn) { btn.scrollIntoView({ block: "nearest" }); btn.click(); return done(true); }
      document.querySelectorAll(".workspace-home-button:not(.is-active)").forEach((h) => h.click());
      document.querySelectorAll('.room-section-bound[aria-expanded="false"]').forEach((m) => m.click());
      if (performance.now() - started > 10000) return done(false);
      setTimeout(loop, 200);
    };
    loop();
  }, room.name);
}

async function scrollUp() {
  const tl = await browser.$('[data-testid="timeline-view"]');
  if (mode === "scrolltop") {
    await browser.execute(() => { const t = document.querySelector('[data-testid="timeline-view"]'); t.scrollTop = 0; t.dispatchEvent(new Event("scroll", { bubbles: true })); });
  } else if (mode === "key") {
    await tl.click({ x: 10, y: 10 }).catch(() => {});
    await browser.keys(["Home"]);
  } else {
    await browser.action("wheel").scroll({ origin: tl, deltaY: -4000, duration: 100 }).perform();
  }
}

try {
  browser = await remote({ hostname: "127.0.0.1", port: driverPort, logLevel: "error", capabilities: webdriverCapabilities(appBinary) });
  if (fresh || (await browser.execute(() => !!document.querySelector('[data-testid="auth-screen"]')))) {
    const deadline = Date.now() + 60000;
    while (!(await browser.execute(() => !!document.querySelector('[data-testid="auth-screen"]'))) && Date.now() < deadline) await sleep(100);
    await writeLocalLoginPipe(loginPipe, { homeserver: seed.homeserver, username: seed.username, password: seed.password, deviceName: "Koushi Perf" });
    try {
      await waitForLocalLoginReady({ browser, allowNewIdentityBootstrap: true, credentials: { homeserver: seed.homeserver, username: seed.username, password: seed.password } }, 90000);
      rec("login", { ready: true });
    } catch { rec("login", { ready: false }); }
  } else {
    for (let i = 0; i < 300 && !/session=ready/.test(await browser.getTitle()); i++) await sleep(100);
  }
  await sleep(3000);
  rec("open", { ok: await openRoom(), room: room.name, total: room.count });
  await sleep(3000);
  rec("opened", await snap());

  let lastMin = Infinity;
  let stuckRounds = 0;
  for (let round = 0; round < 40 && stuckRounds < 3; round++) {
    const before = await snap();
    await scrollUp();
    let after = before;
    const t0 = Date.now();
    // Keep the user at the top for up to 10 s: progress = older message index mounted.
    while (Date.now() - t0 < 10000) {
      await sleep(250);
      after = await snap();
      if (after.minIdx !== null && after.minIdx < (before.minIdx ?? Infinity)) break;
      if (mode !== "scrolltop" && after.scrollTop > 50) await scrollUp();
    }
    const progressed = after.minIdx !== null && after.minIdx < (before.minIdx ?? Infinity);
    rec("round", { round, ms: Date.now() - t0, progressed, before, after });
    if (after.minIdx === 0 || after.startOfConversation) { rec("reachedStart", { round }); break; }
    stuckRounds = progressed ? 0 : stuckRounds + 1;
    lastMin = Math.min(lastMin, after.minIdx ?? Infinity);
    await sleep(500);
  }
  rec("final", { oldestSeen: lastMin, total: room.count, stuck: stuckRounds >= 3 });
  const diag = await browser.executeAsync((done) => {
    window.__TAURI_INTERNALS__.invoke("get_diagnostic_snapshot", {}).then(done, (e) => done({ error: String(e) }));
  });
  writeFileSync(join(runDir, "diagnostics.json"), JSON.stringify(diag, null, 1));
} catch (e) {
  console.error("repro failed:", e.stack ?? e.message);
  process.exitCode = 1;
} finally {
  writeFileSync(join(runDir, "results.json"), JSON.stringify(out, null, 1));
  console.log(`run_dir=${runDir}`);
  if (browser) await safeDeleteSession(browser);
  try { process.kill(dbus.pid, "SIGTERM"); } catch {}
  terminateProcessGroup(driver, "SIGTERM"); await settleChild(driver);
  terminateProcessGroup(xvfb.child, "SIGTERM"); await settleChild(xvfb.child);
  await stopProcess(hs);
}
