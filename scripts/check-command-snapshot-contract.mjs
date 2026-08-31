#!/usr/bin/env node
import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const commands = join(root, "apps/desktop/src-tauri/src/commands");
const forbidden = /\b(?:FrontendDesktopSnapshot|current_snapshot)\b/;
const allowed = new Set(["get_snapshot", "settlement_snapshot", "resync_snapshot"]);
const violations = [];

function functions(source) {
  const matches = [...source.matchAll(/pub async fn ([a-z0-9_]+)[\s\S]*?\{/g)];
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
  if (name !== "session.rs" && forbidden.test(source)) {
    violations.push(relative(root, path));
    continue;
  }
  for (const fn of functions(source)) {
    if (!allowed.has(fn.name) && forbidden.test(fn.text)) {
      violations.push(`${relative(root, path)}:${source.slice(0, fn.start).split("\n").length} ${fn.name}`);
    }
  }
}

const helper = join(commands, "mod.rs");
if (/async fn current_snapshot\b/.test(readFileSync(helper, "utf8"))) {
  violations.push(`${relative(root, helper)} current_snapshot helper`);
}

if (violations.length) {
  console.error("normal Tauri commands must not return or read full snapshots:\n" + violations.map((v) => `  ${v}`).join("\n"));
  process.exit(1);
}
console.log("command snapshot contract ok");
