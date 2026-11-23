//! Fuzzing the simulator against a dev kit: `ur-sim --fuzz ELF`.
//!
//! `--devkit` checks the simulator against a dev kit along a script someone
//! wrote. This writes the scripts: random ones, from a seed, of what a dev
//! kit can be made to do (touches, double taps and retries among them, the
//! button, waits, test notifications, the battery and the charger, sleep
//! tracking saying the wearer is asleep, and, with this computer as its
//! phone, Gadgetbridge's lines, the clock set back and ahead among them,
//! connecting and disconnecting, and a BTHome thermometer), each run
//! starting from random settings in its storage. The
//! follower (`devkit.rs`) runs each, and checks every step: the frame, what
//! the dev kit's UI got from the phone and sent it, and, now and then, what
//! it saved and boots with.
//!
//! A run that finds a difference is shrunk: cut after the step that showed
//! it, then steps taken out while it still shows (delta debugging), each
//! try a fresh run from the same storage. What's left, a script and a
//! storage image, is the repro, under `--out`, for `--devkit --storage`.
//!
//! The dev kit's storage is read first, kept as `original-storage.bin`,
//! and written back at the end. The runs start from it without its bonds
//! with anything but this computer, so the phone it's paired with stays
//! out of them.
//!
//! `--dry` writes the scripts and runs them on the simulator alone, without
//! a dev kit: a check of the generator (every line one the simulator
//! takes), not of the simulator.
//!
//! Not checked yet (TODO): the simulator's own models where the follower
//! replays the dev kit's answer instead. A finger's path (`press` …
//! `lift`) through the simulator's gesture recognition (`gesture::Tracking`)
//! against the gesture the dev kit logged; buzzes (`motor::take_request`)
//! against the dev kit's motor, which `compare` doesn't export; and the
//! button grid's BTHome adverts on the air, which this computer could scan
//! for (the presses the dev kit's UI hands its radio are checked). Nor
//! the grid's presses with both Bluetooth connections busy (BTHOME NOT
//! SENT), which takes a second central beside this computer.

use std::fmt::Write as _;
use std::path::Path;

use crate::devkit::{self, Finding, Setup};
use crate::flash::Flash;
use crate::storage::{self, key};
use crate::{alarm, gadgetbridge, themes};

/// What `--fuzz` was asked for.
pub struct Fuzz {
    pub seed: u64,
    pub runs: u64,
    pub steps: usize,
    /// Coverage-guided: see [`Recipe`].
    pub guided: bool,
    /// The inputs of the coverage-guided fuzzer on the simulator alone
    /// (`fuzz/`) to run, rather than seeds: see [`corpus`].
    pub corpus: Option<std::path::PathBuf>,
    /// The corpus is the `gadgetbridge` target's (`fuzz/`), not `steps`':
    /// its inputs are sessions with a model of Gadgetbridge (see
    /// [`gadgetbridge_runs`]).
    pub gadgetbridge: bool,
    pub dry: bool,
}

pub fn run(elf: &Path, out: &Path, setup: &Setup, fuzz: &Fuzz) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("creating {}: {e}", out.display()))?;
    if fuzz.gadgetbridge && (fuzz.corpus.is_none() || fuzz.guided) {
        return Err("--gadgetbridge runs a corpus (--corpus DIR), unguided".into());
    }
    if fuzz.dry && fuzz.gadgetbridge {
        return gadgetbridge_dry(out, fuzz);
    }
    if fuzz.dry {
        return dry(out, setup, fuzz);
    }
    if fuzz.gadgetbridge && setup.peer.is_none() {
        return Err("--gadgetbridge needs this computer as the dev kit's phone (--peer)".into());
    }
    let mut bench = devkit::Bench::open(elf, setup.peer.clone())?;
    // The dev kit's own storage, to start from and put back. Kept if it's
    // there already: a fuzz run that stopped half way left the dev kit's
    // storage as its last run made it.
    let original_path = out.join("original-storage.bin");
    let original = match std::fs::read(&original_path) {
        Ok(bytes) => {
            eprintln!(
                "fuzz: starting from {} (an earlier run's), not the dev kit's storage now",
                original_path.display()
            );
            bytes
        }
        Err(_) => {
            let bytes = bench.read_storage()?;
            write(&original_path, &bytes)?;
            bytes
        }
    };
    // What's put back at the end, and what the runs start from: less what
    // the firmware can't read.
    let original = without_stale(&original, &out.join("scratch"))?;
    // The runs start from it less its bonds with anything but this
    // computer: the phone that's paired with the dev kit would otherwise
    // connect when it pleased, and send what it pleased (its media, say).
    let base = only_this_computer(&original, &out.join("scratch"))?;
    let bases = Bases {
        phone: base.clone(),
        lone: without_bonds(&original, &out.join("scratch"))?,
    };
    if fuzz.guided && fuzz.corpus.is_some() {
        // A guided run extends a run by its seeds; an input has none.
        return Err("--guided extends seeded runs; --corpus runs inputs: one or the other".into());
    }
    // Coverage-guided: which functions have run, and the runs to extend.
    let mut guide = if fuzz.guided {
        let names = bench
            .functions(elf, &out.join("scratch"))?
            .ok_or("--guided needs a coverage build (sim/coverage-build.sh)")?;
        eprintln!("fuzz: guided by {} functions' coverage", names.len());
        Some(Guide {
            seen: vec![false; names.len()],
            names,
            corpus: Vec::new(),
        })
    } else {
        None
    };
    let mut found = 0;
    let began = std::time::Instant::now();
    let mut shrinking = std::time::Duration::ZERO;
    let inputs = match &fuzz.corpus {
        Some(dir) => corpus(dir)?,
        None => Vec::new(),
    };
    let runs = if fuzz.corpus.is_some() {
        fuzz.runs.min(inputs.len() as u64)
    } else {
        fuzz.runs
    };
    let result = (|| {
        if fuzz.gadgetbridge {
            let storage = like_erased(&base, &out.join("scratch"))?;
            return gadgetbridge_runs(
                &mut bench,
                &storage,
                &inputs[..runs as usize],
                out,
                fuzz.seed,
                &mut found,
                &mut shrinking,
            );
        }
        for n in 0..runs {
            let seed = fuzz.seed.wrapping_add(n);
            let (recipe, input) = match (&guide, inputs.get(n as usize)) {
                (_, Some(input)) => (Recipe::fresh(seed), Some(input)),
                (Some(guide), None) => (guide.next(seed), None),
                (None, None) => (Recipe::fresh(seed), None),
            };
            let (storage, steps) = match input {
                // As the simulator alone ran it, over the dev kit's
                // storage (its bonds) rather than erased flash.
                Some(input) => {
                    let bytes = std::fs::read(input)
                        .map_err(|e| format!("reading {}: {e}", input.display()))?;
                    let mut rng = Rng::from_bytes(&bytes);
                    let storage = random_storage(&base, &mut rng, &out.join("scratch"))?;
                    (storage, generate(&mut rng, HOST_STEPS, true, false))
                }
                None => recipe.make(
                    &bases,
                    fuzz.steps,
                    setup.peer.is_some(),
                    &out.join("scratch"),
                )?,
            };
            let dir = out.join(format!("run-{seed}"));
            save_repro(&dir, seed, &steps, &storage, &[])?;
            eprintln!(
                "fuzz: run {} of {runs} (seed {seed}{}{})",
                n + 1,
                recipe.describe(),
                input.map_or(String::new(), |input| format!(
                    ": {}, {} steps",
                    input.display(),
                    steps.len()
                ))
            );
            let (started, spent) = (std::time::Instant::now(), bench.spent());
            let seen = guide.as_mut().map(|guide| &mut guide.seen);
            let outcome = trial(&mut bench, &storage, &steps, &dir, seen);
            report_time(&format!("seed {seed}"), started, spent, bench.spent());
            if let Some(guide) = &mut guide {
                guide.learn(seed, &recipe, &outcome, out)?;
            }
            let Some((at, kind)) = outcome.first() else {
                continue;
            };
            found += 1;
            for line in outcome.report() {
                eprintln!("fuzz: seed {seed}: {line}");
            }
            eprintln!("fuzz: seed {seed}: step {at}: {kind}; shrinking");
            let (started, spent) = (std::time::Instant::now(), bench.spent());
            // From the last reboot before it, if there was one: what came
            // before it is only in that storage. Failing that, from the
            // run's start.
            let reboot = (outcome.reboots.iter()).rfind(|(after, _)| *after < at);
            let (mut minimal, mut shrunk) = (Vec::new(), None);
            let mut from = &storage;
            if let Some((after, rebooted)) = reboot {
                eprintln!("fuzz: seed {seed}: from the reboot after step {after}");
                let (steps, outcome) =
                    shrink(&mut bench, rebooted, &steps[after + 1..=at], &kind, &dir);
                if outcome.shows(&kind) {
                    (minimal, shrunk, from) = (steps, Some(outcome), rebooted);
                }
            }
            let (minimal, outcome) = match shrunk {
                Some(outcome) => (minimal, outcome),
                None => shrink(&mut bench, &storage, &steps[..=at], &kind, &dir),
            };
            report_time(
                &format!("seed {seed}'s shrinking"),
                started,
                spent,
                bench.spent(),
            );
            shrinking += started.elapsed();
            let repro = out.join(format!("finding-{seed}"));
            save_repro(&repro, seed, &minimal, from, &outcome.report())?;
            eprintln!(
                "fuzz: seed {seed}: {} steps of {} still show it: {}",
                minimal.len(),
                steps.len(),
                repro.display()
            );
        }
        Ok::<(), String>(())
    })();
    eprintln!("fuzz: putting the dev kit's storage back");
    let restored = bench.restore(&original, &out.join("scratch"));
    let spent = bench.spent();
    eprintln!(
        "fuzz: {:.0} s in all ({:.0} s shrinking): {} storage writes, {:.1} s; \
         {} resets, {:.1} s",
        began.elapsed().as_secs_f64(),
        shrinking.as_secs_f64(),
        spent.writes,
        spent.writing.as_secs_f64(),
        spent.resets,
        spent.resetting.as_secs_f64()
    );
    result?;
    restored?;
    if found == 0 {
        eprintln!("fuzz: {runs} runs, no differences");
        Ok(())
    } else {
        Err(format!(
            "{found} of {runs} runs found differences (see {})",
            out.display()
        ))
    }
}

// ==============================================================================
// Coverage-guided runs
// ==============================================================================

/// How a run is made: its storage and first steps from `seeds[0]`, then,
/// from step `from[i]` on, its steps from `seeds[i + 1]`. A fresh run is
/// one seed; a guided one extends a run that reached functions no run had
/// before (see [`Guide`]): its seeds up to the step that did, then a new
/// one. As the generator's choices come from the seeds alone, the same
/// seeds make the same steps, so the run before the new seed takes over
/// replays the one it extends.
#[derive(Clone)]
struct Recipe {
    seeds: Vec<u64>,
    from: Vec<usize>,
    /// A lone run: from storage with no bonds at all, and no Bluetooth
    /// steps, so the dev kit advertises throughout, for nobody. A state of
    /// its own (a flash write's radio timeslots fall between adverts, not
    /// connection events: F5 of the overnight findings needed it), that
    /// runs with this computer as the phone never reach.
    lone: bool,
}

