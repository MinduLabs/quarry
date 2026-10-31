#!/bin/sh
# Prints the CHANGELOG.md section of version $1, the notes of its GitHub Release. Fails when the
# changelog has no section for it, so a tag cannot ship without its notes.
set -eu
root=$(dirname "$0")/..
notes=$(awk -v heading="## [$1]" '
  index($0, heading) == 1 { on = 1; next }
  on && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  on { print }
' "$root/CHANGELOG.md")
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no notes under \"## [$1]\"" >&2
  exit 1
fi
printf '%s\n' "$notes"
