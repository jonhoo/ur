#!/usr/bin/env bash
# The local release gate: the long, exploratory checks that CI doesn't
# run (no CI job may take hours), on the commit about to be tagged, on
# this computer. Run it from a clean checkout of that commit, before
# tagging; scripts/hooks/pre-push refuses to push a `v*` tag whose commit
# hasn't passed it.
#
#     scripts/release-gate.sh [OPTIONS]
#
# Its stages, which run side by side:
#
# - `kani`: every Kani proof of every host-test crate, a few crates at a
#   time. Fails on a proof that fails or times out, and on a
#   `kani::cover!` that isn't satisfied (Kani calls such a proof
#   verified, but it checks nothing there).
# - `bolero`: every Bolero harness of every host-test crate, fuzzed with
#   libFuzzer for four minutes each, several at a time. Fails on a crash.
# - `sim-fuzz`: the simulator's fuzz targets (sim/fuzz/fuzz_targets/),
#   half an hour each, from their seeds (sim/fuzz/seeds.tar.zst), at the
#   same time. Fails on a crash.
# - `mutants`: scripts/mutants.sh on every module. Advisory: its missed
#   mutants are for reading (see CONTRIBUTING.md's "Mutation testing"),
#   and never fail the gate, nor does the script failing to run.
#
# (Not Miri or coverage: CI runs those on the tag, in minutes.)
#
# Options:
#
#     --only STAGE[,STAGE]   run only these stages (may be repeated)
#     --out DIR              where the results go (default
#                            tmp/release-gate/<the commit's full sha>/)
#     --allow-dirty          run on a tree with uncommitted changes; no
#                            stamp is written, as there's no commit it
#                            would be for
#     --sequential           one stage after another
#     --bolero-seconds N     fuzz each Bolero harness for N seconds
#     --sim-fuzz-seconds N   fuzz each simulator target for N seconds
#     --kani-crates "A B"    only these crates' proofs
#     --bolero-crates "A B"  only these crates' harnesses
#     --mutants-modules "A"  only these modules (as scripts/mutants.sh
#                            names them: `--list`)
#
# and, from the environment (the defaults suit 32 cores and 30 GB, on
# which the whole gate takes about an hour; the fuzzers use a core per
# job or fork, Kani one per crate, and cargo-mutants builds with what's
# left):
#
#     GATE_KANI_JOBS=4          crates whose proofs run at once
#     GATE_KANI_MEMORY=8G       the most memory one crate's proofs get
#     GATE_KANI_TIMEOUT=30m     the most time one proof gets
#     GATE_BOLERO_JOBS=6        harnesses fuzzed at once
#     GATE_SIM_FUZZ_FORKS=6     libFuzzer processes per simulator target
#     GATE_MUTANTS_JOBS=6       mutants built and tested at once
#     GATE_MEMORY=24G           the most memory all of it gets together
#     CARGO_TARGET_DIR          where it builds (default
#                               target/release-gate/), in a directory
#                               per stage
#     NIGHTLY                   the nightly toolchain for the fuzzers
#                               (default: verify.yml's pin)
#
# A shorter fuzz, or a subset of crates or modules, makes a stage
# *reduced*: for trying the gate out, or looking into one failure. A
# reduced stage can pass, but doesn't count towards the stamp.
#
# The results, in OUT:
#
# - `summary.md`: each stage's verdict and time, and the gate's;
# - `passed`: the stamp, with the commit and the date, written only when
#   every stage has run in full on the clean commit and `kani`, `bolero`
#   and `sim-fuzz` passed. A stage may have run in an earlier invocation
#   for the same commit (`--only kani`, to repeat a stage that ran out
#   of memory, say): each leaves its verdict in `status/`;
# - `kani.log`, `bolero.log`, `sim-fuzz.log`, `mutants.log`: what each
#   stage did, a line per step (as on the terminal);
# - `kani/`: each crate's output (`<crate>.txt`), and `timing.md`, every
#   harness with its result, covers and seconds, slowest first;
# - `bolero/`: each harness's output, and `crashes/`, the inputs that
#   failed (also left in the crate's `src/__fuzz__/`, where `cargo test`
#   replays them);
# - `sim-fuzz/`: each target's output, its corpus, and `artifacts/`, the
#   inputs that failed (see CONTRIBUTING.md's "Fuzzing the simulator"
#   for turning one into a script);
# - `mutants/`: scripts/mutants.sh's results, `summary.md` first, and
#   `mutants.txt`, what it printed.
#
# It exits non-zero if a stage it ran failed (`mutants` can't), or it
# couldn't start.
#
# Memory: Kani's CBMC can take many GB on a proof whose bounds are too
# loose, and an out-of-memory kill of something started from a terminal
# can take the terminal down with it. So with systemd, every command
# runs in a scope of its own (`systemd-run --user --scope`) in one slice,
# `ur-release-gate.slice`, capped as above with no swap: going over
# either cap kills only that command, which fails its stage. Without
# systemd there are no caps: the gate warns, runs its stages one after
# another, and Kani a crate at a time.
set -euo pipefail