impl Recipe {
    /// A run of its own: a lone one about a quarter of the time (by the
    /// seed, so the same seed makes the same run).
    fn fresh(seed: u64) -> Self {
        Self {
            seeds: vec![seed],
            from: Vec::new(),
            lone: Rng::new(seed ^ 0x6c6f_6e65).chance(0.25), // "lone"
        }
    }

    /// The storage and steps: `steps` more after the last seed takes over.
    fn make(
        &self,
        bases: &Bases,
        steps: usize,
        phone: bool,
        scratch: &Path,
    ) -> Result<(Vec<u8>, Vec<Step>), String> {
        let (base, phone) = if self.lone {
            (&bases.lone, false)
        } else {
            (&bases.phone, phone)
        };
        let mut first = Rng::new(self.seeds[0]);
        let storage = random_storage(base, &mut first, scratch)?;
        let mut then: Vec<(usize, Rng)> = (self.from.iter().copied())
            .zip(self.seeds[1..].iter().map(|&seed| Rng::new(seed)))
            .collect();
        let count = self.from.last().copied().unwrap_or(0) + steps;
        Ok((
            storage,
            generate_spliced(&mut first, &mut then, count, phone, false),
        ))
    }

    fn describe(&self) -> String {
        let lone = if self.lone { ", lone" } else { "" };
        if self.from.is_empty() {
            return lone.to_owned();
        }
        let parts: Vec<String> = (self.seeds.iter().zip(std::iter::once(&0).chain(&self.from)))
            .map(|(seed, from)| format!("{seed} from step {from}"))
            .collect();
        format!("{lone}: {}", parts.join(", "))
    }
}

/// The storage runs start from: with this computer's bond (for runs with
/// it as the phone), and with none (for lone ones).
struct Bases {
    phone: Vec<u8>,
    lone: Vec<u8>,
}

/// What a coverage-guided campaign knows: which of the coverage build's
/// functions have run (on any run), and the runs that reached ones none
/// had before, to extend.
struct Guide {
    seen: Vec<bool>,
    /// Each flag's function, by name and source line.
    names: Vec<String>,
    corpus: Vec<Entry>,
}

/// A run that reached functions none had before: its first `keep` steps.
struct Entry {
    recipe: Recipe,
    keep: usize,
    /// How many new functions those steps reached.
    found: usize,
}

impl Guide {
    /// The next run: now and then a fresh one, mostly one that extends an
    /// entry (the newest, more often than not, as what they reached is
    /// where the unexplored is), with `seed` from where it found something.
    fn next(&self, seed: u64) -> Recipe {
        let mut rng = Rng::new(seed ^ 0x6775_6964_6564); // "guided"
        if self.corpus.is_empty() || rng.chance(0.2) {
            return Recipe::fresh(seed);
        }
        let len = self.corpus.len() as u64;
        let index = if rng.chance(0.5) {
            len - 1 - rng.below(len.min(4))
        } else {
            rng.below(len)
        };
        let entry = &self.corpus[index as usize];
        // Its seeds up to `keep`, then `seed`.
        let mut recipe = entry.recipe.clone();
        let segments = 1 + recipe
            .from
            .iter()
            .filter(|&&from| from < entry.keep)
            .count();
        recipe.seeds.truncate(segments);
        recipe.from.truncate(segments - 1);
        recipe.seeds.push(seed);
        recipe.from.push(entry.keep);
        recipe
    }

    /// Takes in a run's new coverage: an entry for each step that reached
    /// something new, and `out/coverage.txt` and `out/corpus.txt` brought
    /// up to date.
    fn learn(
        &mut self,
        seed: u64,
        recipe: &Recipe,
        outcome: &Outcome,
        out: &Path,
    ) -> Result<(), String> {
        for (at, new) in &outcome.new {
            let some: Vec<&str> = new
                .iter()
                .take(4)
                .map(|&i| self.names[i].as_str())
                .collect();
            eprintln!(
                "fuzz: seed {seed}: step {at}: {} new functions ({}{})",
                new.len(),
                some.join("; "),
                if new.len() > some.len() { "; …" } else { "" }
            );
            self.corpus.push(Entry {
                recipe: recipe.clone(),
                keep: at + 1,
                found: new.len(),
            });
        }
        let run = self.seen.iter().filter(|&&seen| seen).count();
        eprintln!(
            "fuzz: {run} of {} functions run so far; {} runs to extend",
            self.seen.len(),
            self.corpus.len()
        );
        let mut report = format!(
            "{run} of {} functions have run.\n\nNot yet:\n",
            self.seen.len()
        );
        let mut never: Vec<&str> = (0..self.seen.len())
            .filter(|&i| !self.seen[i])
            .map(|i| self.names[i].as_str())
            .collect();
        never.sort_unstable();
        for name in never {
            report.push_str(name);
            report.push('\n');
        }
        write(&out.join("coverage.txt"), report.as_bytes())?;
        let corpus: String = (self.corpus.iter())
            .map(|entry| {
                format!(
                    "{} steps of (seeds {:?}, from {:?}): {} new\n",
                    entry.keep, entry.recipe.seeds, entry.recipe.from, entry.found
                )
            })
            .collect();
        write(&out.join("corpus.txt"), corpus.as_bytes())
    }
}

// ==============================================================================
// Running and shrinking
// ==============================================================================

/// A step: an input (or a few, for a finger's path), perhaps a wait, and a
/// `compare`.
type Step = Vec<String>;

/// What a run found: at which step, and what (each `Finding`, or the error
/// that stopped it).
#[derive(Default)]
struct Outcome {
    findings: Vec<(usize, Finding)>,
    error: Option<(usize, String)>,
    /// After which steps a `reset` rebooted the dev kit, and with what
    /// storage.
    reboots: Vec<(usize, Vec<u8>)>,
    /// After which steps a coverage build had run functions no earlier run
    /// had, and which (see `trial`).
    new: Vec<(usize, Vec<usize>)>,
}

impl Outcome {
    /// The first thing found, and its kind (for the shrinking to keep).
    fn first(&self) -> Option<(usize, String)> {
        let finding = self.findings.first().map(|(at, f)| (*at, kind(&f.name)));
        let error = (self.error.as_ref()).map(|(at, error)| (*at, error_kind(error)));
        match (finding, error) {
            (Some(f), Some(e)) => Some(if e.0 < f.0 { e } else { f }),
            (f, e) => f.or(e),
        }
    }

    fn shows(&self, kind_wanted: &str) -> bool {
        self.findings
            .iter()
            .any(|(_, f)| kind(&f.name) == kind_wanted)
            || (self.error.as_ref()).is_some_and(|(_, error)| error_kind(error) == kind_wanted)
    }

    fn report(&self) -> Vec<String> {
        let mut lines: Vec<String> = (self.findings.iter())
            .map(|(at, f)| format!("step {at}: {}: {}", f.name, f.what))
            .collect();
        if let Some((at, error)) = &self.error {
            lines.push(format!("step {at}: stopped: {}", error.replace('\n', ": ")));
        }
        lines
    }
}

/// What kind of error an error (`LINE\nWHY`) is: why, without its numbers,
/// so that the shrinking keeps the same error (the dev kit's events
/// outrunning the follower, say), and not any (a step it needed taken out).
fn error_kind(error: &str) -> String {
    let why = error.rsplit('\n').next().unwrap_or(error);
    let why: String = why.chars().filter(|c| !c.is_ascii_digit()).collect();
    format!("error: {why}")
}

/// What kind of difference a finding is, by its name (see `devkit.rs`): a
/// frame's (named for its step); what the phone got or sent, or Home
/// Assistant heard (a step's name and a suffix); or, by its whole name, the
/// settings a reset booted with, the storage's, or too little stack left.
fn kind(name: &str) -> String {
    for suffix in ["-phone-in", "-phone-out", "-ha"] {
        if name.ends_with(suffix) {
            return suffix[1..].to_owned();
        }
    }
    match name {
        "reset" | "storage" | "stack" => name.to_owned(),
        _ => "frame".to_owned(),
    }
}

/// How long something took, and how much of that was the dev kit's
/// storage writes and resets (from `before` to `after`).
fn report_time(
    what: &str,
    started: std::time::Instant,
    before: devkit::Spent,
    after: devkit::Spent,
) {
    eprintln!(
        "fuzz: {what}: {:.1} s: {} storage writes, {:.1} s; {} resets, {:.1} s",
        started.elapsed().as_secs_f64(),
        after.writes - before.writes,
        (after.writing - before.writing).as_secs_f64(),
        after.resets - before.resets,
        (after.resetting - before.resetting).as_secs_f64()
    );
}

/// One run of `steps`, from `storage`; stops at the first error. With
/// `seen` (a coverage build's functions run so far, over every run), it
/// reads the dev kit's coverage after each step, and notes in the outcome
/// the steps that ran functions none had before (and marks them seen).
fn trial(
    bench: &mut devkit::Bench,
    storage: &[u8],
    steps: &[Step],
    dir: &Path,
    mut seen: Option<&mut Vec<bool>>,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut follower = match bench.follow(storage, dir) {
        Ok(follower) => follower,
        Err(e) => {
            outcome.error = Some((0, e));
            return outcome;
        }
    };
    for (at, step) in steps.iter().enumerate() {
        for line in step {
            match follower.execute(line, dir) {
                Ok(findings) => outcome
                    .findings
                    .extend(findings.into_iter().map(|f| (at, f))),
                Err(e) => {
                    // A restart the script didn't ask for: a panic, a
                    // fault, or the watchdog. Its boot log says which.
                    if e.contains("restarted") {
                        let log = follower.boot_log();
                        eprintln!("fuzz: the dev kit's log after it restarted:\n{log}");
                        let _ = write(&dir.join("restart-log.txt"), log.as_bytes());
                    }
                    outcome.error = Some((at, format!("{line}\n{e}")));
                    return outcome;
                }
            }
        }
        if let Some(storage) = follower.take_rebooted() {
            outcome.reboots.push((at, storage));
        }
        // The stack's room: a step that leaves little is a finding, as the
        // next path a little deeper restarts the watch (see
        // `DevKit::stack_left`). (A probe that fails here costs only this.)
        if let Ok(Some(left)) = follower.stack_left()
            && left < STACK_WARN
        {
            outcome.findings.push((
                at,
                Finding {
                    name: "stack".into(),
                    what: format!("only {left} bytes of stack were left at the deepest"),
                },
            ));
            break;
        }
        if let Some(seen) = seen.as_deref_mut() {
            // (A probe that fails here only costs the step's coverage.)
            if let Ok(Some(now)) = follower.coverage() {
                seen.resize(now.len(), false);
                let new: Vec<usize> = (0..now.len()).filter(|&i| now[i] && !seen[i]).collect();
                for &i in &new {
                    seen[i] = true;
                }
                if !new.is_empty() {
                    outcome.new.push((at, new));
                }
            }
        }
        if !outcome.findings.is_empty() {
            break;
        }
    }
    outcome
}

