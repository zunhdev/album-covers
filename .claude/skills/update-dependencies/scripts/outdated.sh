#!/usr/bin/env bash
# Lists the package's direct dependencies (normal, build and dev) with the
# locked version and the newest release on crates.io, and classifies the gap:
#
#   compatible  within the range in Cargo.toml: `cargo update` picks it up
#   MAJOR       a semver-breaking release: needs `cargo add <crate>@<version>`
#
# Run from anywhere inside the package. Needs network access to crates.io.
# Pre-releases are ignored, as `cargo search` reports the newest stable version.
set -euo pipefail

# The breaking component of a version: the major, or for 0.x the minor (0.0.x: the patch).
compat_key() {
  local major minor patch
  IFS=. read -r major minor patch <<<"${1%%[-+]*}"
  if [[ $major != 0 ]]; then echo "$major"
  elif [[ $minor != 0 ]]; then echo "0.$minor"
  else echo "0.0.$patch"; fi
}

printf '%-20s %-12s %-12s %s\n' CRATE LOCKED LATEST STATUS
cargo tree --depth 1 --prefix none --edges normal,build,dev 2>/dev/null |
  tail -n +2 | sed 's/ (proc-macro)//; s/ (\*)//' | sort -u |
  while read -r name version; do
    locked=${version#v}
    line=$(cargo search "$name" --limit 1 2>/dev/null | head -n 1)
    found=${line%% =*}
    latest=$(sed -n 's/^[^"]*"\([^"]*\)".*/\1/p' <<<"$line")
    if [[ $found != "$name" || -z $latest ]]; then
      status="? not found on crates.io"
    elif [[ $latest == "$locked" ]]; then
      status="up to date"
    elif [[ $(compat_key "$latest") != "$(compat_key "$locked")" ]]; then
      status="MAJOR"
    else
      status="compatible"
    fi
    printf '%-20s %-12s %-12s %s\n' "$name" "$locked" "$latest" "$status"
  done
