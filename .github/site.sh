#!/usr/bin/env bash
# The catalogue (`cargo xtask site`), from the registry's index and this
# checkout's extensions, pushed to gh-pages in place of what's there --
# nothing when it's the same. release.yml runs it after each release, and
# site.yml by hand. $1 names the commit's cause.
set -euo pipefail
tmp=${RUNNER_TEMP:-$(mktemp -d)}

git fetch --no-tags --depth=1 origin registry
git show FETCH_HEAD:index.json > "$tmp/index.json"
# The site's one date: the index's, so the same index makes the same site.
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct FETCH_HEAD)
export SOURCE_DATE_EPOCH
rm -rf "$tmp/site"
cargo xtask site "$tmp/index.json" "$tmp/site"

rm -rf "$tmp/gh-pages"
git worktree prune
if git ls-remote --exit-code --heads origin gh-pages > /dev/null; then
  git fetch --no-tags --depth=1 origin gh-pages
  git worktree add -B gh-pages "$tmp/gh-pages" FETCH_HEAD
else
  git worktree add --orphan -b gh-pages "$tmp/gh-pages"
fi
cd "$tmp/gh-pages"
find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$tmp/site/." .
git add -A
if git diff --cached --quiet; then
  echo "The catalogue is as it was"
  exit 0
fi
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git commit -q -m "Catalogue: ${1:-rebuilt}"
git push origin HEAD:gh-pages