/// A step that leaves less stack than this unused (of ~11 KB) is a
/// finding: see `trial`.
const STACK_WARN: u32 = 512;

/// At most this many runs to shrink one finding, and for at most this
/// long: each is a reset and a storage write, and the steps in real time.
const SHRINK_TRIES: usize = 30;
const SHRINK_TIME: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// The fewest of `steps` (whose last showed `kind`) that still show it:
/// delta debugging (Zeller's ddmin) over all but the last, which stays.
fn shrink(
    bench: &mut devkit::Bench,
    storage: &[u8],
    steps: &[Step],
    kind_wanted: &str,
    dir: &Path,
) -> (Vec<Step>, Outcome) {
    let tries = std::cell::Cell::new(0);
    let began = std::time::Instant::now();
    let spent = || tries.get() >= SHRINK_TRIES || began.elapsed() > SHRINK_TIME;
    let attempt = |bench: &mut devkit::Bench, candidate: &[Step]| {
        tries.set(tries.get() + 1);
        eprintln!(
            "fuzz: shrinking: try {}, {} steps",
            tries.get(),
            candidate.len()
        );
        trial(bench, storage, candidate, &dir.join("shrinking"), None)
    };
    let (last, mut rest) = steps
        .split_last()
        .map(|(l, r)| (l.clone(), r.to_vec()))
        .expect("a step");
    // First, the last few steps alone, then twice as many, and so on: most
    // differences need only what came just before them, and a try costs
    // its steps in real time, so the short ones come cheap. (Bounded by
    // twice the steps of the one that shows.) One that shows has shown it
    // can be shown again.
    let mut best_outcome = None;
    let mut tail = 4;
    while tail < rest.len() && !spent() {
        let mut candidate = rest[rest.len() - tail..].to_vec();
        candidate.push(last.clone());
        let outcome = attempt(bench, &candidate);
        if outcome.shows(kind_wanted) {
            candidate.pop();
            rest = candidate;
            best_outcome = Some(outcome);
            break;
        }
        tail *= 2;
    }
    // Failing that, it must show again from the start, or it's not one to
    // shrink (it may depend on timing): keep the whole thing.
    let mut best_outcome = match best_outcome {
        Some(outcome) => outcome,
        None => {
            let outcome = attempt(bench, steps);
            if !outcome.shows(kind_wanted) {
                eprintln!("fuzz: it didn't show again; keeping the run as it was");
                return (steps.to_vec(), outcome);
            }
            outcome
        }
    };
    // Then delta debugging: halves out, then quarters, and so on.
    let mut chunks = 2;
    'outer: while !rest.is_empty() && !spent() {
        let size = rest.len().div_ceil(chunks);
        for start in (0..rest.len()).step_by(size) {
            if spent() {
                break 'outer;
            }
            let mut candidate: Vec<Step> = rest[..start].to_vec();
            candidate.extend_from_slice(&rest[(start + size).min(rest.len())..]);
            candidate.push(last.clone());
            let outcome = attempt(bench, &candidate);
            if outcome.shows(kind_wanted) {
                candidate.pop();
                rest = candidate;
                best_outcome = outcome;
                chunks = (chunks - 1).max(2);
                continue 'outer;
            }
        }
        if size == 1 {
            break;
        }
        chunks = (chunks * 2).min(rest.len());
    }
    rest.push(last);
    (rest, best_outcome)
}

/// Writes a run's script and storage (and what it found) to `dir`.
fn save_repro(
    dir: &Path,
    seed: u64,
    steps: &[Step],
    storage: &[u8],
    report: &[String],
) -> Result<(), String> {
    save_script(dir, &format!("seed {seed}"), steps, storage, report)
}

/// [`save_repro`], for steps that came from `origin` (a seed, or a fuzzer's
/// input).
fn save_script(
    dir: &Path,
    origin: &str,
    steps: &[Step],
    storage: &[u8],
    report: &[String],
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut script = format!(
        "# ur-sim --fuzz, {origin}: run with\n\
         #   ur-sim --devkit ELF --storage {0}/storage.bin [--peer ADDRESS] {0}/script.txt\n",
        dir.display()
    );
    for line in report {
        let _ = writeln!(script, "# {line}");
    }
    for (n, step) in steps.iter().enumerate() {
        let _ = writeln!(script, "\n# step {n}");
        for line in step {
            let _ = writeln!(script, "{line}");
        }
    }
    write(&dir.join("script.txt"), script.as_bytes())?;
    write(&dir.join("storage.bin"), storage)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))
}

// ==============================================================================
// Without a dev kit
// ==============================================================================

/// The scripts on the simulator alone: each line must be one it takes.
fn dry(out: &Path, setup: &Setup, fuzz: &Fuzz) -> Result<(), String> {
    for n in 0..fuzz.runs {
        let seed = fuzz.seed.wrapping_add(n);
        let mut rng = Rng::new(seed);
        let storage = random_storage(Flash::erased().bytes(), &mut rng, &out.join("scratch"))?;
        let steps = generate(&mut rng, fuzz.steps, setup.peer.is_some(), false);
        let dir = out.join(format!("run-{seed}"));
        save_repro(&dir, seed, &steps, &storage, &[])?;
        let flash = Flash::from_bytes(storage).ok_or("a storage image of the wrong size")?;
        let mut watch = crate::watch::Watch::boot(flash);
        let output = crate::script::Output {
            dir: dir.clone(),
            zoom: 1,
            save: true,
        };
        let text: String = steps
            .iter()
            .flatten()
            .map(|line| alone(line) + "\n")
            .collect();
        crate::script::run(&mut watch, &format!("seed {seed}"), &text, &output)?;
    }
    eprintln!(
        "fuzz: {} runs' scripts ran on the simulator alone",
        fuzz.runs
    );
    Ok(())
}

// ==============================================================================
// Coverage-guided, on the simulator alone
// ==============================================================================

/// Most steps one input makes: libFuzzer's inputs can be long, and a
/// script that long takes the simulator a while.
const HOST_STEPS: usize = 200;

/// One input of a coverage-guided fuzzer (`fuzz/` here): its bytes choose
/// the storage and the steps, as a seed does for `--fuzz`, and the steps
/// run on the simulator alone. Panics if the simulator (the firmware's UI,
/// mostly) does, or a step fails; a step that never ends (the UI spinning)
/// is the fuzzer's timeout.
///
/// Storage goes through a file in `scratch` (the firmware's own saving,
/// as `--fuzz` does): one directory per process.
pub fn host_one(bytes: &[u8], scratch: &Path) {
    let mut rng = Rng::from_bytes(bytes);
    let storage = random_storage(Flash::erased().bytes(), &mut rng, scratch)
        .expect("the scratch directory is writable");
    let steps = generate(&mut rng, HOST_STEPS, true, true);
    // In a file, so what the firmware saves can be read back below.
    let path = scratch.join("watch-storage.bin");
    write(&path, &storage).expect("the scratch directory is writable");
    let flash = Flash::open(&path).expect("it was just written");
    let mut watch = crate::watch::Watch::boot(flash);
    let output = crate::script::Output {
        dir: scratch.to_owned(),
        zoom: 1,
        save: true,
    };
    let text: String = steps
        .iter()
        .flatten()
        .map(|line| alone(line) + "\n")
        .collect();
    if let Err(e) = crate::script::run(&mut watch, "fuzz", &text, &output) {
        panic!("{e}\n--- the script:\n{text}");
    }
    // What `check-storage` checks on a dev kit: after a moment to save,
    // the settings the UI has are the ones it would boot with.
    watch.advance(embassy_time::Duration::from_secs(2));
    let expected = devkit::Persisted::of(&watch.app);
    let image = std::fs::read(&path).expect("the storage file is there");
    let booted = devkit::booted_from(image).expect("a region's worth");
    if let Some(finding) =
        devkit::Persisted::differ("storage", &devkit::Persisted::of(&booted), &expected)
    {
        panic!(
            "{}: {}\n--- the script:\n{text}",
            finding.name, finding.what
        );
    }
}

/// A coverage-guided fuzzer's corpus (`fuzz/`; best distilled first, with
/// libFuzzer's `-merge=1`), to run on a dev kit: the simulator alone
/// explored the firmware's branches with these, and each is a path the
/// dev kit should follow the same way. Smallest first, as those are the
/// shortest scripts: the most paths in a night.
fn corpus(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    let mut inputs: Vec<(u64, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| Some((entry.metadata().ok()?.len(), entry.path())))
        .collect();
    inputs.sort();
    Ok(inputs.into_iter().map(|(_, path)| path).collect())
}

/// What `host_one` runs for the input in `input`, as a repro in `out`
/// (`script.txt`, `storage.bin`): its storage's settings over `base` (a
/// dev kit's, say, to keep its bonds), or over erased flash. On the
/// simulator alone, `ur-sim --flash storage.bin script.txt`; on a dev kit,
/// as any finding's (`--devkit --storage`).
pub fn host_repro(input: &Path, base: Option<&Path>, out: &Path) -> Result<(), String> {
    let bytes = std::fs::read(input).map_err(|e| format!("reading {}: {e}", input.display()))?;
    let base = match base {
        Some(base) => {
            std::fs::read(base).map_err(|e| format!("reading {}: {e}", base.display()))?
        }
        None => Flash::erased().bytes().to_vec(),
    };
    let mut rng = Rng::from_bytes(&bytes);
    let storage = random_storage(&base, &mut rng, &out.join("scratch"))?;
    let steps = generate(&mut rng, HOST_STEPS, true, true);
    save_script(
        out,
        &format!("input {}", input.display()),
        &steps,
        &storage,
        &[],
    )?;
    eprintln!(
        "fuzz: {} steps: {}",
        steps.len(),
        out.join("script.txt").display()
    );
    Ok(())
}

/// The script `host_one` runs for `bytes`, to look at a finding.
pub fn host_script(bytes: &[u8], scratch: &Path) -> String {
    let mut rng = Rng::from_bytes(bytes);
    let _ = random_storage(Flash::erased().bytes(), &mut rng, scratch);
    generate(&mut rng, HOST_STEPS, true, true)
        .iter()
        .flatten()
        .map(|line| line.clone() + "\n")
        .collect()
}

/// A line of a dev kit script as the simulator alone takes it: what only
/// a dev kit does (comparing with it, resetting it, its storage, its test
/// media) is left out, as a comment.
fn alone(line: &str) -> String {
    let command = line.split_whitespace().next().unwrap_or_default();
    match command {
        "compare" | "reset" | "check-storage" | "media" => format!("# {line}"),
        _ => line.to_owned(),
    }
}

// ==============================================================================
// The Gadgetbridge mock's sessions, on a dev kit
// ==============================================================================

