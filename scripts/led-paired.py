#!/usr/bin/env python3
"""The paired LED test's verdict, from a decoded field log.

The logging build (0.11.3-rc.5 on) runs an awake heart rate session that
starts on a calm wrist at two LED drives, one after the other: 13 s at
its level's drive (12.5 mA, "dim") and 13 s at a brighter one (30 mA, or
20 mA where 30 would be too much light or budget; "bright"), which goes
first taking turns by round. Each phase logs what its own three windows
made of the wrist, as if the session had been that phase alone (see
`heart_schedule`'s "Both drives in one session", and `heart::Paired`).
This reads those lines and compares the two phases of each session.

Usage:
    python3 -I scripts/led-paired.py LOG [LOG ...] [--drives 0,2] [--resamples N]

LOG is `defmt-print`'s output for an `ur-logging` build, through
`scripts/expand_log.py` (lines of `UPTIME LEVEL TEXT`), or a timeline TSV
as `tmp/night-1121/mk_timeline.py` makes (segment, UTC, local, uptime,
level, text). Several logs are taken as one after the other.

What it prints:

- How many sessions were paired, how many weren't and why, and how many
  pairs ran their course (both phases' lines). A pair that went void
  (the light level moved) or whose session ended early (no skin, the
  charger) is left out, and counted.
- **The question**: did the bright phase read where the dim one didn't
  more often than the other way round? Each complete pair is one of four
  (both read, only bright, only dim, neither). The difference in read
  rates, bright minus dim, is (only bright - only dim) / pairs; its
  exact two-sided p is McNemar's (a binomial on the discordant pairs),
  and its 95% interval a bootstrap over rounds (a round's sessions are
  the same wrist minutes apart, so they're resampled together).
- The same by which drive went first. The second phase has a wrist
  that's been still 13 s longer, so order matters; the "order-adjusted"
  difference is the mean of the two orders' differences, which is what
  to quote if the orders aren't evenly filled.
- The rescues: of the pairs whose dim phase didn't read, how many the
  bright one did, and the reverse. "Bright never helps" is the first
  being no higher than the second.
- Each phase's window quality (the best of its windows' correlations),
  accepted windows, and, from the logged samples (numpy needed), the
  pulse's size (0.7-3 Hz, peak to peak) and the noise (above 5 Hz, rms,
  and in the pulse's band but off its line), with the paired difference
  or ratio and its interval.
- Where both phases read: how far apart their rates were.
- What the test did to the reading: the share of paired sessions that
  read at all, against the 44% of calm starts that did before it
  (2026-11-16 to 21, single-drive sessions of 30 s).
- How many more pairs, and days at this log's rate, it takes to pin the
  difference down to +-3 and +-4 points, and to see a difference of 5
  with 80% power, given how often the phases disagreed so far.

How many sessions it needs, before any are in: a phase alone read 31% of
calm starts within 13 s in the earlier logs, and a session that hadn't
read by then read later 15% of the time, so two phases of one session
should disagree in something like 17% of pairs (42% if they were
independent, which sessions minutes apart nearly are). The interval's
half-width is 1.96 * sqrt(disagreeing share / pairs): +-4 points takes
~410 pairs at 17% (~1,010 at 42%), +-3 points ~730 (~1,790). The logs had
91 calm starts a day, so that's 5 to 8 days, 11 to 20 if the phases turn
out independent; this prints which it is after the first day or two.
The window qualities and the pulse over the noise are measured on every
pair rather than counted, and settle within a day or two.
"""
import argparse
import math
import random
import re
import statistics
import sys
from collections import Counter

try:
    import numpy as np
except ImportError:  # the read rates need none of it
    np = None

# What a single-drive session of 30 s read from a calm start before the
# test (tmp/led-paired/plan.py, on tmp/night-1121/all.sessions.jsonl).
BEFORE_READ = (193, 435)
# The sensor's sample rate (its period is ~36.9 ms on the sealed watch).
FS = 1e6 / 36900.0
# A phase's samples before its estimator's first: one warming up, thirteen
# settling (`ppg::WARMUP`, `SETTLE`).
SKIP = 14

