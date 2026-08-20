import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, existsSync } from "node:fs";
import * as net from "node:net";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { childEnvironment, nssWrapperEnvironment, buildLdPreload } from "./redaction.mjs";

export function guiScenarioServerKind() {
  if (serverOption === "tuwunel") {
    return serverOption;
  }
  throw new Error("--server must be tuwunel for local GUI scenarios");
}


export function checkLinuxTools() {
  if (process.platform !== "linux") {
    throw new Error("linux GUI smoke must run on Linux");
  }
  const requiredTools = [
    "npm",
    "cargo",
    "Xvfb",
    "tauri-driver",
    "WebKitWebDriver",
    "mkfifo",
    "dbus-daemon",
    "dbus-monitor"
  ];
  const missing = [];
  for (const tool of requiredTools) {
    try {
      execFileSync("which", [tool], { encoding: "utf8", stdio: "ignore" });
    } catch {
      missing.push(tool);
    }
  }
  if (missing.length) {
    throw new Error(`missing required Linux GUI tools: ${missing.join(", ")}`);
  }
}


export function qaDataDirForRun(runDir) {
  if (qaProfile === undefined) {
    return join(runDir, "data");
  }
  return join(repoRoot, ".local-secrets", "qa-profiles", validatedQaProfileName(), "data");
}


export function validatedQaProfileName() {
  if (!qaProfile || !/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(qaProfile)) {
    throw new Error("qa profile must be 1-64 characters of letters, numbers, underscore, or dash");
  }
  return qaProfile;
}


export async function startXvfb(logPath, buildEnv) {
  const display = await findFreeDisplayNumber();
  const child = spawn("Xvfb", [`:${display}`, "-screen", "0", "1280x900x24", "-nolisten", "tcp", "-ac"], {
    cwd: repoRoot,
    env: buildEnv,
    detached: true,
    stdio: ["ignore", "pipe", "pipe"]
  });
  recordProcessOutput(child, logPath, "Xvfb");
  child.unref();
  try {
    await waitForDisplaySocket(display, timeoutMs);
    return { child, display };
  } catch (error) {
    terminateProcessGroup(child, "SIGTERM");
    await settleChild(child);
    throw error;
  }
}


export async function waitForSignedOutTitle(browser, timeout) {
  const startedAt = Date.now();
  let lastTitle = "";
  while (Date.now() - startedAt < timeout) {
    lastTitle = await browser.execute(() => document.title);
    if (
      lastTitle.includes("session=signedOut") &&
      lastTitle.includes("errors=0") &&
      qaStatusHasAttentionBaseline(parseQaTitle(lastTitle))
    ) {
      return lastTitle;
    }
    await sleep(250);
  }
  throw new Error(`signed-out QA title did not appear. Last title: ${lastTitle}`);
}


export function spawnLogged(command, argsList, { cwd, env, detached = false, logPath, label }) {
  const child = spawn(command, argsList, {
    cwd,
    env,
    detached,
    stdio: ["ignore", "pipe", "pipe"]
  });
  recordProcessOutput(child, logPath, label);
  if (detached) {
    child.unref();
  }
  return child;
}


export function recordProcessOutput(child, logPath, label) {
  const prefix = `[${label}] `;
  child.stdout.on("data", (chunk) => appendFileSync(logPath, prefix + chunk.toString()));
  child.stderr.on("data", (chunk) => appendFileSync(logPath, prefix + chunk.toString()));
  child.on("error", (error) => {
    appendFileSync(logPath, `${prefix}error: ${error.message}\n`);
  });
}


export async function runLoggedCommand(command, argsList, { cwd, env, logPath, label }) {
  const child = spawn(command, argsList, {
    cwd,
    env,
    stdio: ["ignore", "pipe", "pipe"]
  });
  recordProcessOutput(child, logPath, label);
  const exitCode = await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve(0);
      } else {
        reject(new Error(`${label} exited with ${code ?? signal}`));
      }
    });
  });
  return exitCode;
}


