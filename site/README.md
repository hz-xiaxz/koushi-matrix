# Koushi website

Static, five-language site published at <https://shinaoka.github.io/koushi-matrix/>.
Design: [website design](../docs/superpowers/specs/2026-10-17-project-website-design.md).

## Layout

| Path | What it is |
| --- | --- |
| `content/en.json` | English copy, the source of truth (`key → text`). |
| `content/{ja,zh-Hans,zh-Hant,ko}.json` | Translations: `key → {text, source_hash}`. |
| `lib/` | Loader, translation check, renderer, builder, and their `node:test` suites. |
| `template/style.css` | The only stylesheet. |
| `fixtures/search-corpus.json` | Synthetic mixed-language messages and search scenes. |
| `generated/search-scenes.json` | Search results from the production query path (golden; do not edit). |
| `screenshots/`, `dist/` | Generated, git-ignored. |

## Rules

- **Copy on `main` describes released features only.** The site is published
  from `main` on demand, while screenshots come from a release tag. A feature
  that has not shipped in a stable release must not appear in `content/`.
- Every claim must also appear in the README or `docs/help/`. Change them
  together.
- All data in screenshots is synthetic.

## Change copy or translations

1. Edit `content/en.json`.
2. `node site/check-translations.mjs` lists each locale's stale keys. Stale
   keys still publish, shown in English with a "being translated" note.
3. Refresh the translations locally (for example, ask Claude Code to
   translate the stale keys into a plain `{key: text}` JSON file), review
   them, and record them:

   ```bash
   node site/stamp-translations.mjs ja hero.lede pain.email.answer --from /path/to/ja-updates.json
   ```

   Without `--from`, the command only re-stamps the existing text as current.
   Use that when an English edit did not change the meaning.

## Search scenes

The two search screenshots render `generated/search-scenes.json`. A
`koushi-core` test (`search::site_scenes`) indexes the corpus through the
encrypted ngram index, queries it with `run_sdk_query`, asserts the expected
hits, and compares the result with the golden file in normal `cargo test`.
After changing the corpus, regenerate it:

```bash
cargo test -p koushi-core --lib write_site_search_golden -- --ignored
```

## Build locally

Screenshots come from the pinned Playwright container with CJK fonts (see
[QA lanes](../docs/agents/qa-lanes.md#website-screenshot-lane)):

```bash
docker run -i --rm --init --shm-size=2g --user 0:0 \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -v "$PWD:/work" -w /work mcr.microsoft.com/playwright:v1.60.0-noble \
  bash -e -u -o pipefail -s <<'SH'
trap 'chown -R "$HOST_UID:$HOST_GID" /work/apps/desktop/node_modules /work/apps/desktop/test-results /work/site/screenshots 2>/dev/null || true' EXIT
apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq fonts-noto-cjk
npm --prefix apps/desktop ci
npm --prefix apps/desktop run site:screenshots
SH
node site/build.mjs --tag v0.26.0
```

Open `site/dist/index.html` in a browser.

## Publishing

`.github/workflows/site.yml` builds and deploys to GitHub Pages:

- automatically after every stable release (called from `release-desktop.yml`);
- manually from **Actions → Publish website → Run workflow**. Leave `app_ref`
  empty to screenshot the latest stable release, which is the normal choice
  for copy, layout, or translation fixes.

A failed run leaves the previous site online and never affects a release.
