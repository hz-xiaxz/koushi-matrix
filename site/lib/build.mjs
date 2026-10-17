// Builds site/dist from per-locale content, the template stylesheet, brand assets,
// and the screenshots produced by the site screenshot lane.
import { copyFileSync, cpSync, mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { checkTranslations, loadLocales, LOCALES, resolveStrings } from "./content.mjs";
import { renderPage, SCENES } from "./render.mjs";

const siteRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(siteRoot, "..");

export function buildSite({ contentDir, outDir, screenshotsDir, release }) {
  const { en, locales } = loadLocales(contentDir);
  const { errors, stale } = checkTranslations(en, locales);
  if (errors.length > 0) {
    throw new Error(`translation errors:\n${errors.join("\n")}`);
  }
  for (const [key, text] of Object.entries(en)) {
    if (/\bTODO\b/.test(text)) {
      throw new Error(`en: ${key} still contains a TODO placeholder`);
    }
  }
  for (const locale of LOCALES) {
    for (const scene of SCENES) {
      const file = path.join(screenshotsDir, locale, `${scene}.png`);
      let size = 0;
      try {
        size = statSync(file).size;
      } catch {
        // reported below
      }
      if (size === 0) {
        throw new Error(`missing screenshot ${locale}/${scene}.png in ${screenshotsDir}`);
      }
    }
  }

  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  for (const locale of LOCALES) {
    const staleKeys = stale[locale] ?? [];
    const html = renderPage({
      locale,
      strings: resolveStrings(en, locales, locale),
      staleKeys,
      englishStrings: en,
      release
    });
    const dir = locale === "en" ? outDir : path.join(outDir, locale);
    mkdirSync(dir, { recursive: true });
    writeFileSync(path.join(dir, "index.html"), html);
  }
  copyFileSync(path.join(siteRoot, "template", "style.css"), path.join(outDir, "style.css"));
  copyFileSync(path.join(repoRoot, "assets", "branding", "koushi-photon.svg"), path.join(outDir, "koushi-photon.svg"));
  copyFileSync(path.join(repoRoot, "assets", "branding", "favicon-32.png"), path.join(outDir, "favicon-32.png"));
  copyFileSync(path.join(repoRoot, "llms.txt"), path.join(outDir, "llms.txt"));
  cpSync(screenshotsDir, path.join(outDir, "screenshots"), { recursive: true });
  writeFileSync(path.join(outDir, ".nojekyll"), "");
  return { stale };
}
