#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const commands = join(root, "apps/desktop/src-tauri/src/commands");
const forbidden = /\b(?:FrontendDesktopSnapshot|current_snapshot)\b/;
const allowed = new Set(["get_snapshot", "settlement_snapshot", "resync_snapshot"]);
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
    ...source.matchAll(/(?:pub(?:\([^)]*\))?\s+)?async fn ([a-z0-9_]+)[\s\S]*?\{/g)
  ];
  return matches.map((match) => {
    let depth = 1;
    let end = match.index + match[0].length;
    while (end < source.length && depth) {
      if (source[end] === "{") depth++;
      if (source[end] === "}") depth--;
      end++;
    }
    return { name: match[1], start: match.index, text: source.slice(match.index, end) };
  });
}

for (const name of readdirSync(commands).filter((name) => name.endsWith(".rs"))) {
  const path = join(commands, name);
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

if (violations.length) {
  console.error("desktop command/state ownership contract failed:\n" + violations.map((v) => `  ${v}`).join("\n"));
  process.exit(1);
}
console.log("command snapshot contract ok");