/// The `gadgetbridge` target's inputs (`--corpus DIR --gadgetbridge`), on
/// the dev kit, from `storage`: each a session with the mock as the phone
/// (`gadgetbridge::drive`), through this computer's link, the simulator
/// following the dev kit as for any script. A step is the mock's action
/// and the lines it and the dev kit then trade, or what the wearer does,
/// and its frame compared; the mock hears every line the dev kit sends.
/// So a run finds what a `steps` run does (a frame, the phone's
/// messages, the storage), and also a line real Gadgetbridge would
/// refuse.
///
/// A run's script is what was run: the mock answers what the dev kit
/// says, so it's not the script the same input makes on the simulator
/// (`gadgetbridge::host_one`) once their answers differ (an activity
/// fetch's, say: the dev kit's history is its own). A difference is
/// shrunk as a `steps` run's is, over that script, the mock left out
/// (the lines it sent are the script's); what the mock refused isn't
/// (without it, nothing would see it), and stays as it ran.
fn gadgetbridge_runs(
    bench: &mut devkit::Bench,
    storage: &[u8],
    inputs: &[std::path::PathBuf],
    out: &Path,
    first_seed: u64,
    found: &mut u64,
    shrinking: &mut std::time::Duration,
) -> Result<(), String> {
    let runs = inputs.len();
    for (n, input) in inputs.iter().enumerate() {
        // A seed only to name the run, as `steps`' corpus runs are.
        let seed = first_seed.wrapping_add(n as u64);
        let bytes =
            std::fs::read(input).map_err(|e| format!("reading {}: {e}", input.display()))?;
        let dir = out.join(format!("run-{seed}"));
        eprintln!(
            "fuzz: run {} of {runs} (seed {seed}: {}, a Gadgetbridge session)",
            n + 1,
            input.display()
        );
        let (started, spent) = (std::time::Instant::now(), bench.spent());
        let (steps, outcome, refused) = gadgetbridge_trial(bench, storage, &bytes, &dir);
        let origin = format!("input {}", input.display());
        save_script(&dir, &origin, &steps, storage, &outcome.report())?;
        report_time(&format!("seed {seed}"), started, spent, bench.spent());
        let Some((at, kind)) = outcome.first() else {
            continue;
        };
        *found += 1;
        for line in outcome.report() {
            eprintln!("fuzz: seed {seed}: {line}");
        }
        let repro = out.join(format!("finding-{seed}"));
        if refused || at >= steps.len() {
            eprintln!("fuzz: seed {seed}: step {at}: {kind}; kept as it ran");
            save_script(&repro, &origin, &steps, storage, &outcome.report())?;
            continue;
        }
        eprintln!("fuzz: seed {seed}: step {at}: {kind}; shrinking");
        let (started, spent) = (std::time::Instant::now(), bench.spent());
        let (minimal, shrunk) = shrink(bench, storage, &steps[..=at], &kind, &dir);
        report_time(
            &format!("seed {seed}'s shrinking"),
            started,
            spent,
            bench.spent(),
        );
        *shrinking += started.elapsed();
        save_script(&repro, &origin, &minimal, storage, &shrunk.report())?;
        eprintln!(
            "fuzz: seed {seed}: {} steps of {} still show it: {}",
            minimal.len(),
            steps.len(),
            repro.display()
        );
    }
    Ok(())
}

/// One session (`bytes`) on the dev kit, from `storage`: the steps it
/// ran, what it found, and whether that was the mock refusing what the
/// dev kit sent (rather than the dev kit and the simulator differing).
fn gadgetbridge_trial(
    bench: &mut devkit::Bench,
    storage: &[u8],
    bytes: &[u8],
    dir: &Path,
) -> (Vec<Step>, Outcome, bool) {
    let mut outcome = Outcome::default();
    let mut follower = match bench.follow(storage, dir) {
        Ok(follower) => follower,
        Err(e) => {
            outcome.error = Some((0, e));
            return (Vec::new(), outcome, false);
        }
    };
    let mut runner = DevKitRunner {
        follower: &mut follower,
        dir,
        findings: Vec::new(),
        at: 0,
        sent: false,
    };
    let mut run = gadgetbridge::drive(bytes, &mut runner);
    // Then what it would boot with, as a `steps` run ends.
    if run.failure.is_none() && runner.findings.is_empty() {
        let line = "check-storage".to_owned();
        runner.at = run.steps.len().saturating_sub(1);
        if let Err(e) = gadgetbridge::Runner::run(&mut runner, &line) {
            run.failure = Some((runner.at, e));
        }
        if let Some(last) = run.steps.last_mut() {
            last.push(line);
        }
    }
    outcome.findings = std::mem::take(&mut runner.findings);
    // What stopped it: a line that failed (`LINE\nWHY`, as `trial` has
    // them), or the mock, which refused what the dev kit sent (no line).
    let refused = (run.failure.as_ref()).is_some_and(|(_, why)| !why.contains('\n'));
    outcome.error = run.failure;
    (run.steps, outcome, refused)
}

/// A dev kit, followed, as where a Gadgetbridge session runs (see
/// `gadgetbridge::Runner`).
struct DevKitRunner<'f, 'a> {
    follower: &'f mut devkit::Follower<'a>,
    dir: &'f Path,
    /// What its `compare`s (and the stack's room) found, by step.
    findings: Vec<(usize, Finding)>,
    /// The step under way.
    at: usize,
    /// A `gb` line went out since the dev kit's lines were last taken.
    sent: bool,
}

/// How long the dev kit gets to answer what was sent before its lines are
/// taken: an answer is a connection event or two away. One that takes
/// longer goes to the mock with the next step's.
const ANSWER_TIME: std::time::Duration = std::time::Duration::from_millis(300);

impl gadgetbridge::Runner for DevKitRunner<'_, '_> {
    fn alone(&self) -> bool {
        false
    }

    fn wait_ms(&self, ms: i64) -> i64 {
        ms.min(gadgetbridge::DEVKIT_WAIT_MS)
    }

    fn run(&mut self, line: &str) -> Result<(), String> {
        self.sent |= line.starts_with("gb ");
        match self.follower.execute(line, self.dir) {
            Ok(findings) => {
                let at = self.at;
                self.findings.extend(findings.into_iter().map(|f| (at, f)));
            }
            Err(e) => {
                // As `trial`: a restart the script didn't ask for.
                if e.contains("restarted") {
                    let log = self.follower.boot_log();
                    eprintln!("fuzz: the dev kit's log after it restarted:\n{log}");
                    let _ = write(&self.dir.join("restart-log.txt"), log.as_bytes());
                }
                return Err(format!("{line}\n{e}"));
            }
        }
        // As `trial`: the stack's room, after each step's frame.
        if line.starts_with("compare ")
            && let Ok(Some(left)) = self.follower.stack_left()
            && left < STACK_WARN
        {
            self.findings.push((
                self.at,
                Finding {
                    name: "stack".into(),
                    what: format!("only {left} bytes of stack were left at the deepest"),
                },
            ));
        }
        Ok(())
    }

    fn to_phone(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let settle = if std::mem::take(&mut self.sent) {
            ANSWER_TIME
        } else {
            std::time::Duration::ZERO
        };
        self.follower.take_heard(settle)
    }

    fn step_end(&mut self, n: usize) -> Vec<String> {
        self.at = n;
        vec![gadgetbridge::compare_line(n)]
    }

    fn found(&self) -> bool {
        !self.findings.is_empty()
    }
}

/// The `gadgetbridge` target's inputs on the simulator alone, as their
/// sessions would run on a dev kit (`--dry --gadgetbridge`): waits cut
/// short, and no steps or heart rates. Each run's script is saved under
/// `out`, and anything that fails (the mock refusing what the simulator
/// sent, say) is reported.
fn gadgetbridge_dry(out: &Path, fuzz: &Fuzz) -> Result<(), String> {
    let dir = fuzz
        .corpus
        .as_ref()
        .ok_or("--gadgetbridge needs --corpus DIR")?;
    let inputs = corpus(dir)?;
    let runs = inputs
        .len()
        .min(usize::try_from(fuzz.runs).unwrap_or(usize::MAX));
    let (mut failed, mut steps) = (0, 0);
    for (n, input) in inputs.iter().take(runs).enumerate() {
        let seed = fuzz.seed.wrapping_add(n as u64);
        let bytes =
            std::fs::read(input).map_err(|e| format!("reading {}: {e}", input.display()))?;
        let dir = out.join(format!("run-{seed}"));
        let mut simulator = gadgetbridge::Simulator::boot(&dir.join("scratch"), true);
        let run = gadgetbridge::drive(&bytes, &mut simulator);
        steps += run.steps.len();
        let report: Vec<String> = (run.failure.iter())
            .map(|(at, why)| format!("step {at}: stopped: {}", why.replace('\n', ": ")))
            .collect();
        let origin = format!("input {}", input.display());
        save_script(&dir, &origin, &run.steps, Flash::erased().bytes(), &report)?;
        if let Some(line) = report.first() {
            failed += 1;
            eprintln!("fuzz: {}: {line}", input.display());
        }
    }
    crate::watch::capture_to_phone(false);
    eprintln!(
        "fuzz: {runs} Gadgetbridge sessions ({steps} steps) ran on the simulator alone, \
         {failed} failed"
    );
    if failed == 0 {
        Ok(())
    } else {
        Err(format!(
            "{failed} of {runs} sessions failed (see {})",
            out.display()
        ))
    }
}

// ==============================================================================
// Random storage
// ==============================================================================

/// `original`, with random settings saved over it, through the firmware's
/// own storage, as the watch would have: the rest (the bonds, the BTHome
/// key) as it was.
fn random_storage(original: &[u8], rng: &mut Rng, scratch: &Path) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("creating {}: {e}", scratch.display()))?;
    let path = scratch.join("random-storage.bin");
    write(&path, original)?;
    storage::init(Flash::open(&path)?);
    let mut saves: Vec<(u8, Vec<u8>)> = Vec::new();
    // Mostly what the UI could have saved; now and then what it couldn't
    // (an index past the end, bytes at random), which boot must cope with.
    let byte = |rng: &mut Rng, valid: u64| {
        if rng.chance(0.9) {
            rng.below(valid) as u8
        } else {
            rng.next() as u8
        }
    };
    for (key, valid) in [
        // As 0.10 and earlier saved the themes, which boot migrates from.
        (key::THEME_LIGHT, 2),
        (key::THEME_DARK, 2),
        (key::FORCE, 3),
        (key::BRIGHTNESS, 2),
        (key::BUTTONS, 4),
        (key::GRID, 4),
    ] {
        if rng.chance(0.6) {
            saves.push((key, vec![byte(rng, valid)]));
        }
    }
    // The themes' IDs: mostly a built-in scheme's, now and then one that
    // isn't (another build's), which boot takes for its first.
    for (key, day) in [(key::THEME_DAY, true), (key::THEME_NIGHT, false)] {
        if rng.chance(0.6) {
            let id = if rng.chance(0.9) {
                let index = rng.below(themes::count(day) as u64) as usize;
                themes::id(day, index)
            } else {
                rng.next() as u32
            };
            saves.push((key, id.to_le_bytes().to_vec()));
        }
    }
    if rng.chance(0.6) {
        saves.push((key::ALARMS, random_alarms(rng).to_bytes().to_vec()));
    }
    if rng.chance(0.4) {
        let bytes: Vec<u8> = (0..alarm::Phone::BYTES).map(|_| rng.next() as u8).collect();
        saves.push((key::PHONE_ALARMS, bytes));
    }
    crate::watch::run(
        async {
            for (key, value) in &saves {
                let saved = storage::save(*key, value).await;
                if !saved {
                    eprintln!("fuzz: couldn't save key {key} to the random storage");
                }
            }
        },
        &mut None,
    );
    std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// `original`, less every bond but this computer's (all of them, if
