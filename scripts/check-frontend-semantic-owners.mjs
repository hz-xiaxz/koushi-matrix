#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const frontend = join(root, "apps/desktop/src");
const failures = [];

const forbiddenFiles = [
  "apps/desktop/src/backend/browserFakeApi.ts",
  "apps/desktop/src/backend/roomListProjection.ts",
  "apps/desktop/src/backend/browser-fake/sidebar.ts",
  "apps/desktop/src/backend/browser-fake/settings.ts",
  "apps/desktop/src/components/searchHighlight.ts"
];
for (const path of forbiddenFiles) {
  if (existsSync(join(root, path))) failures.push(`${path}: retired semantic owner still exists`);
}

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    if (![".ts", ".tsx"].includes(extname(path))) return [];
    if (/\.(test|spec)\.[^.]+$/.test(path) || path.includes(`${join("src", "test")}/`)) return [];
    return [path];
  });
}

for (const path of sourceFiles(frontend)) {
  const source = readFileSync(path, "utf8");
  const projectPath = relative(root, path);
  if (
    source.includes("localStorage") &&
    projectPath !== "apps/desktop/src/app/legacyPreferenceMigration.ts"
  ) {
    failures.push(`${projectPath}: localStorage is allowed only in legacyPreferenceMigration.ts`);
  }
}

const avatarOwnerTokens = [
  "MAX_AVATAR_THUMBNAIL_ATTEMPTS",
  "avatarThumbnailFailureIsRetryable",
  "avatarRetryCountsRef",
  "memberAvatarRetryCountsRef"
];
for (const path of sourceFiles(frontend)) {
  const source = readFileSync(path, "utf8");
  const projectPath = relative(root, path);
  for (const token of avatarOwnerTokens) {
    if (source.includes(token)) failures.push(`${projectPath}: retired avatar retry owner ${token}`);
  }
}

const peoplePanelPath = join(frontend, "components/PeoplePanel.tsx");
const peoplePanelSource = readFileSync(peoplePanelPath, "utf8");
if (
  peoplePanelSource.includes("roomMemberRoleOptions") ||
  /power_?level\s*:\s*(?:0|50|100)/.test(peoplePanelSource) ||
  /\[(?:100\s*,\s*50\s*,\s*0|0\s*,\s*50\s*,\s*100)\]/.test(peoplePanelSource)
) {
  failures.push("apps/desktop/src/components/PeoplePanel.tsx: room role options must come from Rust");
}

const desktopApiPath = join(frontend, "backend/desktopApi.ts");
if (readFileSync(desktopApiPath, "utf8").includes("setRoomListProjection")) {
  failures.push("apps/desktop/src/backend/desktopApi.ts: fake-only setRoomListProjection remains");
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("frontend semantic owners ok");
