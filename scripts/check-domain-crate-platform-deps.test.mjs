#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BANNED_PLATFORM_DEPS,
  scanManifest
} from "./check-domain-crate-platform-deps.mjs";

test("scans normal, renamed, and target-specific dependency tables", () => {
  const manifest = `
[dependencies]
serde = "1"
runtime = { package = "tokio", version = "1" }

[target.'cfg(windows)'.dependencies]
windows-sys = "0.60"
`;
  assert.deepEqual(scanManifest(manifest, BANNED_PLATFORM_DEPS), [
    {
      kind: "package-alias",
      dep: "tokio",
      line: 'runtime = { package = "tokio", version = "1" }'
    },
    { kind: "name", dep: "windows-sys", line: 'windows-sys = "0.60"' }
  ]);
});

test("ignores package metadata and allowed pure dependencies", () => {
  const manifest = `
[package]
name = "tokio"

[dependencies]
serde = "1"
koushi-state = { path = "../koushi-state" }
`;
  assert.deepEqual(scanManifest(manifest, BANNED_PLATFORM_DEPS), []);
});