export function resolveDebugAppBinary() {
  const cargoTargetDir = process.env.CARGO_TARGET_DIR;
  const candidates = [];
  if (cargoTargetDir) {
    candidates.push(join(cargoTargetDir, "debug", "koushi-desktop"));
  }
  candidates.push(join(desktopDir, "src-tauri", "target", "debug", "koushi-desktop"));
  candidates.push(join(repoRoot, "target", "debug", "koushi-desktop"));
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(`unable to resolve debug Tauri binary. Checked: ${candidates.join(", ")}`);
  }
  return found;
}

/**
 * Resolve the debug Tauri binary to drive, building it first unless
 * `--skip-build` is passed. `--skip-build` is the fast inner-loop path: it
 * reuses an already-built binary (via `--app-binary=PATH` or the default
 * debug target) so iterating on a scenario does not pay the full Tauri
 * rebuild each time.
 */

export async function ensureAppBinary({ cwd, env, logPath }) {
  if (!args.has("--skip-build")) {
    await runLoggedCommand("npm", ["run", "tauri", "build", "--", "--debug", "--no-bundle"], {
      cwd,
      env,
      logPath,
      label: "tauri build"
    });
  }
  const explicit = optionValue("--app-binary");
  const appBinary = explicit
    ? isAbsolute(explicit)
      ? explicit
      : resolve(explicit)
    : resolveDebugAppBinary();
  if (!existsSync(appBinary)) {
    throw new Error(
      `app binary not found at ${appBinary}. With --skip-build, pass --app-binary=PATH ` +
        `or build once first: npm --prefix apps/desktop run tauri build -- --debug --no-bundle`
    );
  }
  return appBinary;
}


export async function waitForDisplaySocket(display, timeout) {
  const socketPath = `/tmp/.X11-unix/X${display}`;
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeout) {
    if (existsSync(socketPath)) {
      return;
    }
    await sleep(100);
  }
  throw new Error(`Xvfb display :${display} did not become available`);
}


export async function findFreeDisplayNumber() {
  for (let display = 90; display < 200; display += 1) {
    if (!existsSync(`/tmp/.X11-unix/X${display}`)) {
      return display;
    }
  }
  throw new Error("unable to find a free Xvfb display");
}


export async function waitForPort(hostname, port, timeout) {
  const startedAt = Date.now();
  let lastError = "";
  while (Date.now() - startedAt < timeout) {
    try {
      await connectOnce(hostname, port, 1000);
      return;
    } catch (error) {
      lastError = error.message;
      await sleep(100);
    }
  }
  throw new Error(`port ${port} on ${hostname} did not become ready: ${lastError}`);
}


export function connectOnce(hostname, port, timeout) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: hostname, port });
    const fail = (error) => {
      socket.destroy();
      reject(error);
    };
    socket.setTimeout(timeout);
    socket.once("connect", () => {
      socket.end();
      resolve();
    });
    socket.once("timeout", () => fail(new Error(`timed out connecting to ${hostname}:${port}`)));
    socket.once("error", fail);
  });
}


export function normalizePath(path) {
  return path.replace(/\\/g, "/");
}


export function startDbusMonitor(logPath, env) {
  const busAddress = env.DBUS_SESSION_BUS_ADDRESS;
  if (!busAddress) {
    throw new Error("DBUS_SESSION_BUS_ADDRESS is required to start the notification DBus monitor");
  }
  const child = spawn(
    "dbus-monitor",
    ["--address", busAddress, "interface='org.freedesktop.Notifications'"],
    {
      cwd: repoRoot,
      env,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"]
    }
  );
  const state = { child, buffer: "" };
  recordProcessOutput(child, logPath, "dbus-monitor");
  child.stdout.on("data", (chunk) => {
    state.buffer += chunk.toString();
  });
  child.stderr.on("data", (chunk) => {
    state.buffer += chunk.toString();
  });
  child.once("exit", (code, signal) => {
    if (code !== 0 && signal === null) {
      state.buffer += `\ndbus-monitor exited with ${code}\n`;
    }
  });
  child.unref();
  return state;
}