/// `bluetoothctl` can't say what its address is).
fn only_this_computer(original: &[u8], scratch: &Path) -> Result<Vec<u8>, String> {
    let computer = computer_address();
    if computer.is_none() {
        eprintln!("fuzz: this computer's Bluetooth address is unknown; forgetting every bond");
    }
    keep_bond(original, computer, scratch)
}

/// `original`, with no bonds at all: for a lone run (see [`Recipe`]).
fn without_bonds(original: &[u8], scratch: &Path) -> Result<Vec<u8>, String> {
    keep_bond(original, None, scratch)
}

/// `original` with its settings as erased flash has them (none saved),
/// its bonds and BTHome key kept: as `gadgetbridge::host_one` starts,
/// but for the link with this computer.
fn like_erased(original: &[u8], scratch: &Path) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("creating {}: {e}", scratch.display()))?;
    let path = scratch.join("erased-settings.bin");
    write(&path, original)?;
    storage::init(Flash::open(&path)?);
    crate::watch::run(
        async {
            for key in [
                key::THEME_LIGHT,
                key::THEME_DARK,
                key::LOCATION,
                key::ALARMS,
                key::PHONE_ALARMS,
                key::BRIGHTNESS,
                key::BUTTONS,
                key::CALIBRATION,
                key::GRID,
            ] {
                if storage::load::<1>(key).await.is_some() && !storage::remove(key).await {
                    eprintln!("fuzz: couldn't drop key {key} from the storage");
                }
            }
        },
        &mut None,
    );
    std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// `original`, less every bond but `computer`'s.
fn keep_bond(
    original: &[u8],
    computer: Option<[u8; 6]>,
    scratch: &Path,
) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("creating {}: {e}", scratch.display()))?;
    let path = scratch.join("base-storage.bin");
    write(&path, original)?;
    storage::init(Flash::open(&path)?);
    crate::watch::run(
        async {
            for slot in key::BONDS {
                // As `ble::encode_bond` stores it: a format byte, the
                // address's kind, then the address, least significant
                // byte first.
                let Some(bond) = storage::load::<BOND_BYTES>(slot).await else {
                    continue;
                };
                let mut address: [u8; 6] = bond[2..8].try_into().expect("six bytes");
                address.reverse();
                if Some(address) == computer {
                    continue;
                }
                if computer.is_some() {
                    let shown: Vec<String> = address.iter().map(|b| format!("{b:02X}")).collect();
                    eprintln!(
                        "fuzz: the runs forget the dev kit's bond with {}",
                        shown.join(":")
                    );
                }
                if !storage::remove(slot).await {
                    eprintln!("fuzz: couldn't forget the bond in slot {slot}");
                }
            }
        },
        &mut None,
    );
    std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// `original`, less any setting stored shorter than the firmware reads it
/// (a value from a build that used its key for something else: a dev kit's
/// `PHONE_ALARMS` held a 1-byte item, which every boot logged as
/// `BufferTooSmall`).
///
/// TODO: `storage::load` removes such an item itself now (see
/// `storage::load_into`), so the full-size loads below already drop it, and
/// the `remove` after them never runs: this can go, with its call.
fn without_stale(original: &[u8], scratch: &Path) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("creating {}: {e}", scratch.display()))?;
    let path = scratch.join("tidied-storage.bin");
    write(&path, original)?;
    storage::init(Flash::open(&path)?);
    crate::watch::run(
        async {
            // A longer item gives its first bytes; a shorter one, nothing.
            // So stale is a byte, but not the whole value. (The one-byte
            // settings can't be too short.)
            for (key, whole) in [
                (
                    key::ALARMS,
                    storage::load::<{ alarm::Settings::BYTES }>(key::ALARMS)
                        .await
                        .is_some(),
                ),
                (
                    key::PHONE_ALARMS,
                    storage::load::<{ alarm::Phone::BYTES }>(key::PHONE_ALARMS)
                        .await
                        .is_some(),
                ),
            ] {
                if !whole && storage::load::<1>(key).await.is_some() {
                    eprintln!("fuzz: dropping key {key} from the storage: too short to read");
                    if !storage::remove(key).await {
                        eprintln!("fuzz: couldn't drop key {key}");
                    }
                }
            }
        },
        &mut None,
    );
    std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// How long a bond is in storage (`ble::BOND_LEN`, private to the
/// firmware's Bluetooth, which the simulator doesn't build).
const BOND_BYTES: usize = 43;

/// This computer's Bluetooth address (its default controller's), most
/// significant byte first, as `bluetoothctl show` says.
fn computer_address() -> Option<[u8; 6]> {
    let output = std::process::Command::new("bluetoothctl")
        .arg("show")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let address = text
        .lines()
        .find_map(|line| line.strip_prefix("Controller "))?;
    let bytes: Vec<u8> = (address.split_whitespace().next()?.split(':'))
        .map(|byte| u8::from_str_radix(byte, 16).ok())
        .collect::<Option<_>>()?;
    bytes.try_into().ok()
}

fn random_alarms(rng: &mut Rng) -> alarm::Settings {
    let bytes: [u8; alarm::Settings::BYTES] = std::array::from_fn(|_| rng.next() as u8);
    alarm::Settings::from_bytes(bytes)
}

// ==============================================================================
// Random scripts
// ==============================================================================

/// What the generator believes of the dev kit, to say sensible things.
struct Context {
    /// This computer can be its phone.
    phone: bool,
    /// For the simulator alone: what only it can be told (sensor readings,
    /// last night's sleep, Bluetooth's states) goes in as is; for a dev
    /// kit, as a comment. Chosen alike either way, so the same choices
    /// make the same script but for those lines (see `host_only`).
    host: bool,
    connected: bool,
    advertising: bool,
    /// A call ringing, to end.
    calling: bool,
    /// The clock as the phone last set it (UTC seconds since 1970, and
    /// the zone's offset in hours), and the seconds of waits since
    /// (roughly): for alarms just ahead, and to set it relative to now.
    clock: Option<(i64, f64)>,
    waited: f64,
    /// The charger's lines as last faked (`battery MV LINES`), or `None`
    /// for the dev kit's own (as after a reset).
    charger: Option<Charger>,
}

/// What the charger's lines say (see `script`'s `battery`).
#[derive(Clone, Copy, PartialEq)]
enum Charger {
    Unplugged,
    Charging,
    Charged,
}

impl Context {
    /// The clock now (UTC seconds), as far as the waits since it was set
    /// go, and its zone (hours).
    fn now(&self) -> Option<(i64, f64)> {
        self.clock
            .map(|(utc, zone)| (utc + self.waited as i64, zone))
    }

    /// The local time now, in seconds since 1970.
    fn local_now(&self) -> Option<i64> {
        self.now().map(|(utc, zone)| utc + zone_seconds(zone))
    }
}

/// `count` random steps; with `phone`, this computer is the dev kit's
/// phone, so they include Bluetooth.
fn generate(rng: &mut Rng, count: usize, phone: bool, host: bool) -> Vec<Step> {
    generate_spliced(rng, &mut [], count, phone, host)
}

/// [`generate`], its choices from `first` until the first of `then`'s
/// steps, then from that one's `Rng`, and so on: the same steps as
/// `generate` with `first` up to there, then others, from the state
/// (connected, a call ringing, the time) they left.
fn generate_spliced(
    first: &mut Rng,
    then: &mut [(usize, Rng)],
    count: usize,
    phone: bool,
    host: bool,
) -> Vec<Step> {
    let rng = first;
    let mut context = Context {
        phone,
        host,
        connected: false,
        advertising: false,
        calling: false,
        clock: None,
        waited: 0.0,
        charger: None,
    };
    let mut steps = Vec::new();
    if phone {
        // As Gadgetbridge connects: the time first.
        let mut step = vec!["connect".to_owned()];
        step.push(set_time(rng, &mut context));
        context.connected = true;
        step.push("wait 1s".into());
        step.push("compare s000".into());
        steps.push(step);
    }
    loop {
        let n = steps.len();
        let rng: &mut Rng = match then.iter().rposition(|(from, _)| *from <= n) {
            Some(i) => &mut then[i].1,
            None => &mut *rng,
        };
        if n >= count || rng.exhausted() {
            break;
        }
        let mut step = action(rng, &mut context);
        // Mostly long enough for a transition to end; now and then not,
        // to catch the frames between.
        if rng.chance(0.7) {
            let ms = rng.range(200, 1500);
            step.push(format!("wait {ms}ms"));
            context.waited += ms as f64 / 1000.0;
        }
        step.push(format!("compare s{n:03}"));
        steps.push(step);
    }
    if let Some(last) = steps.last_mut() {
        last.push("check-storage".into());
    }
    steps
}

/// A step only the simulator can take (a sensor's reading, say): as is
/// for it, and as a comment for a dev kit, where the choices that made it
/// still count, so the steps after it are the same either way.
fn host_only(context: &Context, line: String) -> Step {
    if context.host {
        vec![line]
    } else {
        vec![format!("# host only: {line}")]
    }
}