# What a full run means, as CI had them when it ran these on release
# tags: four minutes a Bolero harness, half an hour a simulator target.
full_bolero_seconds=240
full_sim_fuzz_seconds=1800

stages=(kani bolero sim-fuzz mutants)
# The stages whose failure fails the gate.
gating=(kani bolero sim-fuzz)

# ==============================================================================
# Arguments
# ==============================================================================

usage() {
  sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$0"
}

only=()
out=
allow_dirty=
sequential=
bolero_seconds=$full_bolero_seconds
sim_fuzz_seconds=$full_sim_fuzz_seconds
kani_crates=
bolero_crates=
mutants_modules=
while (($# > 0)); do
  case $1 in
    --only)
      IFS=, read -ra more <<<"$2"
      only+=("${more[@]}")
      shift 2
      ;;
    --out) out=$2 && shift 2 ;;
    --allow-dirty) allow_dirty=1 && shift ;;
    --sequential) sequential=1 && shift ;;
    --bolero-seconds) bolero_seconds=$2 && shift 2 ;;
    --sim-fuzz-seconds) sim_fuzz_seconds=$2 && shift 2 ;;
    --kani-crates) kani_crates=$2 && shift 2 ;;
    --bolero-crates) bolero_crates=$2 && shift 2 ;;
    --mutants-modules) mutants_modules=$2 && shift 2 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "scripts/release-gate.sh: unknown argument $1 (--help lists them)" >&2
      exit 2
      ;;
  esac
