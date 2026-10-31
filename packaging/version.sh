#!/bin/sh
# Prints Quarry's version: `version` under [workspace.package] in Cargo.toml, the only place it is
# written. Builds, file names, the metainfo and the release tag check all read it from here.
set -eu
root=$(dirname "$0")/..
version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml")
if [ -z "$version" ]; then
  echo "No version under [workspace.package] in Cargo.toml" >&2
  exit 1
fi
echo "$version"