STAMPED = re.compile(r"^(\d+\.\d+) (\w+)\s+(.*)$")
DUE = re.compile(r"heart: due \((\w+)\); budget (\d+) s, .*round (\d+) (\w+) drive (\d+); (calm|busy) start")
PAIRED = re.compile(r"heart: paired session, drive (\d) then (\d)")
NOT_PAIRED = re.compile(r"heart: not paired \((.*)\)")
PHASE = re.compile(
    r"heart: pair phase (\d) at (\d)/(\d): (\d+) windows, (\d+) accepted, (\d+) faint; "
    r"(?:read (\d+) BPM at window (\d+)|no reading) \(check (\d+)\)")
WINDOW = re.compile(r"heart: window (rejected: |faint: )?(.*)")
QUALITY = re.compile(r"quality:? (-?[\d.]+)")
SAMPLES = re.compile(r"heart: samples \[([\d, ]*)\] movement (\d+)")
READ = re.compile(r"heart: (\d+) BPM(?: after (\d+) s)?")
END = re.compile(r"heart: (no reading after|not on a wrist|too much light|on the charger; stopped|lost the sensor)")


def lines(paths):
    """(uptime, text) of every line, with None between boots and files."""
    for path in paths:
        yield None
        for line in open(path, encoding="utf-8", errors="replace"):
            line = line.rstrip("\n")
            if "\t" in line:
                parts = line.split("\t", 5)
                if len(parts) < 6:
                    continue
                try:
                    t, text = float(parts[3]), parts[5]
                except ValueError:
                    continue
            else:
                m = STAMPED.match(line)
                if not m:
                    continue
                t, text = float(m[1]), m[3]
            if text.startswith("ur ") and "booting" in text:
                yield None
            yield t, text


def sessions(paths):
    """Every awake background session with a `due` line, as a dict."""
    out, cur, boot, packed = [], None, 0, False
    for item in lines(paths):
        if item is None:
            boot += 1
            cur = None
            continue
        t, text = item
        if not text.startswith("heart: "):
            continue
        if "packed samples" in text:
            packed = True
        m = DUE.match(text)
        if m:
            cur = dict(boot=boot, t=t, why=m[1], budget=int(m[2]), round=int(m[3]), arm=m[4],
                       start=m[6], paired=None, why_not=None, void=False, phases={}, part=[],
                       readings=[], end=None)
            if cur["arm"] != "Asleep":
                out.append(cur)
            continue
        # A session is over within its 30 s (and a little, for the log's
        # own timing); what follows without a `due` line is someone
        # looking at the clock face.
        if cur is None or t - cur["t"] > 40:
            cur = None
            continue
        if m := PAIRED.match(text):
            cur["paired"] = (int(m[1]), int(m[2]))
        elif m := NOT_PAIRED.match(text):
            cur["why_not"] = m[1]
        elif text.startswith("heart: pair void"):
            cur["void"] = True
        elif m := PHASE.match(text):
            wins = [w for w in cur["part"] if w[0] == "w"]
            samples = [v for w in cur["part"] if w[0] == "s" for v in w[1]]
            moves = [w[2] for w in cur["part"] if w[0] == "s"]
            cur["phases"][int(m[1])] = dict(
                drive=int(m[2]), gain=int(m[3]), windows=int(m[4]), accepted=int(m[5]), faint=int(m[6]),
                bpm=int(m[7]) if m[7] else None, at=int(m[8]) if m[8] else None, check=int(m[9]),
                quality=[w[2] for w in wins if w[2] is not None], kinds=[w[1] for w in wins],
                samples=samples, movement=statistics.median(moves) if moves else None)
            cur["part"] = []
            # With a reading, the session ends with its pair.
            if int(m[1]) == 2 and cur["readings"]:
                cur = None
        elif m := SAMPLES.match(text):
            cur["part"].append(("s", [int(v) for v in m[1].split(", ") if v], int(m[2])))
        elif m := WINDOW.match(text):
            kind = "accepted" if not m[1] else "faint" if m[1].startswith("faint") else m[2].split()[0].rstrip(":{")
            q = QUALITY.search(m[2])
            cur["part"].append(("w", kind, float(q[1]) if q else None))
        elif END.match(text):
            cur["end"] = text[len("heart: "):]
            cur = None
        elif m := READ.match(text):
            cur["readings"].append(int(m[1]))
            # A reading ends a session, unless its pair is still under way.
            if not cur["paired"] or cur["void"] or 2 in cur["phases"]:
                cur = None
    if packed:
        print("note: this log has packed lines; run it through scripts/expand_log.py for the samples", file=sys.stderr)
    return out


# ------------------------------------------------------------------------------
# Statistics
# ------------------------------------------------------------------------------