export async function triggerNotificationSmoke(browser, timeout) {
  const result = await browser.executeAsync((done) => {
    const notificationApi = window.Notification;
    if (!notificationApi) {
      done({ ok: false, reason: "notification_api_unavailable" });
      return;
    }

    Promise.resolve(notificationApi.requestPermission())
      .then((permission) => {
        if (permission !== "granted") {
          done({ ok: false, reason: `permission_${permission}` });
          return;
        }

        const notification = new notificationApi("Koushi QA", {
          body: "Notification smoke"
        });
        window.setTimeout(() => {
          try {
            notification.close();
          } catch {
            // ignore close errors
          }
          done({ ok: true });
        }, 0);
      })
      .catch((error) => {
        done({ ok: false, reason: String(error) });
      });
  });

  if (!result?.ok) {
    throw new Error(`notification smoke failed: ${result?.reason ?? "unknown error"}`);
  }

  await sleep(Math.min(timeout, 250));
}


export async function waitForDbusMonitorToken(monitor, timeout) {
  const startedAt = Date.now();
  let lastBuffer = "";
  while (Date.now() - startedAt < timeout) {
    lastBuffer = monitor.buffer;
    if (
      lastBuffer.includes("org.freedesktop.Notifications") &&
      lastBuffer.includes("Notify")
    ) {
      return;
    }
    if (monitor.child.exitCode !== null || monitor.child.signalCode !== null) {
      throw new Error(`notification DBus monitor exited early. Last output: ${lastBuffer}`);
    }
    await sleep(100);
  }
  throw new Error(`notification DBus evidence not observed. Last monitor output: ${lastBuffer}`);
}


export async function waitForDbusMonitorReady(monitor, timeout) {
  const startedAt = Date.now();
  let lastBuffer = "";
  while (Date.now() - startedAt < timeout) {
    lastBuffer = monitor.buffer;
    if (lastBuffer.includes("NameAcquired") || lastBuffer.includes("monitoring")) {
      return;
    }
    if (monitor.child.exitCode !== null || monitor.child.signalCode !== null) {
      throw new Error(`notification DBus monitor exited before readiness. Last output: ${lastBuffer}`);
    }
    await sleep(100);
  }
  throw new Error(`notification DBus monitor did not become ready. Last monitor output: ${lastBuffer}`);
}


export function terminateProcessGroup(child, signal) {
  if (!child?.pid) {
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
      // ignore cleanup failures
    }
  }
}


export async function settleChild(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  await new Promise((resolve) => {
    const timer = setTimeout(() => {
      terminateProcessGroup(child, "SIGKILL");
      resolve();
    }, 5000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}


export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}


export function ensureDbusSession(logPath, env) {
  if (process.env.DBUS_SESSION_BUS_ADDRESS) {
    return {
      env: { DBUS_SESSION_BUS_ADDRESS: process.env.DBUS_SESSION_BUS_ADDRESS },
      pid: null
    };
  }

  const output = execFileSync(
    "dbus-daemon",
    ["--session", "--fork", "--print-address=1", "--print-pid=1"],
    {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      env
    }
  );
  appendFileSync(logPath, `[dbus-daemon] ${output}`);

  const [addressLine, pidLine] = output
    .trim()
    .split(/\s*\n\s*/)
    .filter((line) => line.length > 0);
  const pid = Number(pidLine);
  if (!addressLine || !Number.isFinite(pid) || pid <= 0) {
    throw new Error(`dbus-daemon did not return a usable session bus: ${output}`);
  }

  return {
    env: { DBUS_SESSION_BUS_ADDRESS: addressLine },
    pid
  };
}


