#!/bin/sh
# Adds each upstream repo as a folder on first run, pulls its latest main afterwards.
# Usage: ./update.sh            (all)
#        ./update.sh filmcraft  (just one)
set -e
cd "$(dirname "$0")"

REPOS="photocraft vectorcraft filmcraft lightcraft printcraft effectcraft designcraft"

for r in ${@:-$REPOS}; do
  url="https://github.com/storytold/$r.git"
  if [ -d "$r" ]; then
    echo "== updating $r"
    git subtree pull --prefix="$r" "$url" main -m "Update $r from upstream"
  else
    echo "== adding $r"
    git subtree add --prefix="$r" "$url" main
  fi
done