def wilson(k, n):
    if n == 0:
        return (float("nan"),) * 2
    z, p = 1.96, k / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return centre - half, centre + half


def mcnemar(b, c):
    """Exact two-sided p for b against c discordant pairs."""
    n = b + c
    if n == 0:
        return 1.0
    tail = sum(math.comb(n, i) for i in range(min(b, c) + 1)) / 2 ** n
    return min(1.0, 2 * tail)


def bootstrap(pairs, stat, resamples, seed=1):
    """A 95% interval for `stat(pairs)`, resampling whole rounds."""
    rounds = {}
    for p in pairs:
        rounds.setdefault(p["key"], []).append(p)
    groups = list(rounds.values())
    if len(groups) < 2:
        return float("nan"), float("nan")
    rng = random.Random(seed)
    values = []
    for _ in range(resamples):
        sample = [p for g in rng.choices(groups, k=len(groups)) for p in g]
        v = stat(sample)
        if v is not None and not math.isnan(v):
            values.append(v)
    if len(values) < resamples // 2:
        return float("nan"), float("nan")
    values.sort()
    return values[int(0.025 * len(values))], values[min(len(values) - 1, int(0.975 * len(values)))]


def difference(pairs):
    if not pairs:
        return None
    return sum(p["bright"]["read"] - p["dim"]["read"] for p in pairs) / len(pairs)


def adjusted(pairs):
    """The mean of the two orders' differences."""
    by = [difference([p for p in pairs if p["first"] == o]) for o in ("dim", "bright")]
    return None if None in by else sum(by) / 2


def features(samples):
    """The pulse's size and the noise around it, from a phase's samples."""
    if np is None or len(samples) < SKIP + 200:
        return None
    x = np.asarray(samples[SKIP:SKIP + 318], float)
    n = len(x)
    i = np.arange(n)
    x = x - np.polyval(np.polyfit(i, x, 2), i)  # the slow drift
    f = np.fft.rfftfreq(n, 1 / FS)
    spectrum = np.fft.rfft(x)
    band, high = (f >= 0.7) & (f <= 3.0), f >= 5.0
    power = np.abs(np.fft.rfft(x * np.hanning(n))) ** 2
    line = f[band][power[band].argmax()]
    # The pulse: what's in its band, peak to peak (5th to 95th percentile).
    pulse = np.fft.irfft(np.where(band, spectrum, 0), n)
    size = float(np.percentile(pulse, 95) - np.percentile(pulse, 5))
    # The sensor's noise: what's above 5 Hz, where no pulse or wrist is.
    noise = float(np.fft.irfft(np.where(high, spectrum, 0), n).std())
    # In the band but off the pulse's line and its second harmonic (each
    # +-0.2 Hz): the wrist's own wobble, which scales with the light.
    off = band & (np.abs(f - line) > 0.2) & (np.abs(f - 2 * line) > 0.2)
    wobble = float(np.fft.irfft(np.where(off, spectrum, 0), n).std())
    return dict(dc=float(np.mean(samples[SKIP:])), size=size, noise=noise, wobble=wobble, line=line * 60)


def show_difference(name, pairs, resamples):
    n = len(pairs)
    if n == 0:
        print(f"  {name}: no pairs")
        return
    both = sum(p["bright"]["read"] and p["dim"]["read"] for p in pairs)
    b = sum(p["bright"]["read"] and not p["dim"]["read"] for p in pairs)
    c = sum(p["dim"]["read"] and not p["bright"]["read"] for p in pairs)
    lo, hi = bootstrap(pairs, difference, resamples)
    print(f"  {name}: {n} pairs; both read {both}, only bright {b}, only dim {c}, neither {n - both - b - c}")
    print(f"      bright phase read {both + b} ({(both + b) / n:.0%}), dim {both + c} ({(both + c) / n:.0%}); "
          f"difference {100 * (b - c) / n:+.1f} points [{100 * lo:+.1f}, {100 * hi:+.1f}], McNemar p {mcnemar(b, c):.3f}")


