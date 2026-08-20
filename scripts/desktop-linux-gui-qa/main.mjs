#!/usr/bin/env node
import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import * as net from "node:net";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { deflateSync } from "node:zlib";

import {
  checkInstalledHomeserver,
  createRoom,
  freePort,
  inviteUser as inviteUserToRoom,
  joinRoom,
  registerUser,
  sendRoomEmoteMessage,
  sendRoomFormattedMessage,
  sendRoomMessage,
  sendRoomNoticeMessage,
  setDisplayName,
  startHomeserver,
  stopProcess,
  tuwunelConfig,
  waitForHomeserver
} from "../lib/local-homeserver-qa.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const desktopDir = join(repoRoot, "apps", "desktop");
const desktopPackageRequire = createRequire(new URL("../apps/desktop/package.json", import.meta.url));
const pngCrc32Table = Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) {
    value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  }
  return value >>> 0;
});
import { checks, run } from "./registry.mjs";
import { childEnvironment, qaDataDirForRun } from "./redaction.mjs";
import { webdriverCapabilities } from "./webdriver.mjs";
import { parseQaTitle, qaStatusHasAttentionBaseline, qaStatusHasRequiredPanel, qaStatusHasSendSuccess, qaStatusIsReady, qaWindowStateIsReady } from "./evidence.mjs";
import { checkLinuxTools, optionValue, resolveArtifactRoot, printUsage } from "./runtime.mjs";


const args = new Set(process.argv.slice(2));
const guiScenario = optionValue("--scenario") ?? "signed-out";
const serverOption = optionValue("--server") ?? "tuwunel";
const qaProfile = optionValue("--qa-profile");
const realLoginFromStdin = args.has("--real-login-from-stdin");
const allowEmptyTimeline = args.has("--allow-empty-timeline");
const artifactRoot = resolveArtifactRoot(optionValue("--artifact-dir"));
const timeoutMs = Number(optionValue("--timeout-ms") ?? "120000");

if (args.has("--print-artifact-root")) {
  console.log(artifactRoot);
  process.exit(0);
}

if (args.has("--list")) {
  for (const check of checks) {
    console.log(check);
  }
  process.exit(0);
}

if (args.has("--check-tools")) {
  checkLinuxTools();
  console.log("linux GUI smoke tools available");
  process.exit(0);
}

if (args.has("--child-env-keys")) {
  for (const key of Object.keys(childEnvironment("/tmp/koushi-desktop-linux-gui-qa")).sort()) {
    console.log(key);
  }
  process.exit(0);
}

if (args.has("--child-env")) {
  for (const [key, value] of Object.entries(
    childEnvironment(qaDataDirForRun("/tmp/koushi-desktop-linux-gui-qa"))
  ).sort(([left], [right]) => left.localeCompare(right))) {
    console.log(`${key}=${value}`);
  }
  process.exit(0);
}

if (args.has("--print-real-login-transport")) {
  console.log("fifo");
  process.exit(0);
}

if (args.has("--print-webdriver-capabilities")) {
  const appBinary = optionValue("--app-binary");
  if (!appBinary) {
    throw new Error("--app-binary=PATH is required when printing WebDriver capabilities");
  }
  console.log(JSON.stringify(webdriverCapabilities(appBinary), null, 2));
  process.exit(0);
}

const qaTitlePanelSample = optionValue("--qa-title-panel");
if (qaTitlePanelSample !== undefined) {
  const status = parseQaTitle(qaTitlePanelSample);
  console.log(status.panel ?? "missing");
  process.exit(0);
}

const qaTitlePanelReadySample = optionValue("--qa-title-panel-ready");
if (qaTitlePanelReadySample !== undefined) {
  const requiredPanel = optionValue("--required-panel") ?? "keyboardSettings";
  const status = parseQaTitle(qaTitlePanelReadySample);
  console.log(qaStatusHasRequiredPanel(status, requiredPanel) ? "ready" : "not-ready");
  process.exit(0);
}

const qaTitleReadySample = optionValue("--qa-title-ready");
if (qaTitleReadySample !== undefined) {
  console.log(
    qaStatusIsReady(parseQaTitle(qaTitleReadySample), false, allowEmptyTimeline)
      ? "ready"
      : "not-ready"
  );
  process.exit(0);
}

const qaTitleAttentionReadySample = optionValue("--qa-title-attention-ready");
if (qaTitleAttentionReadySample !== undefined) {
  console.log(
    qaStatusHasAttentionBaseline(parseQaTitle(qaTitleAttentionReadySample)) ? "ready" : "not-ready"
  );
  process.exit(0);
}

const qaWindowStateReadySample = optionValue("--qa-window-state-ready");
if (qaWindowStateReadySample !== undefined) {
  console.log(qaWindowStatePathHasContract(qaWindowStateReadySample) ? "ready" : "not-ready");
  process.exit(0);
}

const qaTitleSendReadySample = optionValue("--qa-title-send-ready");
if (qaTitleSendReadySample !== undefined) {
  console.log(qaStatusHasSendSuccess(parseQaTitle(qaTitleSendReadySample)) ? "ready" : "not-ready");
  process.exit(0);
}

const qaRecoveredTitleReadySample = optionValue("--qa-title-ready-require-recovered");
if (qaRecoveredTitleReadySample !== undefined) {
  console.log(
    qaStatusIsReady(parseQaTitle(qaRecoveredTitleReadySample), true, allowEmptyTimeline)
      ? "ready"
      : "not-ready"
  );
  process.exit(0);
}

const TIMELINE_NAVIGATION_SEED_MESSAGE_COUNT = 24;
const TIMELINE_NAVIGATION_SEED_LINE_COUNT = 12;

if (args.has("--run")) {
  await run();
  process.exit(0);
}

printUsage();


if (args.has("--run")) {
  await run();
  process.exit(0);
}

printUsage();
