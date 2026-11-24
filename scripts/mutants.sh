#!/usr/bin/env bash
# Mutation testing of the host-tested firmware modules with cargo-mutants
# (https://mutants.rs): it changes the code a little at a time (`>` to
# `>=`, a function's body to `Default::default()`, ...) and reruns the
# tests, and a change no test notices, a "missed" mutant, is a behaviour
# nothing checks.
#
#     scripts/mutants.sh [-o OUT] [-j JOBS] [--in-diff DIFF] [MODULE ...]
#
# MODULE is a module of src/ that a host-test crate includes (`decimal`,
# `src/gesture.rs`, `themes/retro/schemes`); with none, every one of them.
# `--in-diff DIFF` (a `git diff` from the repository's root, such as
# `<(git diff main)`) narrows that to the code the diff touches, as it is
# in the working tree now. JOBS (default 3) is how many mutants build and
# test at once, each in a copy of its crate with a target directory of
# its own, so a few GB of memory and disk each.
#
# OUT (default `tmp/mutants`, emptied first) receives:
#
# - `summary.md`: a table of each module's mutants and how many were
#   missed, then each missed mutant as `src/FILE:LINE:COLUMN: what it
#   changed`. CI posts it as the job's summary.
# - `missed.txt`: the missed mutants alone, a line each, as above.
# - `modules/<module>.txt`: what became of a module's mutants in each
#   crate that tested them, and the missed ones.
# - `crates/<crate>/mutants.out/` and `crates/<crate>.log`:
#   cargo-mutants' own output, with each mutant's diff and test log
#   (`<crate>-shared`, for a crate's second run; see below).
#
# A missed mutant is a hint, not a failure: some can't be caught, as the
# code means the same either way (an equivalent mutant), and about a
# quarter were that in the first trial (November 2026). The ones known
# are excluded in .cargo/mutants.toml; see CONTRIBUTING.md for reading the
# rest. The script exits non-zero only if it couldn't run.
#
# It never changes the checkout. cargo-mutants mutates only the files in
# a crate's own tree, and a host-test crate includes the module it tests
# from `../../../src/` by `#[path]`, so the script copies the crates (as
# in the working tree, uncommitted changes included) and their modules
# into a scratch directory, puts each crate's modules in its `src/ur/`,
# points the `#[path]`s there, and runs cargo-mutants on the copy. A
# module some crates share (`sleep`, `stages`, `minute_ring`, ...) counts
# as tested by all of them: a mutant is missed only if every one missed
# it. So it's mutated first in its own crate (host-tests/sleep for
# `sleep`), and each other crate tests only what the ones before it
# missed: a third of the time a mutant per crate would take.
#
# The builds use the dev profile at opt-level 2: it keeps the overflow
# checks and `debug_assert!`s that the host tests' release profile turns
# on, and gb_buckets' slowest test takes a second instead of a minute
# unoptimised. Heavy, so locally run it in a memory-capped scope (see
# CONTRIBUTING.md's "Finding bugs").
set -euo pipefail

out=tmp/mutants
jobs=3
diff=
modules=()
while (($# > 0)); do
  case $1 in
    -o | --out) out=$2 && shift 2 ;;
    -j | --jobs) jobs=$2 && shift 2 ;;
    --in-diff) diff=$2 && shift 2 ;;
    -h | --help)
      sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$0"
      exit 0
      ;;
    -*)
      echo "usage: scripts/mutants.sh [-o OUT] [-j JOBS] [--in-diff DIFF] [MODULE ...]" >&2
      exit 2
      ;;
    *) modules+=("$1") && shift ;;
  esac
done
command -v cargo-mutants >/dev/null || {
  echo "cargo-mutants isn't installed: cargo install --locked cargo-mutants" >&2
  exit 1
}

root=$(cd "$(dirname "$0")/.." && pwd)
# OUT relative to where it was run from, as a path argument would be.
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -rf "${out:?}"/{summary.md,missed.txt,modules,crates}
mkdir -p "$out/modules" "$out/crates"

# ==============================================================================
# Which crates include which modules
# ==============================================================================

# `#[path = "../../../src/X.rs"]` in a crate's src/*.rs (relative to the
# file's directory, so only the top-level files can reach src/ that way)
# includes src/X.rs. A module given as a `mod.rs` brings its directory's
# submodules too (the themes' colour schemes), so that whole directory is
# the module: it's named by the directory, and copied whole.
declare -A crates_of # module (path under src/) -> crates, space-separated
while IFS=: read -r file path; do
  crate=${file#host-tests/}
  crate=${crate%%/*}
  module=${path#*../../../src/}
  module=${module%\"*}
  [[ " ${crates_of[$module]-} " == *" $crate "* ]] || crates_of[$module]+=" $crate"
done < <(cd "$root" && grep -oE '#\[path = "\.\./\.\./\.\./src/[^"]+"\]' host-tests/*/src/*.rs)

# A module's name, as the user gives it and the output names it: its path
# under src/ without `.rs`, or a `mod.rs`'s directory.
name_of() {
  local module=${1%.rs}
  echo "${module%/mod}"
}
# The files a module is, under src/: a `mod.rs`'s whole directory.
files_of() {
  if [[ $1 == */mod.rs ]]; then
    (cd "$root/src" && git ls-files -co --exclude-standard -- "${1%/mod.rs}")
  else
    echo "$1"
  fi
}

