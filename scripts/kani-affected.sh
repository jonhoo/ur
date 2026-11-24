#!/usr/bin/env bash
# Names the host-test crates whose Kani proofs a change could affect, a
# line each: what .github/workflows/kani.yml runs the proofs of, so that
# a pull request waits for the proofs of what it touches and no others
# (all of them take over an hour; scripts/release-gate.sh runs them all
# before a release).
#
#     scripts/kani-affected.sh BASE [HEAD]   the change from BASE to HEAD
#                                            (default: the commit checked out)
#     scripts/kani-affected.sh --files       the paths on standard input,
#                                            a line each, from the root
#     scripts/kani-affected.sh --all         every crate with proofs
#
# A crate is affected by a changed path that is:
#
# - under its own directory (host-tests/CRATE/: its tests and proofs, its
#   Cargo.toml and Cargo.lock);
# - a firmware module it includes: `#[path = "../../../src/X.rs"]` in its
#   sources names src/X.rs, and src/X/ with it, where the module's own
#   `mod`s would be; a `mod.rs` brings its whole directory (the themes'
#   colour schemes);
# - under a directory its Cargo.toml depends on by `path` (gbmock/, for
#   host-tests/gb).
#
# And every crate is affected by a change to what decides how the proofs
# run at all: the workflow (Kani's version and flags), and this script.
#
# The crates and what they include are read from the working tree, which
# should be HEAD's: a crate the change deletes has no proofs left to run,
# and one it adds is affected by its own directory. Not followed: what a
# module includes by some other means (`include_str!` of a file outside
# its crate, a build script); nothing does today.
# TODO: follow those if a host-test crate ever grows one.
set -euo pipefail

usage() {
  sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$0"
}

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# What changes everything.
always=(.github/workflows/kani.yml scripts/kani-affected.sh)

mapfile -t crates < <(grep -rl 'kani::proof' host-tests/*/src | cut -d/ -f2 | sort -u)

case ${1-} in
  --all)
    printf '%s\n' "${crates[@]}"
    exit 0
    ;;
  --files)
    mapfile -t changed
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  "" | -*)
    usage >&2
    exit 2
    ;;
  *)
    # Both trees' paths of a rename (`--no-renames`): the old one is what
    # a crate may have included.
    mapfile -t changed < <(git diff --name-only --no-renames "$1" "${2:-HEAD}")
    ;;
esac

for path in "${changed[@]}"; do
  for file in "${always[@]}"; do
    if [[ $path == "$file" ]]; then
      printf '%s\n' "${crates[@]}"
      exit 0
    fi
  done
done

# The paths a crate depends on, from the root: a directory ends in `/`,
# and means everything under it.
dependencies_of() {
  local crate=$1 file included
  echo "host-tests/$crate/"
  # `#[path]` is relative to the directory of the file it's in.
  while IFS=: read -r file included; do
    included=$(realpath -m --relative-to=. "$(dirname "$file")/$included")
    if [[ $included == */mod.rs ]]; then
      echo "${included%mod.rs}"
    else
      echo "$included"
      echo "${included%.rs}/"
    fi
  done < <(grep -rHoE '#\[path = "[^"]+"\]' "host-tests/$crate/src" | sed -E 's/:#\[path = "([^"]+)"\]$/:\1/')
  # `name = { path = "../../gbmock" }`, relative to the crate.
  while read -r included; do
    echo "$(realpath -m --relative-to=. "host-tests/$crate/$included")/"
  done < <(grep -oE '\bpath *= *"[^"]+"' "host-tests/$crate/Cargo.toml" | sed -E 's/.*"([^"]+)"/\1/')
}

for crate in "${crates[@]}"; do
  mapfile -t dependencies < <(dependencies_of "$crate")
  affected=
  for path in "${changed[@]}"; do
    for dependency in "${dependencies[@]}"; do
      if [[ $path == "$dependency" || ($dependency == */ && $path == "$dependency"*) ]]; then
        affected=1
        break 2
      fi
    done
  done
  if [[ -n $affected ]]; then
    echo "$crate"
  fi
done
