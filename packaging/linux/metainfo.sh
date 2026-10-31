#!/bin/sh
# Writes the metainfo to $1 with a <release> for this version, dated today (UTC), so the version is
# never written by hand in the metainfo.
set -eu
here=$(dirname "$0")
version=$(sh "$here/../version.sh")
date=$(date -u +%Y-%m-%d)
mkdir -p "$(dirname "$1")"
sed "s|<releases/>|<releases>\n    <release version=\"$version\" date=\"$date\"/>\n  </releases>|" \
  "$here/io.github.mindulabs.Quarry.metainfo.xml" > "$1"
grep -q "<release version=\"$version\"" "$1"