done
if ((${#only[@]} == 0)); then
  only=("${stages[@]}")
fi
for stage in "${only[@]}"; do
  if [[ " ${stages[*]} " != *" $stage "* ]]; then
    echo "scripts/release-gate.sh: no stage $stage (there are: ${stages[*]})" >&2
    exit 2
  fi
done
selected() {
  [[ " ${only[*]} " == *" $1 "* ]]
}

kani_jobs=${GATE_KANI_JOBS:-4}
kani_memory=${GATE_KANI_MEMORY:-8G}
kani_timeout=${GATE_KANI_TIMEOUT:-30m}
bolero_jobs=${GATE_BOLERO_JOBS:-6}
sim_fuzz_forks=${GATE_SIM_FUZZ_FORKS:-6}
mutants_jobs=${GATE_MUTANTS_JOBS:-6}
memory=${GATE_MEMORY:-24G}

# ==============================================================================
# The commit, and where its results go
# ==============================================================================

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# The stamp says "this commit passed", so what's tested must be the
# commit: nothing modified, and nothing untracked that git doesn't
# ignore. (What the gate itself writes is ignored: tmp/, target/,
# Bolero's corpus in src/__fuzz__/.)
sha=$(git rev-parse HEAD)
dirty=$(git status --porcelain)
if [[ -n $dirty && -z $allow_dirty ]]; then
  {
    echo "scripts/release-gate.sh: the tree has uncommitted changes:"
    echo "$dirty"
    echo "The gate vouches for a commit: commit them first. (--allow-dirty"
    echo "runs it anyway, to try something, and writes no stamp.)"
  } >&2
  exit 2
fi

out=${out:-tmp/release-gate/$sha}
mkdir -p "$out/status"
out=$(cd "$out" && pwd)
# Whatever this run finds, an earlier run's stamp no longer stands alone.
rm -f "$out/passed"

target=${CARGO_TARGET_DIR:-$root/target/release-gate}
mkdir -p "$target"
target=$(cd "$target" && pwd)

# ==============================================================================
# What the stages need
# ==============================================================================

# The fuzzers need nightly (libFuzzer's instrumentation), the one CI
# pins: verify.yml's `NIGHTLY`, which test.yml's matches.
nightly=${NIGHTLY:-$(sed -n 's/^  NIGHTLY: *//p' .github/workflows/verify.yml)}

missing=()
need() {
  command -v "$1" >/dev/null || missing+=("$1 ($2)")
}
if selected kani; then
  need cargo-kani "cargo install --locked kani-verifier && cargo kani setup"
fi
if selected bolero; then
  need cargo-bolero "cargo install --locked cargo-bolero"
  need jq "your distribution's package"
fi
if selected sim-fuzz; then
  need cargo-fuzz "cargo install --locked cargo-fuzz"
  need zstd "your distribution's package"
fi
if selected mutants; then
  need cargo-mutants "cargo install --locked cargo-mutants"
fi
if selected bolero || selected sim-fuzz; then
  if [[ -z $nightly ]]; then
    missing+=("a nightly toolchain's name (no NIGHTLY in .github/workflows/verify.yml; set NIGHTLY)")
  elif ! rustup run "$nightly" rustc --version >/dev/null 2>&1; then
    missing+=("$nightly (rustup toolchain install $nightly --profile minimal --component rust-src)")
  fi
fi
if ((${#missing[@]} > 0)); then
  echo "scripts/release-gate.sh: missing:" >&2
  printf -- '- %s\n' "${missing[@]}" >&2
  exit 2
fi

# ==============================================================================
# Memory caps
# ==============================================================================

# One slice for everything, capped in all, and a scope per command in it
# with its own cap. The slice's cap is a runtime property (gone after a
# reboot), so it's set every time. No swap for either: a proof that
# outgrows its cap should be killed, not grind.
slice=ur-release-gate.slice
systemd=
if command -v systemd-run >/dev/null &&
  systemd-run --user --scope --quiet --slice="$slice" true 2>/dev/null &&
  systemctl --user set-property --runtime "$slice" MemoryMax="$memory" MemorySwapMax=0 2>/dev/null; then
  systemd=1
else
  cat >&2 <<EOF
scripts/release-gate.sh: WARNING: no systemd user scopes here
(\`systemd-run --user --scope\` failed), so nothing caps the gate's
memory: a Kani proof that blows up can take this session down with it.
Running the stages one after another, and Kani a crate at a time.
EOF
  sequential=1
  kani_jobs=1
fi

# `capped MAX COMMAND...` runs the command in a scope of its own, with at
# most MAX of memory, where there's systemd; as it is, where not.
capped() {
  local max=$1
  shift
  if [[ -n $systemd ]]; then
    systemd-run --user --scope --quiet --slice="$slice" \
      -p MemoryMax="$max" -p MemorySwapMax=0 -p OOMPolicy=continue "$@"
  else
    "$@"
  fi
}

# ==============================================================================
# Reporting
# ==============================================================================

# A line of progress: to the terminal, and to the stage's log.
say() {
  local stage=$1
  shift
  printf '%s %-8s %s\n' "$(date +%T)" "$stage" "$*" | tee -a "$out/$stage.log"
}

minutes() {
  printf '%dm%02ds' $(($1 / 60)) $(($1 % 60))
}

# A stage's verdict, for the summary and the stamp: `pass`, `fail` or
# `advisory`; whether it ran in full (1) or reduced (0); how long it
# took; and a line about it.
record() {
  local stage=$1 verdict=$2 full=$3 seconds=$4 detail=$5
  printf '%s\n' "$verdict" "$full" "$seconds" "$detail" "$sha" >"$out/status/$stage"
  say "$stage" "$verdict$( ((full)) || echo " (reduced)") in $(minutes "$seconds"): $detail"
}

# The functions and settings that the stages' parallel workers need
# (`xargs` starts each in a shell of its own).
export out target root sha nightly systemd slice memory
export kani_memory kani_timeout bolero_seconds sim_fuzz_seconds sim_fuzz_forks
export -f capped say minutes

# ==============================================================================
# Kani: every proof of every crate
# ==============================================================================

# One crate's proofs, one after another (as `cargo kani` runs them), into
# kani/CRATE.txt, with what failed, if anything, a line each in
# kani/CRATE.failed.
#
# A target directory per crate: `cargo kani` holds cargo's lock on it
# while it compiles, and the crates run side by side.
#
# Each harness gets GATE_KANI_TIMEOUT (`--harness-timeout`, unstable in
# Kani 0.68), so that one that blew up fails alone, named, and the rest
# of its crate still runs; `timeout 90m` is the crate's backstop.
kani_crate() {
  local crate=$1 dir=$out/kani started=$SECONDS
  local log=$dir/$crate.txt
  (
    cd "host-tests/$crate" &&
      CARGO_TARGET_DIR=$target/kani/$crate capped "$kani_memory" timeout 90m \
        cargo kani --output-format terse -Z unstable-options --harness-timeout "$kani_timeout"
  ) >"$log" 2>&1 || echo "\`cargo kani\` failed (exit $?): see kani/$crate.txt" >>"$dir/$crate.failed"
  # A row per harness: `Checking harness NAME...`, then its covers (`** 2
  # of 2 cover properties satisfied`), its result (`VERIFICATION:-
  # SUCCESSFUL`) and its time (`Verification Time: 1.5s`). A harness that
  # timed out gets `CBMC timed out.` instead of a time, so a row is also
  # finished by the next harness or the end of the log.
  awk -v crate="$crate" '
    function row() {
      if (harness != "") printf "| %s | %s | %s | %s | %s |\n", crate, harness, result, covers, time
      harness = ""
    }
    /^Checking harness / { row(); harness = $3; sub(/\.\.\.$/, "", harness); result = "?"; covers = "-"; time = "-" }
    /cover properties satisfied/ { covers = $2 " of " $4; if ($2 != $4) print harness > "/dev/stderr" }
    /^VERIFICATION:- / { result = $2 }
    /^CBMC timed out/ { time = "timed out" }
    /^Verification Time: / { time = sprintf("%.1f", $3); row() }
    END { row() }
  ' "$log" >"$log.rows" 2>"$log.covers"
  # Kani calls a proof with an unsatisfied cover verified, but the cover
  # is there to show the proof isn't vacuous (its `kani::assume`s leave
  # some input that reaches the case it's about).
  if [[ -s $log.covers ]]; then
    echo "a cover unsatisfied in: $(tr '\n' ' ' <"$log.covers")" >>"$dir/$crate.failed"
  fi
  # A crate here has proofs, so none run is a failure too (the build's,
  # which `cargo kani` reports above, or this parsing's).
  if [[ ! -s $log.rows ]]; then
    echo "no harness ran" >>"$dir/$crate.failed"
  fi
  if [[ -e $dir/$crate.failed ]]; then
    say kani "$crate: FAILED after $(minutes $((SECONDS - started)))"
  else
    say kani "$crate: $(wc -l <"$log.rows") proofs in $(minutes $((SECONDS - started)))"
  fi
}
export -f kani_crate

stage_kani() {
  local started=$SECONDS dir=$out/kani full=1 crates=() ordered=() crate
  rm -rf "$dir" "$out/kani.log"
  mkdir -p "$dir"
  if [[ -n $kani_crates ]]; then
    full=0
    read -ra crates <<<"$kani_crates"
  else
    mapfile -t crates < <(scripts/kani-affected.sh --all)
  fi
  # The slowest first, so that the longest (alarm, 20 minutes alone on a
  # desktop in November 2026; then heart_schedule and gb_buckets) starts
  # at once rather than last. Only an order: every crate runs.
  for crate in alarm heart_schedule gb_buckets; do
    [[ " ${crates[*]} " != *" $crate "* ]] || ordered+=("$crate")
  done
  for crate in "${crates[@]}"; do
    [[ " ${ordered[*]-} " == *" $crate "* ]] || ordered+=("$crate")
  done
  say kani "${#ordered[@]} crates' proofs, $kani_jobs at a time"
  # (A worker records its own failure, so xargs' status says nothing.)
  # shellcheck disable=SC2016 # `$1` is for the worker's shell to expand.
  printf '%s\n' "${ordered[@]}" | xargs -P "$kani_jobs" -I{} bash -c 'kani_crate "$1"' _ {} || true

  # The timing table: the harnesses that timed out, then the rest,
  # slowest first.
  {
    echo "| crate | harness | result | covers | seconds |"
    echo "|---|---|---|---|--:|"
    find "$dir" -name '*.rows' -exec cat {} + >"$dir/rows"
    grep 'timed out' "$dir/rows" || true
    { grep -v 'timed out' "$dir/rows" || true; } | sort -t'|' -k6 -rg
  } >"$dir/timing.md"
  rm "$dir/rows"

  local failed=()
  for crate in "${ordered[@]}"; do
    if [[ -e $dir/$crate.failed ]]; then
      failed+=("$crate")
      sed "s/^/$crate: /" "$dir/$crate.failed" >>"$dir/failed.txt"
    fi
  done
  local proofs
  proofs=$(($(wc -l <"$dir/timing.md") - 2))
  if ((${#failed[@]} > 0)); then
    record kani fail "$full" $((SECONDS - started)) "failed in ${failed[*]} (kani/failed.txt)"
    return 1
  fi
  record kani pass "$full" $((SECONDS - started)) "$proofs proofs in ${#ordered[@]} crates (kani/timing.md)"
}

# ==============================================================================
# Bolero: every harness, with libFuzzer
# ==============================================================================

# A crate's harnesses, a line each as `CRATE HARNESS`, into
# bolero/CRATE.harnesses (`cargo bolero list` names them, a JSON object a
# line). The listing builds the crate's tests, hence in parallel too.
bolero_list() {
  local crate=$1 dir=$out/bolero
  (
    cd "host-tests/$crate" &&
      CARGO_TARGET_DIR=$target/bolero/$crate capped "$memory" \
        cargo "+$nightly" bolero list
  ) >"$dir/$crate.list" 2>"$dir/$crate.list.txt" || true
  jq -r --arg crate "$crate" 'select(.test) | "\($crate) \(.test)"' "$dir/$crate.list" >"$dir/$crate.harnesses" || true
  if [[ ! -s $dir/$crate.harnesses ]]; then
    echo "$crate: no harnesses listed (bolero/$crate.list.txt)" >>"$dir/failed.txt"
    say bolero "$crate: FAILED to list its harnesses"
  fi
}
export -f bolero_list

# One harness, fuzzed for its time, into bolero/CRATE/HARNESS.txt.
#
# A target directory per crate, which the crate's harnesses share:
# cargo-bolero builds under cargo's lock and then runs the binary itself,
# so harnesses of one crate fuzz side by side once the first has built.
# The corpus and crashes stay where Bolero keeps them, in the crate's
# src/__fuzz__/ (which git ignores): `cargo test` replays both, so a
# crash found here fails the tests until it's fixed, and the corpus a
# run grows is where the next one starts. What this run found is copied
# to bolero/crashes/ besides.
bolero_harness() {
  local crate=$1 harness=$2 dir=$out/bolero started=$SECONDS
  local log=$dir/$crate/${harness//::/.}.txt
  mkdir -p "$dir/$crate"
  if (
    cd "host-tests/$crate" &&
      CARGO_TARGET_DIR=$target/bolero/$crate capped "$memory" timeout $((bolero_seconds + 1800)) \
        cargo "+$nightly" bolero test "$harness" --engine libfuzzer -T "${bolero_seconds}s"
  ) >"$log" 2>&1; then
    say bolero "$crate $harness: ok in $(minutes $((SECONDS - started)))"
  else
    echo "$crate $harness: failed (bolero/$crate/${harness//::/.}.txt)" >>"$dir/failed.txt"
    find "host-tests/$crate/src/__fuzz__" -path '*/crashes/*' -type f -newer "$dir/started" \
      -exec cp --parents -t "$dir/crashes" {} + 2>/dev/null || true
    say bolero "$crate $harness: FAILED after $(minutes $((SECONDS - started)))"
  fi
}
export -f bolero_harness

stage_bolero() {
  local started=$SECONDS dir=$out/bolero full=1 crates=()
  rm -rf "$dir" "$out/bolero.log"
  mkdir -p "$dir/crashes"
  touch "$dir/started"
  if [[ -n $bolero_crates ]]; then
    full=0
    read -ra crates <<<"$bolero_crates"
  else
    mapfile -t crates < <(grep -l bolero host-tests/*/Cargo.toml | cut -d/ -f2)
  fi
  if ((bolero_seconds < full_bolero_seconds)); then
    full=0
  fi
  say bolero "listing ${#crates[@]} crates' harnesses"
  # shellcheck disable=SC2016 # `$1` is for the worker's shell to expand.
  printf '%s\n' "${crates[@]}" | xargs -P "$bolero_jobs" -I{} bash -c 'bolero_list "$1"' _ {} || true
  # The crates' harnesses dealt in turn (`paste`-like, by sorting on each
  # one's place in its crate), so that the harnesses fuzzing at once are
  # of different crates where they can be, and don't all wait on one
  # crate's build.
  for crate in "${crates[@]}"; do
    nl -w1 -s' ' "$dir/$crate.harnesses"
  done | sort -s -n -k1,1 | cut -d' ' -f2- >"$dir/harnesses.txt"
  local count
  count=$(wc -l <"$dir/harnesses.txt")
  say bolero "$count harnesses, ${bolero_seconds}s each, $bolero_jobs at a time"
  # shellcheck disable=SC2016 # `$1` is for the worker's shell to expand.
  xargs -P "$bolero_jobs" -L1 bash -c 'bolero_harness "$1" "$2"' _ <"$dir/harnesses.txt" || true

  if [[ -s $dir/failed.txt ]]; then
    record bolero fail "$full" $((SECONDS - started)) "$(wc -l <"$dir/failed.txt") of $count harnesses failed (bolero/failed.txt, bolero/crashes/)"
    return 1
  fi
  record bolero pass "$full" $((SECONDS - started)) "$count harnesses in ${#crates[@]} crates, ${bolero_seconds}s each"
}

# ==============================================================================
# The simulator's fuzz targets
# ==============================================================================

# As CI ran them on release tags: each target from its seeds into a
# scratch corpus (the seeds stay as unpacked), libFuzzer forking
# GATE_SIM_FUZZ_FORKS processes. `-s none`: the targets catch what they
# check themselves (panics, `debug_assert!`s, a step that never ends),
# and a sanitizer would only slow safe Rust. A crash ends the target's
# run, with the input in sim-fuzz/artifacts/.
stage_sim_fuzz() {
  local started=$SECONDS dir=$out/sim-fuzz full=1 targets=() fuzz_target
  rm -rf "$dir" "$out/sim-fuzz.log"
  mkdir -p "$dir/artifacts" "$dir/corpus"
  if ((sim_fuzz_seconds < full_sim_fuzz_seconds)); then
    full=0
  fi
  # Found in the tree, so a new target needs no list here.
  mapfile -t targets < <(find sim/fuzz/fuzz_targets -name '*.rs' -exec basename {} .rs \; | sort)
  tar --zstd -xf sim/fuzz/seeds.tar.zst -C "$dir"

  # Built first, one after another (they share the build, under cargo's
  # lock), so that every target's time is all fuzzing.
  say sim-fuzz "building ${targets[*]}"
  for fuzz_target in "${targets[@]}"; do
    if ! (
      cd sim/fuzz &&
        capped "$memory" cargo "+$nightly" fuzz build -s none --target-dir "$target/sim-fuzz" "$fuzz_target"
    ) >>"$dir/build.txt" 2>&1; then
      record sim-fuzz fail "$full" $((SECONDS - started)) "$fuzz_target didn't build (sim-fuzz/build.txt)"
      return 1
    fi
  done

  say sim-fuzz "${#targets[@]} targets, ${sim_fuzz_seconds}s each, $sim_fuzz_forks processes each"
  local pids=()
  for fuzz_target in "${targets[@]}"; do
    mkdir -p "$dir/corpus/$fuzz_target"
    (
      # A target with no seeds yet starts from nothing.
      seeds=()
      if [[ -d $dir/seeds/$fuzz_target ]]; then
        seeds=("$dir/seeds/$fuzz_target")
      fi
      target_started=$SECONDS
      if (
        cd sim/fuzz &&
          capped "$memory" timeout $((sim_fuzz_seconds + 900)) \
            cargo "+$nightly" fuzz run -s none --target-dir "$target/sim-fuzz" "$fuzz_target" \
            "$dir/corpus/$fuzz_target" "${seeds[@]}" -- \
            -fork="$sim_fuzz_forks" -timeout=20 -rss_limit_mb=2048 \
            -max_total_time="$sim_fuzz_seconds" -artifact_prefix="$dir/artifacts/$fuzz_target-"
      ) >"$dir/$fuzz_target.txt" 2>&1; then
        say sim-fuzz "$fuzz_target: ok in $(minutes $((SECONDS - target_started)))"
      else
        echo "$fuzz_target: failed (sim-fuzz/$fuzz_target.txt)" >>"$dir/failed.txt"
        say sim-fuzz "$fuzz_target: FAILED after $(minutes $((SECONDS - target_started)))"
      fi
    ) &
    pids+=($!)
  done
  wait "${pids[@]}" || true

  if [[ -s $dir/failed.txt ]]; then
    record sim-fuzz fail "$full" $((SECONDS - started)) "$(cut -d: -f1 "$dir/failed.txt" | tr '\n' ' ')failed (sim-fuzz/failed.txt, sim-fuzz/artifacts/)"
    return 1
  fi
  record sim-fuzz pass "$full" $((SECONDS - started)) "${targets[*]}, ${sim_fuzz_seconds}s each"
}

# ==============================================================================
# Mutation testing, to read
# ==============================================================================

# Advisory, whatever happens: a missed mutant is a hint for a person
# (some can't be caught at all), and under the other stages' load a slow
# test can time out where it wouldn't alone.
stage_mutants() {
  local started=$SECONDS dir=$out/mutants full=1 modules=() status=0
  rm -rf "$dir" "$out/mutants.log"
  if [[ -n $mutants_modules ]]; then
    full=0
    read -ra modules <<<"$mutants_modules"
  fi
  say mutants "${mutants_modules:-every module}, $mutants_jobs mutants at a time"
  # Its scratch copies (a crate and its build per job, some GB each) go
  # under the target directory rather than /tmp, which may be memory.
  mkdir -p "$target/mutants"
  TMPDIR=$target/mutants capped "$memory" \
    scripts/mutants.sh -o "$dir" -j "$mutants_jobs" "${modules[@]}" >"$out/mutants.txt" 2>&1 || status=$?
  local detail
  if [[ -e $dir/missed.txt ]]; then
    detail="$(wc -l <"$dir/missed.txt") missed mutants to read (mutants/summary.md)"
  else
    detail="no list of missed mutants (mutants.txt)"
  fi
  if ((status != 0)); then
    detail+="; scripts/mutants.sh couldn't run everything (exit $status; mutants.txt)"
  fi
  record mutants advisory "$full" $((SECONDS - started)) "$detail"
}

# ==============================================================================
# Running them
# ==============================================================================

trap 'trap - INT TERM; echo "scripts/release-gate.sh: interrupted" >&2; kill 0' INT TERM

gate_started=$SECONDS
echo "release gate for $sha$([[ -z $dirty ]] || echo " (with uncommitted changes)")"
echo "results in $out"

run_stage() {
  # Its last verdict goes first: if the stage dies without recording
  # one, it hasn't run.
  rm -f "$out/status/$1"
  case $1 in
    kani) stage_kani ;;
    bolero) stage_bolero ;;
    sim-fuzz) stage_sim_fuzz ;;
    mutants) stage_mutants ;;
  esac
}

# (A stage that isn't gating can't fail the gate, even by dying.)
failed_now=()
ran() {
  if [[ $2 != 0 && " ${gating[*]} " == *" $1 "* ]]; then
    failed_now+=("$1")
  fi
}
if [[ -n $sequential ]]; then
  for stage in "${only[@]}"; do
    status=0
    run_stage "$stage" || status=$?
    ran "$stage" "$status"
  done
else
  declare -A pid_of
  for stage in "${only[@]}"; do
    run_stage "$stage" &
    pid_of[$stage]=$!
  done
  for stage in "${only[@]}"; do
    status=0
    wait "${pid_of[$stage]}" || status=$?
    ran "$stage" "$status"
  done
fi

# ==============================================================================
# The summary, and the stamp
# ==============================================================================

# From every stage's recorded verdict for this commit, this run's or an
# earlier one's. The stamp needs each gating stage passed in full, and
# the mutants run in full (so that there's a list to have read), all on
# the commit as committed; `why_not` collects what's in its way.
why_not=()
if [[ -n $dirty ]]; then
  why_not+=("the tree had uncommitted changes")
fi
# The gate ran for an hour: the checkout may have moved under it.
if [[ $(git rev-parse HEAD) != "$sha" || $(git status --porcelain) != "$dirty" ]]; then
  why_not+=("the checkout changed while the gate ran")
fi
{
  echo "# Release gate: \`$sha\`"
  echo
  echo "$(git describe --tags --always --dirty), on $(date -u +%Y-%m-%dT%H:%M:%SZ)."
  echo
  echo "| stage | verdict | time | |"
  echo "|---|---|--:|---|"
  for stage in "${stages[@]}"; do
    status=()
    if [[ -e $out/status/$stage ]]; then
      mapfile -t status <"$out/status/$stage"
    fi
    # (A verdict for another commit, in an `--out` that was reused, is
    # none for this one.)
    if [[ ${status[4]-} != "$sha" ]]; then
      echo "| $stage | not run | | |"
      why_not+=("$stage hasn't run")
      continue
    fi
    verdict=${status[0]}
    if [[ ${status[1]} != 1 ]]; then
      verdict+=" (reduced)"
      why_not+=("$stage ran reduced")
    fi
    if [[ ${status[0]} == fail ]]; then
      why_not+=("$stage failed")
    fi
    echo "| $stage | $verdict | $(minutes "${status[2]}") | ${status[3]} |"
  done
  echo
  if ((${#why_not[@]} == 0)); then
    echo "**Passed.** \`passed\` is this commit's stamp."
  else
    echo "**No stamp**, as:"
    echo
    printf -- '- %s\n' "${why_not[@]}"
  fi
  echo
  echo "This invocation: \`${only[*]}\`, in $(minutes $((SECONDS - gate_started)))."
} >"$out/summary.md"

echo
cat "$out/summary.md"
if ((${#why_not[@]} == 0)); then
  printf '%s\n%s\n' "$sha" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$out/passed"
fi
if ((${#failed_now[@]} > 0)); then
  echo
  echo "scripts/release-gate.sh: FAILED: ${failed_now[*]} (see $out)" >&2
  exit 1
fi
