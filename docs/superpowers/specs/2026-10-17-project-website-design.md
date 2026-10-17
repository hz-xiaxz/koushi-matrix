# Koushi project website — design

Status: draft for maintainer review (2026-10-17).

## Intent

Koushi needs a public web page that does two jobs:

1. **A link to hand to someone who has never heard of Matrix.** "Install
   this" should be enough: what Koushi is in one line, one screenshot, a
   download button, and a pointer to help for creating an account.
2. **A switching argument for people who already use Matrix.** They should
   recognise their own pain points in a list and see, for each one, how
   Koushi addresses it, with a screenshot.

Detailed settings walkthroughs are explicitly out of scope. Users can ask
ChatGPT or another AI assistant, which reads the site and the Markdown help
(`llms.txt`, `docs/help/`). The site shows an overview of what is
configurable and where the entry points are, and leaves detailed help to
those sources.

Audience framing follows the README: general users. Math rendering is one
of several rich-message features, not the headline.

## Goals

- One page per supported UI language: `en`, `ja`, `zh-Hans`, `zh-Hant`,
  `ko`, matching the in-app language selector.
- Copy derived from the README's "Why Koushi?" and "Features" sections, kept
  honest: the site never claims more than the README or help claims.
- Screenshots that are generated, not hand-captured. One command regenerates
  every screenshot for every language, and the images are deterministic.
- Automatic rebuild and publish to GitHub Pages
  (`https://shinaoka.github.io/koushi-matrix/`) on every stable release.

## Non-goals

- Per-setting documentation or screenshots of every settings page.
- A blog, changelog page, or news feed.
- Running AI in CI. Translations are updated locally (for example by asking
  Claude Code) and reviewed before merge.
- A custom domain.
- A documented Windows/Linux install guide beyond what the README already
  says (those builds stay labelled untested).

## Anti-goals (failure even if it "works")

- **Staged claims.** A screenshot that shows a search result the real
  search engine would not return. The mixed-language search shots must come
  from real `koushi-search` output (see below).
- **Overclaiming encryption reliability.** The site must not say
  "unable to decrypt never happens". It says Koushi is designed to make it
  rare, and names the mechanisms.
- **Advertising unreleased features.** The site is built from the release
  tag, never from `main`.
- **Unreviewed machine translation shipped as authoritative.** Non-English
  pages may start machine-translated, but each says corrections from native
  speakers are welcome, and CI flags stale translations rather than silently
  shipping English fallbacks.
- **Real data.** All screenshot content is synthetic; no real accounts,
  rooms, or homeservers (repository rule).

## Page structure (per language)

Order is a recommendation; the builder may adjust layout.

1. **Hero.** Wordmark, one-line pitch ("A desktop Matrix client for
   conversations in many languages"), main screenshot (account tabs + three
   panes), download button for macOS Apple Silicon. Other platforms are
   secondary links labelled untested, as in the README.
2. **What is Matrix? (one or two sentences).** Something like "an open chat
   network where you choose your server, like email". *(Maintainer's
   decision)* For a longer explanation, link to the maintainer's existing
   blog on `shinaoka.github.io`. The exact post URL is supplied by the
   maintainer; until then, leave a clearly marked placeholder that the
   build refuses to publish. Add a light pointer to creating an account at
   matrix.org ("no account yet? you can create one at matrix.org") without
   presenting it as the only choice. Mirror that one-line pointer in
   `docs/help/getting-started.md` so the site doesn't claim more than the
   help.
