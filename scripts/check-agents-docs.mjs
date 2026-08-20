#!/usr/bin/env node
// Guards the AGENTS.md -> docs/agents/ hierarchy.
//
// AGENTS.md is loaded into every agent session, so its size is a per-session
// cost and the tree only stays useful if the routing survives future edits.
// This gate enforces the rules recorded in the "Keeping these notes
// maintainable" section of AGENTS.md:
//
//   1. AGENTS.md stays within its line budget.
//   2. Every docs/agents/*.md file is linked from AGENTS.md, and every
//      docs/agents link in AGENTS.md resolves.
//   3. Retired QA flags never appear in a runnable position (a fenced code
//      block). docs/agents/history.md is the deliberate quarantine and is
//      exempt.
//   4. Every --scenario=<name> and every `local-*` scenario named in the lane
//      catalog exists in the runners, which are the single source of truth.
//
// Run standalone: node scripts/check-agents-docs.mjs
// Wired into: npm --prefix apps/desktop run lint

import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const routerPath = join(repoRoot, "AGENTS.md");
const treeDir = join(repoRoot, "docs/agents");
const quarantine = "docs/agents/history.md";

const ROUTER_LINE_BUDGET = 240;

const RETIRED = [
  ["--server=conduit", "no lane supports conduit; use --server=tuwunel"],
  ["--core-backend", "the runner rejects it: one Sliding Sync engine"],
  ["KOUSHI_QA_FORCE_SYNC_BACKEND", "backend forcing was removed"],
  ["--scenario=timeline_legacy", "the runner rejects timeline_legacy_* scenarios"]
];

const problems = [];
const fail = (file, message) => problems.push(`${file}: ${message}`);
const rel = (path) => relative(repoRoot, path);

// ---------------------------------------------------- authoritative scenarios
function guiScenarios() {
  const out = execFileSync(
    process.execPath,
    [join(repoRoot, "scripts/desktop-linux-gui-qa.mjs"), "--list"],
    { encoding: "utf8", timeout: 60_000 }
  );
  const names = new Set();
  for (const line of out.split("\n")) {
    const match = /^scenario (\S+)$/.exec(line.trim());
    if (match) names.add(match[1]);
  }
  if (names.size === 0) {
    throw new Error("desktop-linux-gui-qa.mjs --list produced no scenarios");
  }
  return names;
}

function coreScenarios() {
  const names = new Set();
  for (const bin of ["headless_core_qa/registry.rs", "real-homeserver-qa.rs"]) {
    const source = readFileSync(
      join(repoRoot, "crates/koushi-core/src/bin", bin),
      "utf8"
    );
    const re = /(?:Some\()?"([a-z_0-9]+)"\)? *=> *Ok\(Self::/g;
    let match;
    while ((match = re.exec(source)) !== null) names.add(match[1]);
  }
  if (names.size === 0) {
    throw new Error("no core QA scenarios found in crates/koushi-core/src/bin");
  }
  return names;
}

let gui;
let core;
try {
  gui = guiScenarios();
  core = coreScenarios();
} catch (error) {
  console.error(`check-agents-docs: cannot resolve scenario lists: ${error.message}`);
  process.exit(2);
}

// --------------------------------------------------------------- gather files
const router = readFileSync(routerPath, "utf8");
const treeFiles = readdirSync(treeDir)
  .filter((name) => name.endsWith(".md"))
  .sort()
  .map((name) => join(treeDir, name));

// 1. router size
const routerLines = router.split("\n").length;
if (routerLines > ROUTER_LINE_BUDGET) {
  fail(
    "AGENTS.md",
    `${routerLines} lines exceeds the ${ROUTER_LINE_BUDGET}-line budget. ` +
      "Move the new note into the matching docs/agents/ file instead of growing " +
      "the file every session pays for."
  );
}

// 2. link integrity, both directions
for (const file of treeFiles) {
  const link = `docs/agents/${rel(file).split("/").pop()}`;
  if (!router.includes(link)) {
    fail("AGENTS.md", `${link} exists but is not linked. An unlinked file is invisible.`);
  }
}
for (const match of router.matchAll(/\(docs\/agents\/([A-Za-z0-9._-]+)\)/g)) {
  const target = join(treeDir, match[1].split("#")[0]);
  if (match[1] !== "" && !treeFiles.includes(target)) {
    fail("AGENTS.md", `links to docs/agents/${match[1]}, which does not exist`);
  }
}

// 3. retired flags in runnable positions, and 4. scenario validity
const fencedBlocks = (text) => [...text.matchAll(/```[a-z]*\n([\s\S]*?)```/g)].map((m) => m[1]);

for (const file of [routerPath, ...treeFiles]) {
  const name = rel(file);
  if (name === quarantine) continue;
  const text = readFileSync(file, "utf8");

  for (const block of fencedBlocks(text)) {
    for (const [flag, why] of RETIRED) {
      if (block.includes(flag)) {
        fail(name, `runnable command contains retired \`${flag}\` — ${why}`);
      }
    }
  }

  for (const match of text.matchAll(/--scenario=([A-Za-z0-9_-]+)/g)) {
    const scenario = match[1];
    const known = scenario.startsWith("local-") || scenario === "signed-out"
      ? gui.has(scenario)
      : core.has(scenario);
    if (!known) fail(name, `--scenario=${scenario} is not a scenario any runner accepts`);
  }
}

// 4b. the lane catalog names GUI scenarios in table cells; those must be real too
const catalog = join(treeDir, "qa-lanes.md");
for (const line of readFileSync(catalog, "utf8").split("\n")) {
  if (!line.startsWith("|")) continue;
  for (const match of line.matchAll(/`(local-[a-z0-9-]+|signed-out)`/g)) {
    if (!gui.has(match[1])) {
      fail(rel(catalog), `table names \`${match[1]}\`, which is not in --list`);
    }
  }
}

// -------------------------------------------------------------------- report
if (problems.length > 0) {
  console.error("check-agents-docs: the AGENTS.md hierarchy is out of contract\n");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error(
    "\nSee the \"Keeping these notes maintainable\" section of AGENTS.md."
  );
  process.exit(1);
}

console.log(
  `agents docs ok: AGENTS.md ${routerLines}/${ROUTER_LINE_BUDGET} lines, ` +
    `${treeFiles.length} topic files linked`
);
