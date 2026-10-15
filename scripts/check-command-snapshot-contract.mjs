#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const commands = join(root, "apps/desktop/src-tauri/src/commands");
const forbidden = /\b(?:FrontendDesktopSnapshot|current_snapshot)\b/;
const allowed = new Set(["get_snapshot", "settlement_snapshot", "resync_snapshot"]);
// The renderer's command names are a frozen half of this contract, so the check below
// refuses to pass vacuously if the file that names them moves or stops matching.
const MIN_RENDERER_COMMANDS = 100;
const violations = [];
const removedAcknowledgement = /(?:AcknowledgeTimeline(?:Projection|BatchRendered)|acknowledge_timeline_(?:projection|batch_rendered)|acknowledgeTimeline(?:Projection|BatchRendered)|timelineAcknowledgementDelivery)/;

function sourceFiles(directory) {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.(?:rs|ts|tsx)$/.test(name) && !/\.test\.(?:ts|tsx)$/.test(name) ? [path] : [];
  });
}

function functions(source) {
  const matches = [
    ...source.matchAll(/(?:pub(?:\([^)]*\))?\s+)?async fn ([a-z0-9_]+)\b/g)
  ];
  return matches.flatMap((match) => {
    let bodyStart = match.index + match[0].length;
    while (bodyStart < source.length && !"{;".includes(source[bodyStart])) bodyStart++;
    if (source[bodyStart] !== "{") return [];
    let depth = 1;
    let end = bodyStart + 1;
    while (end < source.length && depth) {
      if (source[end] === "{") depth++;
      if (source[end] === "}") depth--;
      end++;
    }
    return [{ name: match[1], start: match.index, text: source.slice(match.index, end) }];
  });
}

for (const path of sourceFiles(commands)) {
  const name = relative(commands, path);
  const source = readFileSync(path, "utf8");
  for (const fn of functions(source)) {
    if (!allowed.has(fn.name) && forbidden.test(fn.text)) {
      violations.push(`${relative(root, path)}:${source.slice(0, fn.start).split("\n").length} ${fn.name}`);
    }
  }
}

for (const directory of [
  join(root, "crates/koushi-core/src"),
  join(root, "apps/desktop/src-tauri/src"),
  join(root, "apps/desktop/src")
]) {
  for (const path of sourceFiles(directory)) {
    if (removedAcknowledgement.test(readFileSync(path, "utf8"))) {
      violations.push(`${relative(root, path)} removed timeline acknowledgement route`);
    }
  }
}
for (const name of [
  "apps/desktop/src/backend/timelineAcknowledgementDelivery.ts",
  "apps/desktop/src/backend/timelineAcknowledgementDelivery.test.ts"
]) {
  if (existsSync(join(root, name))) violations.push(`${name} removed acknowledgement owner`);
}

const helper = join(commands, "mod.rs");
if (/async fn current_snapshot\b/.test(readFileSync(helper, "utf8"))) {
  violations.push(`${relative(root, helper)} current_snapshot helper`);
}

// The IPC snapshot contract version exists in two languages, and the renderer rejects every
// snapshot whose version it does not recognize, so a bump on one side alone ships an app that
// renders its own recovery screen instead of the app. Both constants are read and compared.
const versionSources = [
  ["apps/desktop/src-tauri/src/dto.rs", /pub const SNAPSHOT_SCHEMA_VERSION: u32 = (\d+);/],
  ["apps/desktop/src/domain/types.ts", /export const SNAPSHOT_SCHEMA_VERSION = (\d+);/]
];
const versions = versionSources.map(([name, pattern]) => {
  const match = readFileSync(join(root, name), "utf8").match(pattern);
  if (!match) violations.push(`${name} has no SNAPSHOT_SCHEMA_VERSION`);
  return [name, match ? Number(match[1]) : null];
});
if (versions.every(([, version]) => version !== null)) {
  const [[rustName, rust], [tsName, ts]] = versions;
  if (rust !== ts) violations.push(`${rustName} says ${rust} while ${tsName} says ${ts}`);
}

// A Tauri command name is a contract in two languages: the renderer invokes a string and
// Rust declares a function and registers it. Getting either half wrong is not a compile
// error anywhere, it is a command that fails at the moment a user clicks it. So every name
// the renderer invokes must exist as a command function and be registered in the handler.
const rendererSources = sourceFiles(join(root, "apps/desktop/src/backend"));
const invoked = new Set();
for (const path of rendererSources) {
  const source = readFileSync(path, "utf8");
  for (const match of source.matchAll(/invokeCommand(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)) {
    invoked.add(match[1]);
  }
}
if (invoked.size < MIN_RENDERER_COMMANDS) {
  violations.push(
    `only ${invoked.size} renderer commands were found; the invocation scan expects at least ${MIN_RENDERER_COMMANDS}`
  );
}
const declared = new Set();
for (const path of sourceFiles(join(root, "apps/desktop/src-tauri/src"))) {
  if (!path.endsWith(".rs")) continue;
  const source = readFileSync(path, "utf8");
  for (const match of source.matchAll(/pub (?:async )?fn ([a-z0-9_]+)\s*\(/g)) declared.add(match[1]);
}
const handler = readFileSync(join(root, "apps/desktop/src-tauri/src/lib.rs"), "utf8");
const registered = new Set(
  [...handler.matchAll(/commands::([a-z0-9_:]+)/g)].map((match) => match[1].split(":").pop())
);
for (const name of invoked) {
  if (!declared.has(name)) violations.push(`renderer invokes ${name}, which no Rust command declares`);
  if (!registered.has(name)) violations.push(`renderer invokes ${name}, which the invoke handler does not register`);
}

if (violations.length) {
  console.error("desktop command/state ownership contract failed:\n" + violations.map((v) => `  ${v}`).join("\n"));
  process.exit(1);
}
console.log("command snapshot contract ok");
