import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { buildSite } from "./build.mjs";
import { LOCALES } from "./content.mjs";
import { SCENES } from "./render.mjs";

const siteRoot = path.resolve(new URL("..", import.meta.url).pathname);
const release = { tag: "v0.26.0", version: "0.26.0" };

function fixture({ mutate } = {}) {
  const dir = mkdtempSync(path.join(tmpdir(), "koushi-site-"));
  const content = path.join(dir, "content");
  cpSync(path.join(siteRoot, "content"), content, { recursive: true });
  if (mutate) mutate(content);
  const shots = path.join(dir, "shots");
  for (const locale of LOCALES) {
    mkdirSync(path.join(shots, locale), { recursive: true });
    for (const scene of SCENES) writeFileSync(path.join(shots, locale, `${scene}.png`), "png");
  }
  return { dir, content, shots, out: path.join(dir, "dist") };
}

test("builds five pages, assets, screenshots and llms.txt", () => {
  const f = fixture();
  const result = buildSite({ contentDir: f.content, outDir: f.out, screenshotsDir: f.shots, release });
  for (const locale of LOCALES) {
    const page = path.join(f.out, locale === "en" ? "" : locale, "index.html");
    assert.ok(existsSync(page), page);
  }
  for (const file of ["style.css", "koushi-photon.svg", "favicon-32.png", "llms.txt", "screenshots/ko/search-mixed.png"]) {
    assert.ok(existsSync(path.join(f.out, file)), file);
  }
  assert.deepEqual(result.stale, {});
});

test("a missing translation key fails the build", () => {
  const f = fixture({
    mutate: (content) => {
      const ja = JSON.parse(readFileSync(path.join(content, "ja.json"), "utf8"));
      delete ja["hero.lede"];
      writeFileSync(path.join(content, "ja.json"), JSON.stringify(ja));
    }
  });
  assert.throws(() => buildSite({ contentDir: f.content, outDir: f.out, screenshotsDir: f.shots, release }), /ja: missing key hero\.lede/);
});

test("a stale translation builds with English and the note", () => {
  const f = fixture({
    mutate: (content) => {
      const ko = JSON.parse(readFileSync(path.join(content, "ko.json"), "utf8"));
      ko["hero.lede"].source_hash = "000000000000";
      writeFileSync(path.join(content, "ko.json"), JSON.stringify(ko));
    }
  });
  const result = buildSite({ contentDir: f.content, outDir: f.out, screenshotsDir: f.shots, release });
  assert.deepEqual(result.stale, { ko: ["hero.lede"] });
  const html = readFileSync(path.join(f.out, "ko", "index.html"), "utf8");
  assert.match(html, /class="translating-note"/);
});

test("a TODO placeholder refuses to publish", () => {
  const f = fixture({
    mutate: (content) => {
      const en = JSON.parse(readFileSync(path.join(content, "en.json"), "utf8"));
      en["matrix.body"] = "TODO write this";
      writeFileSync(path.join(content, "en.json"), JSON.stringify(en));
    }
  });
  assert.throws(() => buildSite({ contentDir: f.content, outDir: f.out, screenshotsDir: f.shots, release }), /TODO/);
});

test("a missing screenshot fails the build", () => {
  const f = fixture();
  writeFileSync(path.join(f.shots, "zh-Hant", "rich.png"), "");
  assert.throws(
    () => buildSite({ contentDir: f.content, outDir: f.out, screenshotsDir: f.shots, release }),
    /zh-Hant\/rich\.png/
  );
});