/// One random action.
fn action(rng: &mut Rng, context: &mut Context) -> Step {
    let point = |rng: &mut Rng| format!("{},{}", rng.below(80), rng.below(80));
    // (weight, what): the weights sum to whatever they sum to.
    let phone = context.phone;
    let connected = context.connected;
    let choices: &[(u64, u8)] = &[
        (14, 0), // tap
        (12, 1), // swipe
        (4, 2),  // long press
        (6, 3),  // a finger's path
        (9, 4),  // button
        (3, 5),  // double press
        (3, 6),  // glance
        (6, 7),  // a wait of a few seconds
        (4, 8),  // a long wait: the screen times out
        (4, 9),  // test notification
        (2, 10), // test media
        (5, 15), // a grid press
        // No `steps`: they'd go into the dev kit's activity, which
        // Gadgetbridge syncs once the phone's paired again (as
        // devkit-compare.txt). TODO: steps, if the dev kit's activity
        // could be put back afterwards, like its storage.
        (3, 12),                             // battery
        (1, 13),                             // reset
        (1, 14),                             // check storage
        (if phone { 3 } else { 0 }, 20),     // connect or disconnect
        (if connected { 6 } else { 0 }, 21), // notification
        (if connected { 3 } else { 0 }, 22), // call
        (if connected { 3 } else { 0 }, 23), // music
        (if connected { 2 } else { 0 }, 24), // find
        (if connected { 3 } else { 0 }, 25), // time
        (if connected { 2 } else { 0 }, 26), // phone alarms
        (if connected { 2 } else { 0 }, 27), // navigation
        (if connected { 1 } else { 0 }, 28), // weather, location
        (if phone { 2 } else { 0 }, 29),     // thermometer
        // What only the simulator can be told (see `host_only`).
        (2, 30), // steps
        (2, 31), // a heart rate
        (2, 32), // sleep tracking: asleep, movement, light sleep
        (1, 33), // the watch's temperature
        (2, 34), // an Aranet4
        (1, 35), // last night's sleep, or a day's history
        (1, 36), // a GPS fix
        (2, 37), // pairing, an update, both connections in use
        (1, 38), // a timer's length, the smart alarm
        // (A new weight changes which byte picks which: the fuzzer's
        // corpus then makes other scripts than it did; see sim/README.md.)
        (8, 40), // two touches: a double tap, a retry, a tap and hold
        (4, 41), // the charger
        (if connected { 4 } else { 0 }, 42), // the phone setting the clock
        (if connected { 1 } else { 0 }, 43), // a doze, and its end
    ];
    let total: u64 = choices.iter().map(|(w, _)| w).sum();
    let mut pick = rng.below(total);
    let what = choices
        .iter()
        .find(|(weight, _)| {
            if pick < *weight {
                true
            } else {
                pick -= weight;
                false
            }
        })
        .map_or(0, |(_, what)| *what);
    let gb = |line: String| vec![format!("gb {line}")];
    match what {
        0 => {
            let ms = if rng.chance(0.2) {
                format!(" {}", rng.range(20, 255))
            } else {
                String::new()
            };
            vec![format!("tap {}{ms}", point(rng))]
        }
        1 => vec![format!(
            "{} {}",
            rng.pick(&["up", "down", "left", "right"]),
            point(rng)
        )],
        2 => vec![format!("long {}", point(rng))],
        3 => {
            let mut path = vec![format!("press {}", point(rng))];
            for _ in 0..rng.below(3) {
                path.push(format!("wait {}ms", rng.range(50, 400)));
                path.push(format!("press {}", point(rng)));
            }
            let held = rng.range(50, 1500);
            context.waited += held as f64 / 1000.0;
            path.push(format!("wait {held}ms"));
            path.push("lift".into());
            path
        }
        4 => vec!["button".into()],
        5 => vec!["button 2".into()],
        6 => vec!["glance".into()],
        7 => {
            let s = rng.range(2, 8);
            context.waited += s as f64;
            vec![format!("wait {s}s")]
        }
        8 => {
            let s = rng.range(10, 40);
            context.waited += s as f64;
            vec![format!("wait {s}s")]
        }
        9 => {
            let hours = if rng.chance(0.3) {
                format!(" {}", rng.below(30))
            } else {
                String::new()
            };
            vec![format!("notify {}{hours}", rng.below(9))]
        }
        10 => vec![format!(
            "media {}",
            rng.pick(&["playing", "paused", "unknown"])
        )],
        12 => {
            // Never critically low for long: below 3.4 V for a minute, the
            // watch switches itself off.
            let mv = if rng.chance(0.2) {
                0
            } else {
                rng.range(3450, 4250)
            };
            vec![format!("battery {mv}")]
        }
        13 => {
            context.connected = false;
            context.calling = false;
            // A dev kit forgets what was injected (its own charger's
            // lines again; the simulator alone, which leaves `reset` out,
            // keeps them, but then any line is right).
            context.charger = None;
            vec!["reset".into(), "wait 2s".into()]
        }
        14 => vec!["check-storage".into()],
        15 => {
            // The button grid (with BUTTONS on, below the clock: the
            // whole screen is its buttons, so any touch on it is a press).
            // Sometimes from the clock; otherwise wherever the screen is,
            // as earlier steps left it.
            let mut path = Vec::new();
            if rng.chance(0.3) {
                path.push("up".into());
                path.push("wait 500ms".into());
            }
            let at = point(rng);
            match rng.below(3) {
                // A tap, which waits 0.3 s (0.6 s with two layers) for a
                // second.
                0 => path.push(format!("tap {at}")),
                // A double tap, or with two layers, a sequence: the same
                // button again, or another. (Two taps over a dev kit's
                // probe are ~0.3 s apart at best: `double` is both in one
                // input, 0.1 s apart.)
                1 => {
                    if rng.chance(0.5) {
                        path.push(format!("double {at}"));
                    } else {
                        path.push(format!("tap {at}"));
                        if rng.chance(0.5) {
                            path.push(format!("wait {}ms", rng.range(50, 700)));
                        }
                        path.push(format!("tap {}", point(rng)));
                    }
                }
                // Held: a hold, then at 1.5 s a long press, then the
                // release.
                _ => {
                    let held = rng.range(600, 2600);
                    context.waited += held as f64 / 1000.0;
                    path.push(format!("press {at}"));
                    path.push(format!("wait {held}ms"));
                    path.push("lift".into());
                }
            }
            path.push(format!("wait {}ms", rng.range(100, 1200)));
            path
        }
        20 => {
            context.connected = !context.connected;
            if context.connected {
                // As Gadgetbridge does on connecting: mostly the phone's
                // own time, which the watch's has drifted a little from
                // meanwhile; now and then one far from it (another phone,
                // or one whose clock was changed while away).
                let time = match context.now() {
                    Some((utc, zone)) if rng.chance(0.7) => {
                        let drift = rng.range(0, 60) as i64 - 30;
                        set_clock(context, utc + drift, zone)
                    }
                    _ => set_time(rng, context),
                };
                vec!["connect".into(), time]
            } else {
                context.calling = false;
                vec!["disconnect".into()]
            }
        }
        21 => gb(format!(
            r#"GB({{"t":"notify","id":{},"src":"{}","title":"{}","subject":"","body":"{}","sender":"{}","tel":"","reply":true}})"#,
            rng.range(1, 2_000_000_000),
            text(rng, 20),
            text(rng, 50),
            text(rng, 80),
            if rng.chance(0.3) {
                text(rng, 30)
            } else {
                String::new()
            },
        )),
        22 => {
            if context.calling && rng.chance(0.7) {
                context.calling = false;
                gb(format!(
                    r#"GB({{"t":"call","cmd":"{}"}})"#,
                    rng.pick(&["end", "reject", "accept"])
                ))
            } else {
                context.calling = true;
                gb(format!(
                    r#"GB({{"t":"call","cmd":"incoming","name":"{}","number":"+47{}"}})"#,
                    if rng.chance(0.8) {
                        text(rng, 30)
                    } else {
                        String::new()
                    },
                    rng.range(10_000_000, 99_999_999)
                ))
            }
        }
        23 => {
            if rng.chance(0.5) {
                gb(format!(
                    r#"GB({{"t":"musicinfo","artist":"{}","album":"{}","track":"{}","dur":{},"c":1,"n":1}})"#,
                    text(rng, 30),
                    text(rng, 20),
                    text(rng, 30),
                    rng.range(30, 600)
                ))
            } else {
                gb(format!(
                    r#"GB({{"t":"musicstate","state":"{}","position":0,"shuffle":-1,"repeat":-1}})"#,
                    rng.pick(&["play", "pause", "stop"])
                ))
            }
        }
        24 => gb(format!(r#"GB({{"t":"find","n":{}}})"#, rng.chance(0.6))),
        25 => vec![set_time(rng, context)],
        26 => {
            let mut alarms = Vec::new();
            for _ in 0..rng.below(5) {
                // Now and then one just ahead, to ring.
                let (h, m) = match context.local_now() {
                    Some(local) if rng.chance(0.4) => {
                        let at = (local + 60 * rng.range(1, 3) as i64).rem_euclid(86_400) / 60;
                        (at / 60, at % 60)
                    }
                    _ => (rng.below(24) as i64, rng.below(60) as i64),
                };
                alarms.push(format!(
                    r#"{{"h":{h},"m":{m},"rep":{},"on":{}}}"#,
                    if rng.chance(0.5) { 0 } else { rng.below(128) },
                    rng.chance(0.8)
                ));
            }
            gb(format!(r#"GB({{"t":"alarm","d":[{}]}})"#, alarms.join(",")))
        }
        27 => {
            if rng.chance(0.25) {
                gb(r#"GB({"t":"nav"})"#.into())
            } else {
                gb(format!(
                    r#"GB({{"t":"nav","instr":"{}","distance":"{} m","action":"{}","eta":"{:02}:{:02}"}})"#,
                    text(rng, 30),
                    rng.range(10, 2000),
                    rng.pick(&[
                        "left",
                        "right",
                        "straight",
                        "slight_left",
                        "slight_right",
                        "sharp_left",
                        "sharp_right",
                        "uturn",
                        "roundabout_right",
                        "finish",
                        "continue",
                    ]),
                    rng.below(24),
                    rng.below(60)
                ))
            }
        }
        28 => {
            if rng.chance(0.5) {
                gb(format!(
                    r#"GB({{"t":"weather","temp":{},"hum":{},"txt":"{}","loc":"{}"}})"#,
                    rng.range(250, 310),
                    rng.below(100),
                    text(rng, 15),
                    text(rng, 15)
                ))
            } else {
                gb(format!(
                    r#"GB({{"t":"gps","lat":{:.4},"lon":{:.4},"alt":10.0,"speed":0.0,"time":1794150001234,"satellites":0,"hdop":{:.1},"externalSource":true,"gpsSource":"network"}})"#,
                    rng.range(0, 140_000) as f64 / 1000.0 - 70.0,
                    rng.range(0, 360_000) as f64 / 1000.0 - 180.0,
                    rng.range(10, 500) as f64 / 10.0
                ))
            }
        }
        30 => host_only(context, format!("steps {}", rng.range(0, 30_000))),
        31 => host_only(
            context,
            if rng.chance(0.2) {
                "heart off".into()
            } else {
                format!("heart {}", rng.range(35, 210))
            },
        ),
        32 => {
            if rng.chance(0.4) {
                // A dev kit can be told this one too (its input 21): its
                // quiet night, which a dev kit on a desk never sleeps
                // into.
                vec![format!(
                    "asleep {}",
                    if rng.chance(0.5) { "on" } else { "off" }
                )]
            } else {
                host_only(
                    context,
                    if rng.chance(0.3) {
                        format!("light-sleep {}", if rng.chance(0.5) { "on" } else { "off" })
                    } else {
                        format!("movement {}", rng.below(4000))
                    },
                )
            }
        }
        33 => host_only(
            context,
            format!("temperature {}", rng.range(0, 60) as i64 - 10),
        ),
        34 => host_only(
            context,
            if rng.chance(0.15) {
                "aranet off".into()
            } else {
                format!(
                    "aranet {} {:.1} {} {} {} {}m",
                    rng.range(350, 5000),
                    rng.range(0, 400) as f64 / 10.0,
                    rng.below(101),
                    rng.range(950, 1050),
                    rng.below(101),
                    rng.below(30)
                )
            },
        ),
        // Both need the time of day, which the watch loses when it
        // switches off (a tap on the storage prompt, say): the phone sets
        // it first.
        35 => {
            let time = format!(
                "time 2026-10-{:02} {:02}:{:02} +00:00",
                rng.range(1, 31),
                rng.below(24),
                rng.below(60)
            );
            let what = if rng.chance(0.3) {
                "demo-day".into()
            } else {
                format!(
                    "last-night {:02}:{:02} {:02}:{:02} deep={} rem={} awake={}",
                    rng.range(20, 23),
                    rng.below(60),
                    rng.range(4, 10),
                    rng.below(60),
                    rng.below(180),
                    rng.below(150),
                    rng.below(90)
                )
            };
            let mut step = host_only(context, time);
            step.extend(host_only(context, what));
            step
        }
        36 => host_only(
            context,
            format!(
                "location {:.4} {:.4}",
                rng.range(0, 140_000) as f64 / 1000.0 - 70.0,
                rng.range(0, 360_000) as f64 / 1000.0 - 180.0
            ),
        ),
        37 => host_only(
            context,
            match rng.below(8) {
                0 => "unpaired not-paired".into(),
                1 => "unpaired pairing-lost".into(),
                2 => "unpaired off".into(),
                3 => format!("passkey {:06}", rng.below(1_000_000)),
                4 => "passkey off".into(),
                5 => format!("update {}", rng.below(101)),
                6 => (*rng.pick(&["update off", "update refused"])).into(),
                _ => (*rng.pick(&["slots full", "slots free"])).into(),
            },
        ),
        38 => host_only(
            context,
            if rng.chance(0.5) {
                format!("timer {} {}", rng.range(1, 2), rng.range(1, 960))
            } else if rng.chance(0.3) {
                "smart off".into()
            } else {
                format!(
                    "smart on {:02}:{:02} {:02}:{:02}",
                    rng.below(24),
                    rng.below(60),
                    rng.below(24),
                    rng.below(60)
                )
            },
        ),
        29 => {
            context.advertising = !context.advertising;
            if context.advertising {
                vec![format!(
                    "bthome-temp {:.2}",
                    rng.range(0, 6000) as f64 / 100.0 - 20.0
                )]
            } else {
                vec!["bthome-temp off".into()]
            }
        }
        40 => two_touches(rng, context),
        41 => charger(rng, context),
        42 => phone_clock(rng, context),
        43 => doze(rng, context),
        _ => unreachable!("only the choices above"),
    }
}

/// Two touches close together, as `wake::after` tells them apart by the
/// time between the first's lift and the second's start: a pair (a
/// double tap, or a tap and hold) within `DOUBLE_TAP_MS` (300), a retry
/// after a light tap within `RETRY_TAP_MS` (1500), and neither after;
/// the wearer's own fell at 16-185 ms and 566-1277 ms. Often from the
/// screen off or a glance, where the first tap's weight matters (light,
/// deliberate from 40 ms, heavy from 75), and mostly near each other,
/// where a pair must be. A dev kit's two taps over its probe are ~0.3 s
/// apart at best, so its pairs are slower than asked (the follower
/// replays them as they were): `double` is its quick one.
fn two_touches(rng: &mut Rng, context: &mut Context) -> Step {
    let mut path = Vec::new();
    match rng.below(3) {
        0 => {}
        // The screen off: it times out 10 s after the last input.
        1 => {
            let ms = rng.range(10_200, 12_000);
            context.waited += ms as f64 / 1000.0;
            path.push(format!("wait {ms}ms"));
        }
        _ => {
            path.push("glance".into());
            path.push(format!("wait {}ms", rng.range(50, 900)));
        }
    }
    // Inside the edges where, with the screen off, touches do nothing.
    let (x, y) = (rng.range(4, 75) as i64, rng.range(4, 75) as i64);
    let held = |rng: &mut Rng| match rng.below(3) {
        0 => rng.range(20, 39),
        1 => rng.range(40, 74),
        _ => rng.range(75, 150),
    };
    let first_held = held(rng);
    // On the simulator alone, now and then a first tap the UI handles
    // late, as when it was drawing a wake (`tap X,Y MS LATE`); a dev
    // kit's taps are as late as its UI makes them.
    let late = if rng.chance(0.25) {
        rng.range(50, 400)
    } else {
        0
    };
    let first = if context.host && late > 0 {
        format!("tap {x},{y} {first_held} {late}")
    } else {
        format!("tap {x},{y} {first_held}")
    };
    // Mostly within `PAIR_RADIUS` (13) of the first.
    let (nx, ny) = if rng.chance(0.85) {
        let off = |rng: &mut Rng, at: i64| (at + rng.range(0, 30) as i64 - 15).clamp(0, 79);
        (off(rng, x), off(rng, y))
    } else {
        (rng.below(80) as i64, rng.below(80) as i64)
    };
    let mut wait = |ms: u64| {
        context.waited += ms as f64 / 1000.0;
        format!("wait {ms}ms")
    };
    // The second tap, `gap` ms after the first's lift: as a tap's touch
    // starts its length before it's handled (`Touch::ended_now`), the
    // wait between the two is the gap and the second's length.
    let mut second = |rng: &mut Rng, path: &mut Step, gap: u64| {
        let held = held(rng);
        path.push(wait(gap + held));
        path.push(format!("tap {nx},{ny} {held}"));
    };
    let kind = rng.below(6);
    if kind == 0 {
        path.push(format!("double {x},{y}"));
        return path;
    }
    path.push(first);
    match kind {
        // A pair: on the simulator alone, the second within 300 ms.
        1 => {
            let gap = if rng.chance(0.7) {
                rng.range(0, 299)
            } else {
                0
            };
            second(rng, &mut path, gap);
        }
        // Too late for a pair, too soon for a retry (none of the
        // wearer's fell here).
        2 => {
            let gap = rng.range(300, 566);
            second(rng, &mut path, gap);
        }
        // A retry.
        3 => {
            let gap = rng.range(566, 1500);
            second(rng, &mut path, gap);
        }
        // Too late for either.
        4 => {
            let gap = rng.range(1500, 2500);
            second(rng, &mut path, gap);
        }
        // A tap and hold: the hold a long press from 0.6 s.
        _ => {
            let gap = rng.range(0, 200);
            path.push(wait(gap));
            path.push(format!("press {nx},{ny}"));
            let held = rng.range(300, 1500);
            path.push(wait(held));
            path.push("lift".into());
        }
    }
    path
}

/// The charger, as its lines say (`battery MV LINES`; a dev kit's input
/// 25): set on it, charging, done, lifted off, and a wobbly cradle's
/// moments off it. What the UI does there: the plug-in wake, the charge
/// screen, the full charge's screen (`battery::FullCharge`), and, every
/// five minutes, a charge glance (`battery::ChargeGlances`).
fn charger(rng: &mut Rng, context: &mut Context) -> Step {
    use Charger::{Charged, Charging, Unplugged};
    let line = |rng: &mut Rng, low: u64, high: u64, lines: Charger| {
        let lines = match lines {
            Unplugged => "unplugged",
            Charging => "charging",
            Charged => "charged",
        };
        format!("battery {} {lines}", rng.range(low, high))
    };
    let mut step = Vec::new();
    // Off it, and back on as it was: a wobbly cradle.
    let wobble = |rng: &mut Rng, step: &mut Step, back: Charger| {
        step.push(line(rng, 3700, 4150, Unplugged));
        step.push(format!("wait {}ms", rng.range(100, 900)));
        step.push(line(rng, 3700, 4200, back));
    };
    let now = match context.charger {
        None | Some(Unplugged) => {
            if rng.chance(0.8) {
                step.push(line(rng, 3500, 4150, Charging));
                Charging
            } else {
                // Set on it full.
                step.push(line(rng, 4100, 4210, Charged));
                Charged
            }
        }
        Some(Charging) => match rng.below(4) {
            // Done.
            0 => {
                step.push(line(rng, 4150, 4210, Charged));
                Charged
            }
            // Further along.
            1 => {
                step.push(line(rng, 3600, 4200, Charging));
                Charging
            }
            2 => {
                step.push(line(rng, 3700, 4150, Unplugged));
                Unplugged
            }
            _ => {
                wobble(rng, &mut step, Charging);
                Charging
            }
        },
        Some(Charged) => match rng.below(4) {
            0 | 1 => {
                step.push(line(rng, 4000, 4210, Unplugged));
                Unplugged
            }
            // A sag, which the charger tops up.
            2 => {
                step.push(line(rng, 4050, 4180, Charging));
                Charging
            }
            _ => {
                wobble(rng, &mut step, Charged);
                Charged
            }
        },
    };
    context.charger = Some(now);
    // Long enough on it for a charge glance: too long for a dev kit's
    // real time, so for the simulator alone.
    if rng.chance(0.15) {
        let s = rng.range(300, 330);
        context.waited += s as f64;
        step.extend(host_only(context, format!("wait {s}s")));
    }
    step
}

/// A doze (`sleep::Night`): asleep in the morning, up (an alarm, or a
/// double press), and asleep again before noon. It ends after 7 minutes
/// awake rather than a night's 22, at noon, or by hand; or it's left
/// going, for the steps after it. Needs the phone, to set the clock to a
/// morning; and on a dev kit, ~15 s of real time.
fn doze(rng: &mut Rng, context: &mut Context) -> Step {
    let Some((utc, zone)) = context.now() else {
        return vec![set_time(rng, context)];
    };
    let local = utc + zone_seconds(zone);
    let midnight = local.div_euclid(DAY) * DAY - zone_seconds(zone);
    // Between 05:00 and 11:30.
    let morning = midnight + rng.range(5 * 3600, 11 * 3600 + 1800) as i64;
    let mut step = vec![set_clock(context, morning, zone)];
    step.extend(["asleep on".into(), "wait 2s".into()]);
    context.waited += 2.0;
    if rng.chance(0.5) {
        step.push("button 2".into());
    } else {
        // A phone alarm at the next minute, a few seconds off, and the
        // button to stop it ringing.
        let at = (morning.div_euclid(60) + 1) * 60;
        step.push(set_clock(context, at - rng.range(2, 6) as i64, zone));
        let of_day = (at + zone_seconds(zone)).rem_euclid(DAY) / 60;
        step.push(format!(
            r#"gb GB({{"t":"alarm","d":[{{"h":{},"m":{},"rep":0,"on":true}}]}})"#,
            of_day / 60,
            of_day % 60
        ));
        step.extend(["wait 8s".into(), "button".into()]);
        context.waited += 8.0;
    }
    step.extend([
        "wait 1s".into(),
        "asleep off".into(),
        "wait 2s".into(),
        "asleep on".into(),
        "wait 2s".into(),
    ]);
    context.waited += 5.0;
    match rng.below(4) {
        0 => {}
        // Up for good: 7 minutes on, it's over. Too long for a dev kit's
        // real time, so its wait is for the simulator alone.
        1 => {
            step.push("asleep off".into());
            let s = rng.range(400, 480);
            context.waited += s as f64;
            step.extend(host_only(context, format!("wait {s}s")));
        }
        // Noon ends it.
        2 => {
            let (utc, zone) = context.now().expect("the clock was just set");
            let noon = (utc + zone_seconds(zone)).div_euclid(DAY) * DAY + 12 * 3600;
            step.push(set_clock(
                context,
                noon - zone_seconds(zone) - rng.range(2, 6) as i64,
                zone,
            ));
            step.push("wait 8s".into());
            context.waited += 8.0;
        }
        _ => step.push("button 2".into()),
    }
    step
}

/// Gadgetbridge's `setTime`, at a random moment in October 2026 (with
/// Europe's and America's clocks going back, at its end), in a random
/// time zone.
///
/// In the past, and not far: the dev kit records its activity at the time
/// it's told, and Gadgetbridge syncs that once the phone's paired again.
/// A minute from the future would sit in its history ahead of the real
/// ones; one from the past only goes over the dev kit's own, of a watch
/// on a desk.
fn set_time(rng: &mut Rng, context: &mut Context) -> String {
    // 2026-10-01 to 2026-11-01, UTC.
    let utc = rng.range(EARLIEST as u64, 1_793_491_200) as i64;
    let zone = *rng.pick(&[-8.0, -5.0, 0.0, 1.0, 2.0, 5.5, 9.0, 12.75]);
    set_clock(context, utc, zone)
}

/// The earliest and (past) the latest the phone sets the clock to: see
/// [`set_time`]. From the start of October 2026 to the day after America's
/// clocks went back (06:00 UTC on 1 November).
const EARLIEST: i64 = 1_790_812_800;
const LATEST: i64 = 1_793_577_600;

/// Time zones a phone may travel to: the ends of the range in use (UTC-12,
/// UTC+14), the half- and quarter-hour ones, and the ones [`set_time`]
/// picks from.
const ZONES: &[f64] = &[
    -12.0, -9.5, -8.0, -5.0, -3.5, 0.0, 1.0, 2.0, 5.5, 5.75, 9.0, 12.75, 14.0,
];

const DAY: i64 = 24 * 60 * 60;

fn zone_seconds(zone: f64) -> i64 {
    (zone * 3600.0) as i64
}

/// Gadgetbridge's `setTime` at `utc` (within [`EARLIEST`] and
/// [`LATEST`]: see [`set_time`] for why), in `zone`.
fn set_clock(context: &mut Context, utc: i64, zone: f64) -> String {
    let utc = utc.clamp(EARLIEST, LATEST - 1);
    context.clock = Some((utc, zone));
    context.waited = 0.0;
    format!("gb {}", devkit::set_time_line(utc, zone))
}

/// The phone setting the clock while connected, as Gadgetbridge does on
/// connecting, at a time zone or summer time change, and every couple of
/// days (`TimeChangeReceiver`): relative to the clock now, as a phone
/// does, where [`set_time`] jumps anywhere in its month. Clock bugs are
/// the commonest kind to reach the wearer, and the fuzzer had found only
/// two (F1 and F4 of the overnight findings), so these aim at the edges:
/// midnight (the day's steps and history), noon (the quiet night), and
/// summer time's hours.
fn phone_clock(rng: &mut Rng, context: &mut Context) -> Step {
    let Some((utc, zone)) = context.now() else {
        return vec![set_time(rng, context)];
    };
    let local = utc + zone_seconds(zone);
    let midnight = local.div_euclid(DAY) * DAY;
    // Seconds either way.
    let signed = |rng: &mut Rng, low: u64, high: u64| {
        let s = rng.range(low, high) as i64;
        if rng.chance(0.5) { s } else { -s }
    };
    match rng.below(6) {
        // A clock that drifted, set right: a few seconds to a few
        // minutes either way (a dev kit's is ~30 s behind after a
        // reset).
        0 => vec![set_clock(context, utc + signed(rng, 1, 180), zone)],
        // Summer time starting or ending: the same instant, the zone an
        // hour on or back.
        1 => {
            let zone = match (rng.chance(0.5), zone) {
                (true, zone) if zone < 14.0 => zone + 1.0,
                (_, zone) if zone > -12.0 => zone - 1.0,
                _ => zone + 1.0,
            };
            vec![set_clock(context, utc, zone)]
        }
        // Travel: the same instant, in another zone.
        2 => vec![set_clock(context, utc, *rng.pick(ZONES))],
        // To just before one of the day's edges, for the steps' waits to
        // cross it: midnight, summer time's hours (02:00 and 03:00), noon,
        // and the next midnight.
        3 => {
            let edge = *rng.pick(&[0, 2 * 3600, 3 * 3600, 12 * 3600, DAY]);
            let at = midnight + edge - rng.range(1, 15) as i64;
            vec![set_clock(context, at - zone_seconds(zone), zone)]
        }
        // Across midnight, and back: just after it, then, a moment
        // later, just before it again (a phone setting right a clock it
        // had set fast).
        4 => {
            let next = midnight + DAY - zone_seconds(zone);
            let after = set_clock(context, next + rng.range(1, 90) as i64, zone);
            let ms = rng.range(200, 1500);
            let before = set_clock(context, next - rng.range(1, 90) as i64, zone);
            vec![after, format!("wait {ms}ms"), before]
        }
        // A large jump: hours or days, back or ahead.
        _ => {
            let s = if rng.chance(0.5) {
                signed(rng, 3600, DAY as u64 - 1)
            } else {
                signed(rng, DAY as u64, 30 * DAY as u64)
            };
            vec![set_clock(context, utc + s, zone)]
        }
    }
}

/// Random text for the phone to send, as Gadgetbridge escapes it: plain
/// words, mostly, and now and then Latin-1 (`\xNN`), other characters
/// (`\uXXXX`, an emoji as two), a quote, or far more than fits.
fn text(rng: &mut Rng, typical: u64) -> String {
    const WORDS: &[&str] = &[
        "Kari",
        "Ola",
        "Nordmann",
        "Ida",
        "Moreau",
        "Signal",
        "Dinner",
        "on",
        "Sunday",
        "Meeting",
        "moved",
        "to",
        "3pm",
        "Late",
        "Trains",
        "Storgata",
        "OK",
        "#plans",
        "ok?",
        "12:30",
        "Quarterly",
        "planning",
        "-",
        "&",
        "Oslo",
    ];
    const ESCAPES: &[&str] = &[
        r"\xc5",
        r"\xe6",
        r"\xf8",
        r"\xe9",
        r"\xfc",
        r"\u2019",
        r"\u201c",
        r"\u2013",
        r"\u2026",
        r"\u00a0",
        r"\ud83d\ude00",
        r"\u4e2d",
        r#"\""#,
        r"\\",
    ];
    let limit = if rng.chance(0.1) {
        typical * 4
    } else {
        rng.range(0, typical)
    };
    let mut out = String::new();
    while (out.len() as u64) < limit {
        if !out.is_empty() {
            out.push(' ');
        }
        if rng.chance(0.15) {
            out.push_str(rng.pick(ESCAPES));
        } else {
            out.push_str(rng.pick(WORDS));
        }
    }
    out
}

/// A small, seeded pseudo-random generator (SplitMix64): the same seed, the
/// same script, here and on any machine.
/// Where the generator's choices come from: a seed (splitmix64), or, for
/// a coverage-guided fuzzer (`fuzz/` here), its input's bytes. Each choice
/// takes as few bytes as it needs, so that the fuzzer's small edits to the
/// bytes are small edits to the script: a byte changed is one choice
/// changed, and the choices after it stay where they were.
enum Rng {
    Seed(u64),
    Bytes { bytes: Vec<u8>, at: usize },
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self::Seed(seed)
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::Bytes {
            bytes: bytes.to_vec(),
            at: 0,
        }
    }

    /// Whether a byte source has run out: the script ends there. (Choices
    /// past the end are zeros, so a script's last step is whole.)
    fn exhausted(&self) -> bool {
        matches!(self, Self::Bytes { bytes, at } if *at >= bytes.len())
    }

    /// The next `count` bytes (at most 8), little-endian.
    fn take(&mut self, count: usize) -> u64 {
        match self {
            Self::Seed(_) => self.next() & (u64::MAX >> (64 - 8 * count)),
            Self::Bytes { bytes, at } => {
                let mut value = 0;
                for i in 0..count {
                    value |= u64::from(bytes.get(*at + i).copied().unwrap_or(0)) << (8 * i);
                }
                *at += count;
                value
            }
        }
    }

    fn next(&mut self) -> u64 {
        match self {
            Self::Seed(state) => {
                *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = *state;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                z ^ (z >> 31)
            }
            Self::Bytes { .. } => self.take(8),
        }
    }

    /// In `0..n` (n > 0).
    fn below(&mut self, n: u64) -> u64 {
        match self {
            Self::Seed(_) => self.next() % n,
            Self::Bytes { .. } => {
                // As many bytes as `n` needs.
                let bytes = (64 - (n - 1).leading_zeros()).div_ceil(8).max(1);
                self.take(bytes as usize) % n
            }
        }
    }

    /// In `low..=high`.
    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }

    fn chance(&mut self, p: f64) -> bool {
        match self {
            Self::Seed(_) => (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p,
            Self::Bytes { .. } => (self.take(1) + 1) as f64 / 256.0 <= p,
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same seed makes the same script: a finding's seed is enough to
    /// see it again.
    #[test]
    fn a_seed_makes_the_same_script() {
        let script = |seed| generate(&mut Rng::new(seed), 40, true, false);
        assert_eq!(script(7), script(7));
        assert_ne!(script(7), script(8));
    }

    /// A guided run that extends another replays it up to where the new
    /// seed takes over (with the same storage), and differs after.
    #[test]
    fn an_extended_recipe_replays_its_parent() {
        let _storage = crate::flash::STORAGE_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // (Unit tests get no `CARGO_TARGET_TMPDIR`.)
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tmp/recipe");
        let bases = Bases {
            phone: Flash::erased().bytes().to_vec(),
            lone: Flash::erased().bytes().to_vec(),
        };
        let make = |recipe: &Recipe| {
            recipe
                .make(&bases, 30, true, &scratch)
                .expect("the scratch directory is writable")
        };
        let parent = Recipe::fresh(11);
        let child = Recipe {
            seeds: vec![11, 12],
            from: vec![9],
            lone: parent.lone,
        };
        let (parent_storage, parent_steps) = make(&parent);
        let (child_storage, child_steps) = make(&child);
        assert_eq!(parent_storage, child_storage);
        assert_eq!(parent_steps[..9], child_steps[..9]);
        assert_ne!(parent_steps[9..], child_steps[9..]);
        assert_eq!(child_steps.len(), 9 + 30);
    }
}