declare -A by_name
for module in "${!crates_of[@]}"; do
  by_name[$(name_of "$module")]=$module
done

# ==============================================================================
# The modules to mutate
# ==============================================================================

selected=()
if ((${#modules[@]} > 0)); then
  for given in "${modules[@]}"; do
    given=${given#src/}
    given=${given%/}
    module=${by_name[$(name_of "$given")]-}
    if [[ -z $module ]]; then
      echo "no host-test crate includes src/$given; these do: $(printf "%s\n" "${!by_name[@]}" | sort | paste -sd" ")" >&2
      exit 1
    fi
    selected+=("$module")
  done
else
  selected=("${!crates_of[@]}")
fi
mapfile -t selected < <(printf '%s\n' "${selected[@]}" | sort -u)

scratch=$(mktemp -d "${TMPDIR:-/tmp}/ur-mutants.XXXXXX")
trap 'rm -rf "$scratch"' EXIT

# With a diff, only the modules it touches, and of the diff only their
# files, with their paths as they will be in each crate's copy:
# `src/gesture.rs` becomes `src/ur/gesture.rs`. (The rest of it, a
# crate's tests say, isn't code cargo-mutants would mutate in any case.)
if [[ -n $diff ]]; then
  cp "$diff" "$scratch/diff.full"
  mapfile -t touched < <(sed -nE 's|^\+\+\+ b/src/(.*\.rs)$|\1|p' "$scratch/diff.full" | sort -u)
  narrowed=()
  for module in "${selected[@]}"; do
    for file in "${touched[@]}"; do
      if [[ $file == "$module" || ($module == */mod.rs && $file == "${module%mod.rs}"*) ]]; then
        narrowed+=("$module")
        break
      fi
    done
  done
  selected=("${narrowed[@]}")
  awk '
    /^diff --git / { keep = ($3 ~ /^a\/src\//) }
    keep {
      if (/^diff --git /) { sub(/ a\/src\//, " a/src/ur/"); sub(/ b\/src\//, " b/src/ur/") }
      else if (/^--- a\/src\//) sub(/^--- a\/src\//, "--- a/src/ur/")
      else if (/^\+\+\+ b\/src\//) sub(/^\+\+\+ b\/src\//, "+++ b/src/ur/")
      print
    }
  ' "$scratch/diff.full" >"$scratch/diff"
fi

if ((${#selected[@]} == 0)); then
  echo "Nothing to mutate: the diff touches no module a host-test crate includes." |
    tee "$out/summary.md"
  : >"$out/missed.txt"
  exit 0
fi

# ==============================================================================
# The scratch copy
# ==============================================================================

# Each crate that includes a selected module, with every module it
# includes (selected or not: it has to build) in its src/ur/. gbmock is a
# path dependency of `gb`'s, at `../../gbmock` as in the repository. And
# what cargo finds above a crate in the repository: the pinned toolchain,
# and .cargo/config.toml's `[env]`, which the scratch directory (and
# cargo-mutants' copies of the crates, in it) would otherwise go without.
declare -A crates_needed
for module in "${selected[@]}"; do
  for crate in ${crates_of[$module]}; do
    crates_needed[$crate]=1
  done
done
mapfile -t crates < <(printf '%s\n' "${!crates_needed[@]}" | sort)
copy() { # files listed on stdin, from the checkout's $1 to the scratch's $2
  (cd "$root/$1" && tar -cf - -T -) | (mkdir -p "$scratch/$2" && cd "$scratch/$2" && tar -xf -)
}
(cd "$root" && git ls-files -co --exclude-standard -- gbmock rust-toolchain.toml .cargo/config.toml) | copy . .
for crate in "${crates[@]}"; do
  # Without what git ignores: target/, and Bolero's corpus (src/__fuzz__/),
  # which `cargo test` would replay, so that results don't depend on what
  # was fuzzed here.
  (cd "$root" && git ls-files -co --exclude-standard -- "host-tests/$crate") | copy . .
  for module in "${!crates_of[@]}"; do
    [[ " ${crates_of[$module]} " == *" $crate "* ]] || continue
    files_of "$module" | copy src "host-tests/$crate/src/ur"
  done
  sed -i 's|#\[path = "\.\./\.\./\.\./src/|#[path = "ur/|' "$scratch/host-tests/$crate"/src/*.rs
  # What only Kani compiles (`#[cfg(kani)]`, as alarm's contracts) never
  # runs under `cargo test`, so its every mutant would be missed.
  # `#[mutants::skip]` beside the `cfg` has cargo-mutants pass it over;
  # on the same line, so no mutant's line moves, and as the `cfg` drops
  # the item first, rustc never looks for a `mutants` crate.
  find "$scratch/host-tests/$crate/src/ur" -name '*.rs' -exec \
    sed -i -E 's/^(\s*)#\[cfg\(kani\)\]\s*$/\1#[cfg(kani)] #[mutants::skip]/' {} +
done

# ==============================================================================
# Running cargo-mutants, a crate at a time
# ==============================================================================

# cargo-mutants copies the crate again for each job; a relative target
# directory keeps each copy's build in the copy (a global `target-dir`,
# as in a ~/.cargo/config.toml, would have them all build in one), and
# TMPDIR keeps the copies in the scratch directory, to go with it.
export CARGO_TARGET_DIR=target
export CARGO_PROFILE_DEV_OPT_LEVEL=2
export TMPDIR=$scratch

# What's known of each module so far, in $scratch/state/: `<key>.all`,
# its mutants, from the first crate that tested it, and `<key>.left`, the
# ones every crate so far missed, as cargo-mutants names them
# (`src/ur/gesture.rs:171:9: replace Touch::end_ms -> u32 with 0`); and
# the crates that tested it. A crate after the first tests only
# `<key>.left`. A mutant a crate catches, or times out on (a loop that no
# longer ends), is settled. One the first crate can't build is dropped,
# but one a later crate can't build stays missed: the first built it, and
# the later one could tell nothing.
mkdir -p "$scratch/state"
key_of() { echo "${1//\//-}"; }
declare -A tested_by
failed=()
for module in "${selected[@]}"; do
  : >"$out/modules/$(key_of "$(name_of "$module")").txt"
done

# Runs cargo-mutants as LABEL on CRATE's MODULEs, each of them in full
# if no crate has yet, or else only its mutants still missed.
run() { # LABEL CRATE MODULE...
  local label=$1 crate=$2 module file name args=() res=() tried=() exact='' status=0
  shift 2
  for module in "$@"; do
    local key
    key=$(key_of "$module")
    if [[ -n ${tested_by[$module]-} ]]; then
      [[ -s $scratch/state/$key.left ]] || continue
      exact=1
      # Exact names, as regular expressions.
      while IFS= read -r name; do
        res+=(-F "$name")
      done < <(sed 's/[][\\.+*?(){}|^$]/\\&/g; s/^/^/; s/$/$/' "$scratch/state/$key.left")
    else
      while IFS= read -r file; do res+=(-F "^src/ur/${file//./\\.}:"); done < <(files_of "$module")
    fi
    while IFS= read -r file; do args+=(-f "src/ur/$file"); done < <(files_of "$module")
    tried+=("$module")
  done
  ((${#tried[@]} > 0)) || return 0
  # Only the narrowing needs names; a run of whole files goes without.
  if [[ -n $exact ]]; then
    args+=("${res[@]}")
  fi
  [[ -z $diff ]] || args+=(--in-diff "$scratch/diff")
  echo "$label: $(for module in "${tried[@]}"; do
    name_of "$module"
    [[ -z ${tested_by[$module]-} ]] || echo "(the $(grep -c . "$scratch/state/$(key_of "$module").left") left)"
  done | paste -sd' ')"
  # Exit codes: 0 nothing missed, 2 some missed, 3 some timed out; 4 is
  # a failing baseline (the tests fail unmutated), anything else an error.
  (cd "$scratch/host-tests/$crate" &&
    cargo mutants --config "$root/.cargo/mutants.toml" --jobs "$jobs" \
      --annotations none --colors never "${args[@]}" -o "$out/crates/$label") \
    >"$out/crates/$label.log" 2>&1 || status=$?
  tail -n 1 "$out/crates/$label.log" | sed 's/^/  /'
  local results=$out/crates/$label/mutants.out
  for module in "${tried[@]}"; do
    local key report pattern counts=() outcome n line
    key=$(key_of "$module")
    report=$out/modules/$(key_of "$(name_of "$module")").txt
    case $status in
      0 | 2 | 3) ;;
      *)
        echo "$crate: didn't run (see crates/$label.log)" >>"$report"
        [[ " ${failed[*]} " == *" $label "* ]] || failed+=("$label")
        continue
        ;;
    esac
    pattern="^src/ur/($(files_of "$module" | sed 's/[.]/\\./g' | paste -sd'|')):"
    for outcome in caught missed timeout unviable; do
      grep -hE "$pattern" "$results/$outcome.txt" 2>/dev/null >"$scratch/$outcome" || true
      # cargo-mutants 27.1.0 runs the mutants that delete a struct field
      # whatever `--re` says, so a narrowed run's results are narrowed here
      # too, or a mutant an earlier crate caught could come back missed.
      if [[ -n ${tested_by[$module]-} ]]; then
        grep -Fxf "$scratch/state/$key.left" "$scratch/$outcome" >"$scratch/narrowed" || true
        mv "$scratch/narrowed" "$scratch/$outcome"
      fi
      n=$(grep -c . "$scratch/$outcome" || true)
      counts+=("$n $outcome")
    done
    printf -v line '%s, ' "${counts[@]}"
    line=${line%, }
    if [[ -n ${tested_by[$module]-} ]]; then
      echo "$crate: $line (of the ones${tested_by[$module]} missed)" >>"$report"
      cat "$scratch"/{missed,unviable} >"$scratch/state/$key.left"
    else
      echo "$crate: $line" >>"$report"
      cat "$scratch"/{caught,missed,timeout,unviable} >"$scratch/state/$key.all"
      cp "$scratch/missed" "$scratch/state/$key.left"
    fi
    tested_by[$module]+=" $crate"
  done
}

# A module is first mutated in its own crate, the one of its name, if
# there is one: that's where its tests are. Those runs take along the
# crate's other modules that no other crate includes.
declare -A run_home
for crate in "${crates[@]}"; do
  mine=()
  for module in "${selected[@]}"; do
    [[ " ${crates_of[$module]} " == *" $crate "* ]] || continue
    if [[ $(name_of "$module") == "$crate" || ${crates_of[$module]} == " $crate" ]]; then
      mine+=("$module")
    fi
  done
  for module in "${mine[@]}"; do
    if [[ $(name_of "$module") == "$crate" ]]; then
      run "$crate" "$crate" "${mine[@]}"
      run_home[$crate]=1
      break
    fi
  done
done
# Then every crate's shared modules, or all its modules for a crate that
# isn't any module's own: what the crates before missed.
for crate in "${crates[@]}"; do
  rest=()
  for module in "${selected[@]}"; do
    [[ " ${crates_of[$module]} " == *" $crate "* ]] || continue
    [[ " ${tested_by[$module]-} " == *" $crate "* ]] && continue
    rest+=("$module")
  done
  ((${#rest[@]} > 0)) || continue
  run "$crate${run_home[$crate]:+-shared}" "$crate" "${rest[@]}"
done

# ==============================================================================
# The summary
# ==============================================================================

{
  echo "# Mutation testing"
  echo
  if [[ -n $diff ]]; then
    echo "Only the code the diff touches."
    echo
  fi
  echo "| module | mutants | missed | crates |"
  echo "|---|--:|--:|---|"
} >"$out/summary.md"
: >"$out/missed.txt"
for module in "${selected[@]}"; do
  name=$(name_of "$module")
  key=$(key_of "$module")
  report=$out/modules/$(key_of "$name").txt
  if [[ -z ${tested_by[$module]-} ]]; then
    echo "| \`$name\` | | didn't run | ${crates_of[$module]# } |" >>"$out/summary.md"
    continue
  fi
  sed 's|^src/ur/|src/|' "$scratch/state/$key.left" |
    sort -t: -k1,1 -k2,2n -k3,3n >"$scratch/module-missed"
  total=$(grep -c . "$scratch/state/$key.all" || true)
  missed=$(grep -c . "$scratch/module-missed" || true)
  {
    echo
    echo "missed by every crate ($missed of $total):"
    cat "$scratch/module-missed"
  } >>"$report"
  cat "$scratch/module-missed" >>"$out/missed.txt"
  echo "| \`$name\` | $total | $missed | ${tested_by[$module]# } |" >>"$out/summary.md"
done

{
  if ((${#failed[@]} > 0)); then
    echo
    echo "cargo-mutants failed in: ${failed[*]}. Their modules' mutants weren't tested there: see"
    echo "\`crates/<crate>.log\` (a build or test that fails unmutated, or a diff that isn't"
    echo "of the working tree)."
  fi
  echo
  if [[ -s $out/missed.txt ]]; then
    echo "## Missed"
    echo
    echo "No test failed with these changes. Each is a behaviour nothing checks, or one that"
    echo "can't change (an equivalent mutant: see CONTRIBUTING.md's \"Mutation testing\")."
    echo
    echo '```'
    cat "$out/missed.txt"
    echo '```'
  else
    echo "No mutant was missed."
  fi
} >>"$out/summary.md"
echo
cat "$out/summary.md"
((${#failed[@]} == 0))