def paired_measure(name, pairs, value, resamples, ratio=False, unit=""):
    rows = [(value(p["bright"]), value(p["dim"]), p) for p in pairs]
    rows = [r for r in rows if r[0] is not None and r[1] is not None and (not ratio or r[1] > 0)]
    if len(rows) < 3:
        print(f"  {name}: too few pairs with both ({len(rows)})")
        return
    kept = [r[2] for r in rows]
    if ratio:
        def stat(sample):
            v = [value(p["bright"]) / value(p["dim"]) for p in sample]
            return statistics.median(v) if v else None
        word = "median ratio"
    else:
        def stat(sample):
            v = [value(p["bright"]) - value(p["dim"]) for p in sample]
            return statistics.fmean(v) if v else None
        word = "mean difference"
    lo, hi = bootstrap(kept, stat, resamples)
    higher = sum(r[0] > r[1] for r in rows)
    ties = sum(r[0] == r[1] for r in rows)
    sign = "" if ratio else "+"
    print(f"  {name}: bright {statistics.median(r[0] for r in rows):.3g}{unit}, dim {statistics.median(r[1] for r in rows):.3g}{unit} (medians, {len(rows)} pairs); "
          f"{word} {stat(kept):{sign}.3g} [{lo:{sign}.3g}, {hi:{sign}.3g}]; bright higher in {higher}, lower in {len(rows) - higher - ties} (sign test p {mcnemar(higher, len(rows) - higher - ties):.3f})")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("logs", nargs="+")
    parser.add_argument("--drives", default="0,2", help="the pair of drives to compare (default 0,2: 12.5 and 30 mA)")
    parser.add_argument("--resamples", type=int, default=4000)
    args = parser.parse_args()
    dim_drive, bright_drive = (int(v) for v in args.drives.split(","))

    every = sessions(args.logs)
    if not every:
        sys.exit("no awake background heart sessions in the log")
    span = Counter()
    for s in every:
        span[s["boot"]] = max(span[s["boot"]], s["t"])
    days = sum(span.values()) / 86400
    paired = [s for s in every if s["paired"]]
    print(f"{len(every)} awake background sessions over {days:.2f} days of uptime ({len(every) / days:.0f} a day)")
    print(f"  not paired: {dict(Counter(s['why_not'] or 'no line (an older build?)' for s in every if not s['paired']))}")
    print(f"  paired: {len(paired)} ({len(paired) / days:.0f} a day), by drives {dict(Counter(s['paired'] for s in paired))}")
    complete = [s for s in paired if len(s["phases"]) == 2 and not s["void"]]
    cut = [s for s in paired if len(s["phases"]) < 2 and not s["void"]]
    print(f"  ran their course: {len(complete)}; void (the level moved): {sum(s['void'] for s in paired)}; "
          f"cut short: {len(cut)} {dict(Counter((s['end'] or 'no end line').split(' (')[0].split(' after')[0] for s in cut))}")
    odd = [s for s in complete if any(ph["windows"] != 3 for ph in s["phases"].values())]
    if odd:
        print(f"  note: {len(odd)} pairs had a phase without exactly 3 windows (kept)")

    pairs = []
    for s in complete:
        one, two = s["phases"][1], s["phases"][2]
        if {one["drive"], two["drive"]} != {dim_drive, bright_drive} or one["gain"] != two["gain"]:
            continue
        first = "dim" if one["drive"] == dim_drive else "bright"
        for ph in (one, two):
            ph["read"] = ph["bpm"] is not None
            ph["best"] = max(ph["quality"], default=None)
            ph["features"] = features(ph["samples"])
        pairs.append(dict(key=(s["boot"], s["round"]), first=first, session=s,
                          dim=one if first == "dim" else two, bright=two if first == "dim" else one))
    print(f"\npairs of drive {dim_drive} against drive {bright_drive}: {len(pairs)}, over {len({p['key'] for p in pairs})} rounds")
    if not pairs:
        return

    print("\nDid a phase alone read? (bright minus dim, 95% interval by rounds)")
    show_difference("all", pairs, args.resamples)
    for order in ("dim", "bright"):
        show_difference(f"{order} first", [p for p in pairs if p["first"] == order], args.resamples)
    adj = adjusted(pairs)
    if adj is not None:
        lo, hi = bootstrap(pairs, adjusted, args.resamples)
        print(f"  order-adjusted difference: {100 * adj:+.1f} points [{100 * lo:+.1f}, {100 * hi:+.1f}]")
    other = {"dim": "bright", "bright": "dim"}
    later = sum(p[other[p["first"]]]["read"] and not p[p["first"]]["read"] for p in pairs)
    sooner = sum(p[p["first"]]["read"] and not p[other[p["first"]]]["read"] for p in pairs)
    print(f"  order by itself: the second phase read where the first didn't in {later} pairs, the first where the second didn't in {sooner}")

    print("\nRescues")
    for failed, other in (("dim", "bright"), ("bright", "dim")):
        missed = [p for p in pairs if not p[failed]["read"]]
        got = sum(p[other]["read"] for p in missed)
        lo, hi = wilson(got, len(missed))
        if missed:
            print(f"  the {failed} phase didn't read in {len(missed)}; the {other} one did in {got} of those ({got / len(missed):.0%} [{lo:.0%}, {hi:.0%}])")

    print("\nThe windows and the signal (bright against dim, same session)")
    paired_measure("best window quality", pairs, lambda ph: ph["best"], args.resamples)
    paired_measure("accepted windows", pairs, lambda ph: ph["accepted"], args.resamples)
    paired_measure("faint windows", pairs, lambda ph: ph["faint"], args.resamples)
    paired_measure("movement (median)", pairs, lambda ph: ph["movement"], args.resamples)
    feature = lambda k: (lambda ph: ph["features"][k] if ph["features"] else None)
    over = lambda a, b: (lambda ph: ph["features"][a] / ph["features"][b] if ph["features"] and ph["features"][b] > 0 else None)
    if np is None:
        print("  (numpy isn't installed: no pulse and noise from the samples)")
    else:
        paired_measure("light level", pairs, feature("dc"), args.resamples, ratio=True, unit=" counts")
        paired_measure("pulse, peak to peak", pairs, feature("size"), args.resamples, ratio=True, unit=" counts")
        paired_measure("noise above 5 Hz", pairs, feature("noise"), args.resamples, ratio=True, unit=" counts rms")
        paired_measure("in-band wobble", pairs, feature("wobble"), args.resamples, ratio=True, unit=" counts rms")
        paired_measure("pulse over noise", pairs, over("size", "noise"), args.resamples, ratio=True)
        paired_measure("pulse over wobble", pairs, over("size", "wobble"), args.resamples, ratio=True)

    agree = [abs(p["bright"]["bpm"] - p["dim"]["bpm"]) for p in pairs if p["bright"]["read"] and p["dim"]["read"]]
    print("\nWhere both phases read")
    if agree:
        print(f"  {len(agree)} pairs: rates apart by a median {statistics.median(agree):.0f} BPM, mean {statistics.fmean(agree):.1f}; "
              f"within 3: {sum(a <= 3 for a in agree)}, within 6: {sum(a <= 6 for a in agree)}, more than 10: {sum(a > 10 for a in agree)}")
    else:
        print("  none yet")

    print("\nWhat the test did to the reading")
    read = sum(bool(s["readings"]) for s in complete)
    lo, hi = wilson(read, len(complete))
    blo, bhi = wilson(*BEFORE_READ)
    print(f"  paired sessions that read at all: {read}/{len(complete)} ({read / len(complete):.0%} [{lo:.0%}, {hi:.0%}]); "
          f"before the test, calm starts read {BEFORE_READ[0]}/{BEFORE_READ[1]} ({BEFORE_READ[0] / BEFORE_READ[1]:.0%} [{blo:.0%}, {bhi:.0%}])")
    across = sum(bool(s["readings"]) and not any(ph["bpm"] for ph in s["phases"].values()) for s in complete)
    print(f"  of those, read by the session as a whole but by neither phase alone: {across}")

    n = len(pairs)
    b = sum(p["bright"]["read"] and not p["dim"]["read"] for p in pairs)
    c = sum(p["dim"]["read"] and not p["bright"]["read"] for p in pairs)
    psi = (b + c) / n
    rate = n / days if days > 0 else float("nan")
    print(f"\nHow much longer (the phases disagreed in {b + c} of {n} pairs, {psi:.0%}; {rate:.0f} pairs a day)")
    if psi == 0:
        print("  no disagreeing pair yet: too early to say")
        return
    for half in (0.04, 0.03):
        need = math.ceil(1.96 ** 2 * psi / half ** 2)
        print(f"  for the difference to within +-{100 * half:.0f} points: {need} pairs, {max(0, need - n)} more, {max(0, need - n) / rate:.1f} days")
    delta = 0.05
    if psi > delta ** 2:
        need = math.ceil((1.96 * math.sqrt(psi) + 0.8416 * math.sqrt(psi - delta ** 2)) ** 2 / delta ** 2)
        print(f"  to see a difference of {100 * delta:.0f} points four times in five: {need} pairs, {max(0, need - n)} more, {max(0, need - n) / rate:.1f} days")


if __name__ == "__main__":
    main()
