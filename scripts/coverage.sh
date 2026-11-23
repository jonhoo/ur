#!/usr/bin/env bash
# Line and branch coverage of everything that runs off the watch: the
# firmware's pure modules (src/), through the host-test crates' tests, and
# the simulator (sim/src/, and the modules of src/ it includes), through
# its tests, merged into one report:
#
#   scripts/coverage.sh OUT [CRATE_DIR...]
#
# OUT receives:
#
# - `html/index.html`: llvm-cov's report, every file line by line, with
#   each branch's counts.
# - `summary.md`: each file's line and branch coverage, least covered
#   first, the functions no test reaches, and the files of src/ that no
#   test even builds (the hardware's: Bluetooth, the display, the
#   sensors' drivers, ...), which the percentages leave out.
#
# CRATE_DIR, relative to the repository, limits it to those crates (say
# `host-tests/alarm`); by default every host-test crate and `sim`.
#
# Needs the nightly that `NIGHTLY` names (verify.yml's pin by default),
# with its `llvm-tools` component, since branch coverage is unstable, and
# cargo-llvm-cov 0.9 (verify.yml's `CARGO_LLVM_COV_VERSION`). Builds into a target directory of its own,
# `COVERAGE_TARGET_DIR` (default `target/llvm-cov`), never the normal
# one: cargo doesn't see the instrumentation (cargo-llvm-cov's rustc
# wrapper adds it) among what decides whether to rebuild, so it would
# reuse the normal, uninstrumented builds there.
#
# A test that fails still counts what it ran, and the script carries on;
# it ends by naming the crates whose tests failed, and exits non-zero, after
# writing the report. CI's test.yml is what holds the tests to passing.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=$(realpath -m "$1")
shift
nightly=${NIGHTLY:-nightly-2026-11-06}
export CARGO_TARGET_DIR=${COVERAGE_TARGET_DIR:-$root/target/llvm-cov}

if [ "$#" -gt 0 ]; then
  crates=("$@")
else
  crates=()
  for dir in "$root"/host-tests/*/; do
    dir=${dir%/}
    crates+=("host-tests/${dir##*/}")
  done
  crates+=(sim)
fi

# llvm-tools' own llvm-profdata and llvm-cov, of the same LLVM as the
# compiler that wrote the profiles: the format changes between releases.
host=$(rustc "+$nightly" -vV | sed -n 's/^host: //p')
llvm=$(rustc "+$nightly" --print sysroot)/lib/rustlib/$host/bin

# The host, by name: the simulator's .cargo/config.toml says `host-tuple`,
# which cargo understands and cargo-llvm-cov doesn't (it asks rustc about
# a target of that name). The host-test crates name it already.
export CARGO_BUILD_TARGET=$host

# Up front, as a missing cargo-llvm-cov would otherwise only show as a
# report with nothing in it.
cargo "+$nightly" llvm-cov --version >/dev/null
[ -x "$llvm/llvm-cov" ] || {
  echo "no llvm-cov: rustup component add llvm-tools --toolchain $nightly" >&2
  exit 1
}

# Only what this writes, should OUT hold anything else.
mkdir -p "$out"
rm -rf "$out/profiles" "$out/html" "$out/binaries" "$out/merged.profdata" \
  "$out/coverage.json" "$out/summary.md"
mkdir "$out/profiles"

# ==============================================================================
# Running each crate's tests, instrumented
# ==============================================================================

# cargo-llvm-cov reports on one workspace at a time, and every crate here
# is a workspace of its own (see CONTRIBUTING.md), so it does only the
# instrumenting: `show-env` gives the environment its `cargo llvm-cov`
# would build and run with, in which a plain `cargo test` is instrumented.
# That's only the crate's own code (src/'s modules, included with
# `#[path]`), through its rustc wrapper; the dependencies build as they
# normally do, so all the crates share their builds in the one target
# directory.
#
# Every profile goes into one directory, and every binary that wrote one
# into a list, for llvm-cov to read together below. The binaries come from
# the build's own messages, rather than a search of the target directory,
# which holds stale builds of the same crates too.
#
# Each crate names a module of src/ its own way (as
# `src/../../../src/alarm.rs` from host-tests/alarm, `src/../../src/alarm.rs`
# from sim), and llvm-cov merges a file's coverage from several binaries
# only where they name it the same, so the paths are remapped to the
# file's own (the last prefix that matches wins). llvm-cov's
# `-path-equivalence` would only show them under the same name, still
# apart. The flags are the same for every crate, so the dependencies'
# builds are still shared.
remap=("--remap-path-prefix=src/../../=$root/" "--remap-path-prefix=src/../../../=$root/")
failed=()
for crate in "${crates[@]}"; do
  name=${crate##*/}
  args=(--locked)
  case $crate in
    # As test.yml runs them: optimised (the PPG's simulations are slow
    # without), with every theme.
    host-tests/*) args+=(--release) ;;
    sim) args+=(--no-default-features --features themes-all) ;;
  esac
  echo "::group::$crate"
  (
    cd "$root/$crate"
    # (This subshell's commands don't stop it when they fail, as it's
    # the left of `||`: hence the `exit`s.)
    env=$(cargo "+$nightly" llvm-cov show-env --branch --sh) || exit 1
    eval "$env"
    export LLVM_PROFILE_FILE="$out/profiles/$name-%p-%32m.profraw"
    export RUSTFLAGS="${remap[*]}"
    cargo "+$nightly" test "${args[@]}" --no-run --message-format=json \
      | jq -r 'select(.reason == "compiler-artifact") | .executable // empty' >>"$out/binaries" \
      || exit 1
    cargo "+$nightly" test "${args[@]}"
  ) || failed+=("$crate")
  # Tests that ran uninstrumented would leave the report silently without
  # them.
  profiles=("$out/profiles/$name"-*.profraw)
  if [ ! -e "${profiles[0]}" ]; then
    echo "::error::no coverage from $crate"
    failed+=("$crate")
  fi
  echo "::endgroup::"
done

# ==============================================================================
# Merging
# ==============================================================================

"$llvm/llvm-profdata" merge -sparse "$out"/profiles/*.profraw -o "$out/merged.profdata"
mapfile -t binaries < <(sort -u "$out/binaries")
objects=()
for binary in "${binaries[@]}"; do
  objects+=(-object "$binary")
done

# Only src/ and sim/src/: not the host-test crates' own tests, gbmock, the
# simulator's integration tests or the dependencies.
common=(
  -instr-profile "$out/merged.profdata"
  "${objects[@]}"
  -include-filename-regex "^$root/(sim/)?src/"
)
"$llvm/llvm-cov" show "${common[@]}" -format=html -output-dir "$out/html" \
  -show-branches=count -show-line-counts-or-regions -show-instantiations=false
"$llvm/llvm-cov" export "${common[@]}" -format=text -skip-expansions >"$out/coverage.json"
python3 -I "$root/scripts/coverage_summary.py" "$root" "$out/coverage.json" >"$out/summary.md"

if [ "${#failed[@]}" -gt 0 ]; then
  echo "::error::tests failed in: ${failed[*]} (the report counts what they ran)"
  exit 1
fi