3. **"Sound familiar?"**: the pain-point list for existing Matrix users.
   Each item has a problem, how Koushi addresses it, and a screenshot or
   short visual:

   | Pain point | Koushi's answer | Visual |
   | --- | --- | --- |
   | Signing out and in to switch between work and personal accounts | Account tabs, all signed in and syncing, per-tab unread counts, composer shows the sending account | Main shot (account tabs) |
   | matrix.org never emails you about missed messages | Notifications sent from this device via **Resend**, **SMTP**, or **WeChat (ServerChan)**, with a wait period, digests, and a choice of how much content is included | Settings → Notifications → Sent from this device, destination list |
   | "Unable to decrypt" keeps appearing | Secure backup is completed at sign-in before you continue; the recovery key must be saved before proceeding; messages whose keys arrive late re-render automatically; backup diagnostics say whether a failure is network or server | Secure-backup / recovery-key screen |
   | Can't find old messages in encrypted rooms | Local full-text search over encrypted history, across all rooms / a Space / one room | Mixed-language search result (real engine) |
   | Rooms in different languages, or one room mixing several | Search is language-agnostic: Japanese, Chinese, Korean and Latin text in the same index, text without spaces between words, full-width/half-width folding | Second search shot (e.g. half-width query finding full-width text) |
   | The IME confirmation sends the message by accident | Candidate confirmation is separate from sending | Text only (builder may add a visual if one is cheap and honest) |
   | Phone photos are huge | Choose image quality / compression before sending | Composer with staged image and compression choice |
   | Getting lost in settings | Ask an AI assistant; the help is written for it (`llms.txt`) | Help & About settings page, or text only |

4. **Everyday features.** Short grid: Markdown, code blocks, LaTeX-style
   math, threads/replies/reactions, image and file uploads, system tray and
   native notifications, keyboard shortcuts, notification keywords, suggested
   Space rooms with bulk join, in-app updates on macOS/Windows, privacy-safe
   diagnostics report. One composite screenshot of a rich message is enough.
5. **Five UI languages.** The language selector screenshot, or simply the
   five language names in their own scripts.
6. **What you can configure.** A condensed version of the table in
   `docs/help/settings.md` (scope → category → one-line summary), plus one
   or two screenshots of the settings entry points (App Settings dialog with
   category list). Ends with "for step-by-step help, ask an AI assistant or
   read the user guide", linking to the guide **at the release tag**.
7. **Not included yet.** Same honest list as the README (calls, screen
   sharing, bots, widgets, integrations).
8. **Footer.** Community room `#koushi-matrix:matrix.org`, GitHub repo,
   license, contributors-wanted note for Windows/Linux, language switcher.

Every page carries a language switcher and `hreflang` alternates. The root
`/` is English. Builder's call: whether to show a non-blocking "this page is
available in your language" hint based on `navigator.language`. No
automatic redirect.

## Screenshots

### Mechanism

Extend the existing deterministic README screenshot lane
(`apps/desktop/e2e-docs/`, `playwright.docs.config.ts`,
`npm --prefix apps/desktop run docs:screenshot`; see
[QA lanes](../../agents/qa-lanes.md)). It renders the real React `<App />`
in the browser harness and pushes synthetic snapshots/events. That is the
right tool for UI shots: deterministic, fast, no homeserver.

- Add a site screenshot spec (or specs) parameterised by **locale**, which
  sets both the app UI language and the Playwright `locale`.
