# Project Website Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish a five-language static site at `https://shinaoka.github.io/koushi-matrix/` with generated screenshots, rebuilt on every stable release and on demand.

**Architecture:** `site/` holds per-locale JSON copy, a Node generator, and a translation check. Screenshots come from a new Playwright lane over the existing React app harness (`apps/desktop/e2e-docs/site/`). The search scenes are fed from a Rust test that runs the production `run_sdk_query` path over a synthetic corpus and checks a golden JSON. A reusable `site.yml` workflow builds and deploys. `release-desktop.yml` calls it after stable releases, and it also accepts `workflow_dispatch`.

**Tech Stack:** Node 24 ESM scripts (`node:test`), Playwright 1.60.0 in `mcr.microsoft.com/playwright:v1.60.0-noble`, Rust (`koushi-core` tests with `MatrixMockServer`), GitHub Pages (`actions/upload-pages-artifact`, `actions/deploy-pages`).

**Spec:** `docs/superpowers/specs/2026-10-17-project-website-design.md`

## Global Constraints

- Locales, exactly: `en`, `ja`, `zh-Hans`, `zh-Hant`, `ko`. Output paths: `/`, `/ja/`, `/zh-Hans/`, `/zh-Hant/`, `/ko/`.
- All asset URLs relative (the site lives under `/koushi-matrix/`).
- Synthetic data only: `example.invalid` / `*.local` identifiers, no real accounts.
- Site claims must appear in README or `docs/help/` (prerequisite doc fixes are in Task 1).
- Builds and tests use at most 4 cores (`CARGO_BUILD_JOBS=4`, Playwright `workers: 1`).
- Blog links: en/zh/ko → `https://shinaoka.github.io/blog/why-matrix/`, ja → `https://shinaoka.github.io/blog/naze-matrix-ja/`. Setup checklist: en/zh/ko `…/blog/matrix-encryption-setup-checklist/`, ja `…/blog/matrix-encryption-setup-checklist-ja/`.
- Help links point at the release tag: `https://github.com/shinaoka/koushi-matrix/blob/<tag>/docs/help/<page>.md`.
- Downloads use `https://github.com/shinaoka/koushi-matrix/releases/latest/download/<asset>` with the README asset names.

## Review Focus

- Stale translation: a non-English entry whose `source_hash` mismatches English → the page renders English for that key and shows the "being translated" note, and the build exits 0. Pinned in Task 3 (`renders English and the translating note for a stale key`).
- Missing key in a locale → build and check exit non-zero naming locale and key. Pinned in Task 3.
- CJK glyphs missing in the container → the screenshot spec fails before writing PNGs. Pinned in Task 4 (font assertion).
- Search corpus drift: a code change makes "Koushi" stop matching a language → the Rust golden test fails in normal `cargo test`. Pinned in Task 2.
- `app_ref` tag lacking the site lane → the workflow fails with a clear message instead of publishing partial output. Pinned in Task 5 (workflow guard step).

---

### Task 1: Prerequisite README/help fixes

**Files:**
- Modify: `README.md` (Why Koushi multilingual bullet; Features list)
- Modify: `docs/help/getting-started.md` (one line: no account yet → matrix.org is one option)
- Modify: `docs/help/security-and-recovery.md` (one sentence: late-arriving keys re-show messages automatically)

- [ ] Step 1: Edit the README. Change "The interface is available in English and Japanese." to list English, Japanese, Simplified Chinese, Traditional Chinese, and Korean. Add Features bullets: "Notifications sent from this device by email (Resend or SMTP) or WeChat (ServerChan), even when your homeserver sends no email" and "Image compression choice before upload".
- [ ] Step 2: Edit the help pages (one sentence each, in the existing tone).
- [ ] Step 3: Run `node scripts/user-help.mjs --check` (or the `user-help.test.mjs` suite) and `node scripts/check-agents-docs.mjs`. Expected: pass. Regenerate `llms.txt` if the script reports drift.
- [ ] Step 4: Commit `docs: align README and help with site claims`.

### Task 2: Real search scenes (Rust)

**Files:**
- Create: `site/fixtures/search-corpus.json`: `{ "rooms": [{ "room_id", "messages": [{ "event_id", "sender", "body", "origin_server_ts" }] }], "scenes": [{ "id", "query", "expect_rooms_min": n, "expect_event_ids": [...] }] }`
- Create: `site/generated/search-scenes.json`: `{ "<scene id>": { "query", "results": [koushi_state::SearchResult, ...] } }` (golden, written by the test)
- Create: `crates/koushi-core/src/search/site_scenes.rs` (`#[cfg(test)] mod site_scenes;` in `search.rs`)
- Modify: `scripts/check-rust-test-structure.mjs`: add both JSON paths to `ALLOWED_NON_RUST_TARGETS`

