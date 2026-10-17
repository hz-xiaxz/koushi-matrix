#!/usr/bin/env bash
# Prepares fonts in the pinned Playwright container for the website screenshot
# lane. The image ships WenQuanYi Zen Hei and IPA Gothic, and Chromium falls back
# to them for every CJK locale, so Japanese, Korean and Chinese would all render
# in the wrong font. Install Noto Sans CJK and remove the two fallbacks.
set -euo pipefail
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq fonts-noto-cjk
DEBIAN_FRONTEND=noninteractive apt-get purge -y -qq fonts-wqy-zenhei fonts-ipafont-gothic
fc-cache -f
for lang in ja ko zh-cn zh-tw; do
  fc-list ":lang=$lang" family | grep -q "Noto Sans CJK" || { echo "No Noto Sans CJK font for $lang" >&2; exit 1; }
done
