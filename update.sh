#!/bin/sh
# Adds each upstream repo as a folder on first run, pulls its latest main afterwards,
# then records the pinned commit in README.md.
# Usage: ./update.sh            (all)
#        ./update.sh filmcraft  (just one)
set -e
cd "$(dirname "$0")"

REPOS="photocraft vectorcraft filmcraft lightcraft printcraft effectcraft designcraft"
start=$(git rev-parse HEAD)

for r in ${@:-$REPOS}; do
  url="https://github.com/storytold/$r.git"
  if [ -d "$r" ]; then
    echo "== updating $r"
    git subtree pull --prefix="$r" "$url" main -m "Update $r from upstream"
  else
    echo "== adding $r"
    git subtree add --prefix="$r" "$url" main
  fi

  # Rewrite the Commit and Commit date columns of this project's README row.
  sha=$(git rev-parse --short=9 FETCH_HEAD)
  day=$(git log -1 --format=%cs FETCH_HEAD)
  sed -i -E "s#^([|] \`$r/\` [|][^|]*[|][^|]*[|]).*#\1 \`$sha\` | $day |#" README.md
done

if ! git diff --quiet README.md; then
  sed -i -E "s#^\*\*Snapshot taken: .*\*\*#**Snapshot taken: $(date +%F)**#" README.md
  git commit -q -m "README: record updated versions" README.md
fi

echo "Done. To undo this update: git reset --hard $start"