**Interfaces:**
- Produces: `site/generated/search-scenes.json` consumed by Task 4. Each `results` array is serde JSON of `Vec<koushi_state::SearchResult>` in returned order.

- [ ] Step 1: Write the corpus. Rooms: `!site-ja:example.invalid` (Japanese), `!site-en:example.invalid` (English), `!site-mixed:example.invalid` (Chinese, Korean and English in one room, including one message mixing two scripts). Scenes: `koushi` (query `Koushi`, hits in ≥3 rooms), `width` (query `ﾃｽﾄ` matching a full-width `テスト` message).
- [ ] Step 2: Write `#[tokio::test] async fn site_search_scenes_match_the_golden_results()`. It builds the client like `history_scale.rs::production_query_reports_cached_resolution_failure_instead_of_success`, syncs each room, calls `koushi_sdk::index_room_events_now`, then for each scene calls `run_sdk_query` with `SearchScope::AllRooms` / matching `SearchRoomFilter` / `MatrixSearchScope` and `crate::search::cjk_search_query_variants(query)` (or the production variant helper used by `submit`). It asserts `expect_rooms_min` and `expect_event_ids`. It then compares the serialized map with the golden file. When `KOUSHI_UPDATE_SITE_GOLDEN=1` is set, it writes the golden instead.
- [ ] Step 3: Run `CARGO_BUILD_JOBS=4 KOUSHI_UPDATE_SITE_GOLDEN=1 cargo test -p koushi-core site_search_scenes`, then run again without the variable. Expected: PASS both times, golden written. Inspect it: no tmp paths, deterministic `timestamp_ms`.
- [ ] Step 4: Run `node scripts/check-rust-test-structure.mjs`. Expected: pass.
- [ ] Step 5: Commit `test(search): generate site search scenes from the production query path`.

### Task 3: Site content, generator, translation check

**Files:**
- Create: `site/content/en.json`: `{ "<key>": "<text>" }`, the source of truth
- Create: `site/content/{ja,zh-Hans,zh-Hant,ko}.json`: `{ "<key>": { "text": "...", "source_hash": "<sha256-12 of en text>" } }`
- Create: `site/lib/content.mjs`: `loadLocales(dir) -> {en, locales}`, `checkTranslations(en, locales) -> {errors: string[], stale: {locale: key[]}}`, `hashText(text) -> string` (first 12 hex chars of SHA-256)
- Create: `site/lib/render.mjs`: `renderPage({locale, strings, staleKeys, release, screenshots}) -> string`
- Create: `site/build.mjs`: CLI `node site/build.mjs --tag vX.Y.Z [--screenshots dir] [--out site/dist]`
- Create: `site/check-translations.mjs`: exits 1 on errors, prints stale as warnings
- Create: `site/template/style.css`, `site/README.md`
- Create: `site/lib/content.test.mjs`, `site/lib/render.test.mjs` (node:test)
- Modify: root test wiring so `node --test site/lib/*.test.mjs` and `check-translations` run in CI's existing node checks job

**Interfaces:**
- Consumes: `site/dist/screenshots/<locale>/<scene>.png` (Task 4). Scene ids: `main`, `notifications`, `search-mixed`, `search-width`, `settings`, `languages`, `rich`.
- Produces: `site/dist/**` static output, `site/dist/llms.txt` (copy of root `llms.txt`).

- [ ] Step 1: Write failing tests:
  - `checkTranslations` reports `ja: missing key hero.title` when the key is absent, and reports an extra key.
  - A stale `source_hash` lands in `stale.ja`, not in `errors`.
  - `renderPage` for `ja` with `staleKeys=["hero.title"]` contains the English `hero.title` text and the `translating` note.
  - Every page has `<html lang>` matching the locale and five `hreflang` alternates.
  - No `href`/`src` starts with `/`.
  - The help link contains `/blob/v0.26.0/docs/help/`.
  - The build refuses (throws) when any string contains `TODO`.
- [ ] Step 2: Run `node --test site/lib`. Expected: FAIL (modules missing).
- [ ] Step 3: Implement the modules. The page sections follow the spec "Page structure" 1–8. The pain-point list is the spec table. Copy in `en.json` is derived from README/help wording.
- [ ] Step 4: Write the four translations with `source_hash` from `hashText`. Footer note: native-speaker corrections welcome.
- [ ] Step 5: Run `node --test site/lib && node site/check-translations.mjs && node site/build.mjs --tag v0.26.0`. Expected: PASS, `site/dist/index.html` plus four locale dirs. Open in a browser at phone and desktop width.
- [ ] Step 6: Commit `feat(site): five-language static site generator`.

