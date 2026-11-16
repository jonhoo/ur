"""The step counter's runs of counting in a decoded field log: how each
opened, and how long each was.

The accelerometer's counter holds back a walk's first steps until a few
arm swings in a row have shown it's one, then adds them at once (see
`STEP_BUFFER_SIZE` in src/activity.rs). So a run of counting (polls with
steps, with at most one poll without between them) opens with a burst:
8 to 12 steps with Bosch's four swings, 6 to 10 with three. This prints:

- the runs by their opening burst: the smallest says how many swings the
  chip waits for, whatever the watch asked of it;
- the runs by their total steps: those at the smallest total are walks
  (or arm movements) that only just passed, which one swing more would
  have dropped. Their steps are what the last swing less bought, false
  steps included; a log can't tell those apart, but they are mostly the
  runs with nothing else near them.

Usage: python3 -I scripts/step_runs.py DECODED_LOG...

Each log as `scripts/expand_log.py` writes it (see CONTRIBUTING's Field
logs). Runs don't carry over from one log, or one boot, to the next.
"""
import re
import sys
from collections import Counter

LINE = re.compile(r"^(\d+\.\d+) \w+\s+activity: \d+ samples, movement \d+, (\d+) new steps")
# Polls are 2 s apart; a longer gap than this (a failed read, a reboot)
# ends a run.
MAX_GAP = 5.0

opening, total = Counter(), Counter()
alone = Counter()  # runs with no other within a minute either side
runs = []  # (start, end, opening, total)
for path in sys.argv[1:]:
    run, gap, prev = None, 0, None
    for line in open(path, encoding="utf-8", errors="replace"):
        m = LINE.match(line)
        if not m:
            continue
        up, steps = float(m[1]), int(m[2])
        if run and prev is not None and not 0 <= up - prev <= MAX_GAP:
            runs.append(tuple(run))
            run = None
        if prev is not None and up < prev:
            # A new boot: uptime starts over. Keep runs apart across it.
            runs.append(None)
        prev = up
        if steps > 0:
            if run is None:
                run = [up, up, steps, 0]
            run[1] = up
            run[3] += steps
            gap = 0
        elif run:
            gap += 1
            if gap > 1:
                runs.append(tuple(run))
                run = None
    if run:
        runs.append(tuple(run))
    runs.append(None)

for i, run in enumerate(runs):
    if run is None:
        continue
    start, end, first, steps = run
    opening[first] += 1
    total[steps] += 1
    before = runs[i - 1] if i else None
    after = runs[i + 1] if i + 1 < len(runs) else None
    if not (before and start - before[1] < 60) and not (after and after[0] - end < 60):
        alone[steps] += 1

count = sum(total.values())
steps = sum(k * v for k, v in total.items())
print(f"{count} runs, {steps} steps")
if not count:
    sys.exit()
print("runs by opening burst:")
for k in sorted(opening):
    print(f"  {k:3d} steps: {opening[k]:5d} ({100 * opening[k] / count:3.0f}%)")
print("runs by total steps (and those with no other run within a minute):")
for k in sorted(k for k in total if k <= 30):
    print(f"  {k:3d} steps: {total[k]:5d} ({alone[k]} alone)")
longer = sum(v for k, v in total.items() if k > 30)
print(f"  more:      {longer:5d}")
least = min(total)
print(
    f"runs of {least} steps, the least: {total[least]} runs, {least * total[least]} steps "
    f"({100 * least * total[least] / steps:.1f}% of all), {least * alone[least]} of them in runs alone"
)
