import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { checkLinuxBuildDeps } from "./check-linux-build-deps.mjs";

test("reports missing compiler and pkg-config together without probing libraries", () => {
  const calls = [];
  const missing = checkLinuxBuildDeps((command, args) => {
    calls.push([command, args]);
    return { status: null, error: { code: "ENOENT" } };
  });
  assert.ok(missing.includes("cc (build-essential)"));
  assert.ok(missing.includes("pkg-config"));
  assert.ok(!calls.some(([, args]) => args.includes("--exists")));
});

test("reports the development package for an unavailable library", () => {
  const missing = checkLinuxBuildDeps((command, args) => ({
    status: command === "pkg-config" && args.includes("webkit2gtk-4.1") ? 1 : 0,
  }));
  assert.deepEqual(missing, ["webkit2gtk-4.1 (libwebkit2gtk-4.1-dev)"]);
});

test("accepts installed build dependencies", () => {
  assert.deepEqual(checkLinuxBuildDeps(() => ({ status: 0 })), []);
});

test("CLI exits unsuccessfully with actionable setup guidance when tools are absent", {
  skip: process.platform !== "linux",
}, () => {
  const result = spawnSync(process.execPath, [
    fileURLToPath(new URL("./check-linux-build-deps.mjs", import.meta.url)),
  ], { env: { ...process.env, PATH: "" }, encoding: "utf8", timeout: 10_000 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /cc \(build-essential\)/);
  assert.match(result.stderr, /pkg-config/);
  assert.match(result.stderr, /README\.md/);
});
