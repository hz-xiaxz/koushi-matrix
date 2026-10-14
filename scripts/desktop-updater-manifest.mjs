#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";

const values = new Map();
const entries = [];
let entry;
for (let index = 2; index < process.argv.length; index += 2) {
  const name = process.argv[index];
  const value = process.argv[index + 1];
  if (!name?.startsWith("--") || value === undefined) {
    fail("arguments must be --name value pairs");
  }
  if (name === "--target") {
    entry = { target: value };
    entries.push(entry);
  } else if (name === "--url" || name === "--signature-file") {
    if (!entry) fail(`${name} must follow --target`);
    if (entry[name]) fail(`duplicate ${name}`);
    entry[name] = value;
  } else {
    if (!["--version", "--output"].includes(name) || values.has(name)) fail(`invalid argument ${name}`);
    values.set(name, value);
  }
}

const version = required("--version");
const output = required("--output");

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  fail(`invalid SemVer: ${version}`);
}
if (!entries.length) fail("missing --target");
const platforms = {};
for (const entry of entries) {
  const target = entry.target;
  if (!["darwin-aarch64", "windows-x86_64"].includes(target) || Object.hasOwn(platforms, target)) {
    fail(`invalid or duplicate target: ${target}`);
  }
  const url = entry["--url"];
  if (!url?.startsWith("https://")) fail("update URL must use HTTPS");
  if (!entry["--signature-file"]) fail("missing --signature-file");
  const signature = readFileSync(entry["--signature-file"], "utf8").trim();
  if (!signature) fail("signature must not be empty");
  platforms[target] = { signature, url };
}

writeFileSync(
  output,
  `${JSON.stringify({
    version,
    pub_date: new Date().toISOString(),
    platforms
  }, null, 2)}\n`,
  "utf8"
);

function required(name) {
  const value = values.get(name);
  if (!value) fail(`missing ${name}`);
  return value;
}

function fail(message) {
  console.error(`desktop-updater-manifest: ${message}`);
  process.exit(1);
}