- **Fixture content is localised too.** Room names, people, and message
  text are written per language, so a Chinese UI does not show English
  chat. Each locale's fixture should still look like a believable
  multilingual workspace (the product's pitch), not a monolingual one.
- Scenes, roughly one per visual in the table above: main/account tabs,
  device-sent notification destinations, secure backup/recovery key, search
  (two scenes), image compression choice, rich message, language selector,
  settings entry point.
- The existing README screenshot (`assets/screenshots/koushi-main.png`) and
  its CI check stay as they are. The site lane is separate, and may reuse
  the README fixture builder where that helps.

### Real search results

The two search scenes must not use hand-written result DTOs.

Production search is layered: the SDK's encrypted ngram index
(`vendor/matrix-rust-sdk/crates/matrix-sdk-search`, configured through
`MatrixSearchIndexStoreConfig`) produces candidates; `SearchActor`
(`crates/koushi-core/src/search.rs`) resolves their current content; and
`koushi_search::verify_candidate()` plus `cjk_search_query_variants()`
filter and normalise them. "Real output" means running the queries through
that path.

1. A small, checked-in synthetic corpus of mixed-language messages across
   several rooms (one Japanese room, one English room, one room mixing
   Chinese, Korean and English, etc.).
2. A Rust step drives the real `SearchActor` against `MatrixMockServer`, the
   same way `crates/koushi-core/src/search/tests.rs` already does: it feeds
   the corpus through sync, runs the scene queries, and writes the
   result DTOs as JSON. *(Designer's call.)* If that proves disproportionate,
   using `matrix-sdk-search` directly plus `verify_candidate` is an
   acceptable fallback. Verifying every corpus message without the ngram
   index is **not** acceptable, because it doesn't show that the index
   would return those hits.
3. **The step asserts the expected hits.** For example, the "Koushi" query
   must return hits from rooms in at least three languages, and a half-width
   katakana query must find the full-width text. If an assertion fails, the
   generation fails.
4. The Playwright spec pushes that JSON into the harness as search-panel
   state.

### Determinism and fonts

- Run in the pinned Playwright container already used in CI
  (`mcr.microsoft.com/playwright:v1.60.0-noble`), at UTC, light theme, 2x
  scale, with reduced motion, matching the README lane.
- **CJK fonts must be present** in the container, or the zh/ko/ja shots
  render tofu. Install a pinned Noto CJK package (or vendor fonts) as part
  of the lane, and assert in the spec that CJK glyphs render. One cheap
  check is to compare the text's rendered width against a tofu baseline.
- Output: generated into a build directory (for example
  `site/dist/screenshots/<locale>/*.png`). Screenshots are **not
  committed**. The README image remains the only committed screenshot.
  (Builder's call, recorded here.)
- Optimise PNGs (or emit WebP alongside) so pages stay light. 2x images
  with explicit width/height attributes.

## Site source and build

- Location: a new top-level `site/` directory.
- No framework. A small Node `.mjs` generator, matching the repository's
  existing scripts, renders a shared HTML template and stylesheet once per
  locale from `site/content/<locale>.json` (or `.mjs`). English is the
  source of truth.
- Hosting note: `shinaoka.github.io` already serves the maintainer's blog
  (a user Pages site). This project site is served under the
  `/koushi-matrix/` path, so all asset URLs must be relative or
  base-path aware. Check that the blog has no `/koushi-matrix/` path of its own.
- Static output in `site/dist/`: `/index.html` (en), `/ja/`, `/zh-Hans/`,
  `/zh-Hant/`, `/ko/`, assets, screenshots. Also copy or link `llms.txt` so
  AI assistants that land on the site find the help index.
- **Version and download links** are filled in at build time from the
  release being published (tag, version). Download URLs use the existing
  `releases/latest/download/...` names from the README.
- Help links point at the **release tag** (`/blob/vX.Y.Z/docs/help/...`),
  consistent with the help's version-selection policy.
- Design: use the existing brand assets (`assets/branding/`) and the app's
  light/dark palette. Respect `prefers-color-scheme`. Works at phone
  width.

### Translation integrity check

A Node check covers two cases:

- **Missing or extra keys** fail the check, both in the normal lint path
  and at publish.
- **Stale entries** produce a warning in PR CI and do not block it. At
  publish, a locale page with stale entries still ships, shows the
  English text for those keys, and displays a visible note that parts of
  the page are being translated. *(Designer's call: English copy changes
  shouldn't require four translations in the same PR, and nothing goes
  stale silently.)* Mechanism suggestion: each non-English entry records a
  hash of the English text it was translated from, and the check compares
  it with the current English hash.

Updating translations is a local, human-reviewed step (for example asking
Claude Code to refresh the stale keys, then reviewing the diff).

## Publishing

- Triggers *(maintainer's decision: manual runs are required)*:
  - **Stable releases** (not pre-releases). Filter on the `prerelease`
    output, because `latest-beta` is updated on every release.
  - **Manual run** (`workflow_dispatch`), so site-only fixes (typos,
    layout, CSS, translations) go live without a desktop release.
- **Source split.** Site template, styles, generator and translations
  (`site/`) come from `main`. The app and the screenshot specs/fixtures
  come from the **latest stable release tag**, so screenshots never show
  unreleased UI. Rule for maintainers: site copy on `main` may describe only
  released features. Document this in `site/README.md`.
- **Workflow shape.** A dedicated workflow (for example
  `.github/workflows/site.yml`) with `workflow_dispatch` and
  `workflow_call`. `release-desktop.yml` calls it from a job
  `needs: publish-release`. Do **not** reuse `release-desktop.yml`'s own
  `workflow_dispatch`, which starts a full release attempt.
- Bootstrap: until a stable tag contains the site screenshot specs, the
  default manual run fails with a clear message rather than falling back to
  `main`. The manual run takes an explicit `app_ref` input, so the first
  publish can screenshot `main` when it matches the latest release apart
  from this work.
- **Constraint:** `release-desktop.yml` creates releases with `gh release
  create` using `GITHUB_TOKEN`, and events caused by `GITHUB_TOKEN` do
  **not** trigger other workflows. A separate `on: release: published`
  workflow would therefore never fire. Wire site publishing as a job
  `needs: publish-release` inside `release-desktop.yml` (skipped when
  `prerelease` is true), or as a reusable workflow it calls.
- Steps: check out the release tag, generate screenshots in the pinned
  container, run the search-assertion step, run the translation check,
  build `site/dist`, deploy with `actions/upload-pages-artifact` +
  `actions/deploy-pages`.
- Failure in any step leaves the previously published site untouched and
  must **not** fail or roll back the desktop release itself (site job is
  downstream and independent).
- One-time setup (manual, maintainer): enable GitHub Pages with source
  "GitHub Actions" in repository settings.

## Local workflow

- `npm --prefix apps/desktop run site:screenshots` (name is the builder's
  call) regenerates all locales' screenshots locally in the same container.
- `node site/build.mjs` (or similar) builds `site/dist/`. Opening it locally
  or with a static server previews the site.
- Document both in [QA lanes](../../agents/qa-lanes.md) (screenshot lane)
  and [environment](../../agents/environment.md) or a short `site/README.md`,
  following the AGENTS.md "one owner per fact" rule.

## Prerequisite doc fixes (part of this work)

The site must not claim more than the README or help. Bring those sources
up to date first, in the same change set:

- README "Why Koushi?": change "The interface is available in English and
  Japanese" to the five languages the app ships. The Features list gains
  notifications sent from the device (Resend/SMTP/WeChat) and image
  compression before upload.
- `docs/help/security-and-recovery.md`: add one sentence saying that
  messages whose room keys arrive later are shown automatically once
  decrypted. Code backs this: the SDK reports in-place UTD resolution, and
  Koushi re-renders and re-indexes the room (#1344). Otherwise the site's
  "Unable to decrypt" row would claim more than the help.
- "README or help" is the source of truth for site claims. A feature that
  appears only in help may appear on the site.

## Constraints

- Repository rules apply: synthetic data only, product text style, English
  for GitHub issues, builds/tests on at most 4 cores.
- The site must not contradict the README or help. When a feature claim
  changes, the README, help, and English site copy change together.
- Platform support wording mirrors the README (macOS official; Windows and
  Linux untested).

## Left to the builder

- Visual design, layout, typography within the brand assets.
- Exact scene list and fixture content, as long as every row of the
  pain-point table has an honest visual or explicit text-only treatment.
- Bridge format between the Rust search step and Playwright.
- Generator internals, file names, script names.
- Whether to add a navigator-language hint on the English root.

## Calls made by the designer (not yet confirmed by the maintainer)

- Static Node generator in `site/`, no framework.
- Screenshots generated, not committed.
- Root `/` is English; no auto-redirect.
- Site workflow is separate and reusable: `release-desktop.yml` calls it
  after a stable release, and it can also be run manually.
- Search shots use the real `SearchActor` with `MatrixMockServer`, with
  `matrix-sdk-search` + `verify_candidate` as a fallback.
- Stale translations warn in PRs and ship with a visible note.
- README and help get the prerequisite fixes above.
- Pain-point wording above is a draft. The maintainer approves final copy
  during implementation review.