### Task 4: Screenshot lane

**Files:**
- Modify: `apps/desktop/e2e-docs/readmeFixture.ts`: accept an optional `FixtureText` (space/room/person names and message bodies). The default reproduces today's English strings exactly, so the README PNG is unchanged.
- Create: `apps/desktop/e2e-docs/site/siteText.ts`: `FixtureText` per locale
- Create: `apps/desktop/e2e-docs/site/site-screenshots.spec.ts`: one `test.describe` per locale, one test per scene
- Create: `apps/desktop/playwright.site.config.ts`: `testDir: ./e2e-docs/site`, port 5185, otherwise as `playwright.docs.config.ts`
- Modify: `apps/desktop/package.json`: `"site:screenshots": "playwright test --config=playwright.site.config.ts"`
- Modify: `apps/desktop/playwright.docs.config.ts`: exclude `site/` (`testIgnore: "site/**"`)

**Interfaces:**
- Consumes: `site/generated/search-scenes.json` (Task 2), harness `window.__harness` (`invoke("update_settings", {patch:{locale:{language_tag, text_direction:"auto"}}})`, `pushStateUpdate`, `pushCoreEvent`, `pushAccountTabs`, `setCommandResponse`).
- Produces: `site/dist/screenshots/<locale>/<scene>.png`, 2560×1600 (or a cropped region for detail scenes).

- [ ] Step 1: Refactor `readmeFixture.ts`. Run `npm --prefix apps/desktop run docs:screenshot` in the pinned container and `git diff --stat assets/screenshots`. Expected: no PNG change.
- [ ] Step 2: Write the site spec. Each test sets the locale, pushes the localized fixture, drives the UI to the scene (settings dialog → Notifications → "Sent from this device" with three synthetic sinks: Resend, SMTP, ServerChan; search panel from the golden results with room names mapped; App Settings → Appearance language list; a message with code, math and Markdown), and asserts a scene-specific locator. Before capture, assert CJK glyphs render: `document.fonts.check` is not reliable for system fonts, so compare `measureText` width of `"漢字한글"` in the UI font against a known tofu font and require a difference.
- [ ] Step 3: Run in the container (installing `fonts-noto-cjk` first). Expected: 35 PNGs, all checks pass. Inspect a sample per locale visually.
- [ ] Step 4: Run `npm --prefix apps/desktop run typecheck && npm --prefix apps/desktop run lint`. Expected: pass.
- [ ] Step 5: Commit `feat(site): localized screenshot lane`.

### Task 5: Workflow, docs, bootstrap

**Files:**
- Create: `.github/workflows/site.yml`: `on: workflow_dispatch: inputs: app_ref (default: latest stable tag, resolved by `gh release view --json tagName`)` and `workflow_call: inputs: tag`. Jobs: `build` (container, checks out `main` into `.`, `app_ref` into `app/`, guard step fails if `app/apps/desktop/e2e-docs/site` is missing, installs `fonts-noto-cjk`, runs the screenshot lane against `app/`, copies PNGs, runs `node site/check-translations.mjs`, `node site/build.mjs --tag $TAG`, then `upload-pages-artifact`) and `deploy` (`deploy-pages`, `environment: github-pages`). Permissions: `pages: write`, `id-token: write`, `contents: read`.
- Modify: `.github/workflows/release-desktop.yml`: job `site: needs: publish-release, if: needs.prepare.outputs.prerelease != 'true', uses: ./.github/workflows/site.yml, with: tag`. It must not affect release success (`continue-on-error` is not allowed on reusable-workflow jobs, so it runs as a downstream job only).
- Modify: `.github/workflows/ci.yml`: node checks job runs `node --test site/lib` and `node site/check-translations.mjs`; Rust jobs already run Task 2's test.
- Modify: `docs/agents/qa-lanes.md` (site screenshot lane section), `site/README.md` (local workflow, "main copy describes released features only", translation refresh procedure)

- [ ] Step 1: Write the workflows. Run `actionlint` if available, else `npx --yes @action-validator/cli` on both files. Expected: no errors.
- [ ] Step 2: Run `node scripts/check-agents-docs.mjs`. Expected: pass.
- [ ] Step 3: Commit `ci(site): publish the website on stable releases and on demand`.
- [ ] Step 4: Push, open the PR, watch CI until green, merge.
- [ ] Step 5: Bootstrap publish: `gh workflow run site.yml -f app_ref=main` (main == v0.26.0 + this PR; check that `git log v0.26.0..origin/main` has no product changes besides this PR). Watch until deployed. Then fetch `https://shinaoka.github.io/koushi-matrix/` and `/ja/`. Expected: 200 and screenshots load.
