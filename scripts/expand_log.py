"""Expand a decoded field log's packed lines into the lines they stand for.

A field build (0.12 on) packs its busiest lines (see src/log_pack.rs):

- `activity: polls over S ms: movement [..], steps [..], samples-24 [..],
  counter C, last (x, y, z)`: a minute's accelerometer polls, which other
  builds log one line each, `activity: N samples, movement M, S new steps
  (counter C), last (x, y, z)`.
- `heart: level D/G, ambient A, period P µs, packed samples [..] movement
  M`: a second's level check and samples, which other builds log as
  `heart: level Level { drive: D, gain: G }, mean …, peak …, ambient A,
  period P µs` and `heart: samples [..] movement M`.
- `heart: packed samples [..] movement M`: a part-second's samples,
  `heart: samples [..] movement M` elsewhere.

This writes those lines back as the others, so every script and replay
that reads a decoded log works on either. Every other line, and every line
of an older build's log, passes through as it is.

What can't come back exactly:

- A poll's time: its line is placed between the first poll's time and the
  minute's line's, evenly (polls are 2 s apart, give or take the timer),
  among the other lines by time.
- A poll's step counter and orientation: the minute's line has the last
  poll's; each poll gets the counter less the steps after it, and the last
  orientation.

A capture build (`UR_ACTIVITY_LOG=trace`) logs each poll's own line too
(at trace): a minute's line after polls of its own is left as it is, and
those polls' lines are relabelled `DEBUG`, as other builds log them.

Usage: defmt-print -e ur-logging-<version>.elf < field-log.defmt | \\
           python3 -I scripts/expand_log.py > decoded.log
       python3 -I scripts/expand_log.py decoded-packed.log > decoded.log
"""
import re
import sys

# `defmt-print`'s lines: the uptime in seconds, the level, the message.
LINE = re.compile(r"^(\d+)\.(\d+) (TRACE|DEBUG|INFO |WARN |ERROR) (.*)$")
POLLS = re.compile(
    r"activity: polls over (\d+) ms: movement \[([\d, ]*)\], steps \[([\d, ]*)\], "
    r"samples-24 \[([\d, ]*)\], counter (\d+), last \((-?\d+), (-?\d+), (-?\d+)\)$")
POLL = re.compile(r"activity: \d+ samples, movement \d+, \d+ new steps")
LEVEL = re.compile(
    r"heart: level (\d+)/(\d+), ambient (\d+), period (\S+) µs, "
    r"packed samples \[([\d, ]*)\] movement (\d+)$")
PACKED = re.compile(r"heart: packed samples \[([\d, ]*)\] movement (\d+)$")

# src/log_pack.rs: `ESCAPE` and `SAMPLES_FROM`.
ESCAPE = 0x80
SAMPLES_FROM = 24


def numbers(text):
    return [int(n) for n in text.split(", ") if n]


def unpack(packed):
    """`log_pack::pack_samples`' bytes back into samples."""
    samples, i = [], 0
    while i < len(packed):
        if packed[i] == ESCAPE:
            samples.append(packed[i + 1] | packed[i + 2] << 8)
            i += 3
        else:
            delta = packed[i] - 256 if packed[i] > 127 else packed[i]
            samples.append(samples[-1] + delta)
            i += 1
    return samples


def samples_line(samples, movement):
    return f"heart: samples [{', '.join(map(str, samples))}] movement {movement}"


class Expander:
    """Lines in, lines out. Lines wait here from one minute's polls to the
    next, so that the polls can go back among them by time."""

    def __init__(self, out):
        self.out = out
        # (ms of uptime, line) since the last minute's line.
        self.waiting = []
        self.last_ms = None
        # Whether a poll's own line is among them (a capture build's).
        self.own_polls = False

    def line(self, line):
        m = LINE.match(line)
        if not m:
            # Not a log line (defmt-print's own messages, a location):
            # where it was, with the line before it.
            self.waiting.append((self.last_ms or 0, line))
            return
        seconds, fraction, level, text = m.groups()
        ms = int(seconds) * 1000 + int(fraction.ljust(3, "0")[:3])
        if self.last_ms is not None and ms < self.last_ms:
            # A new boot: whatever was waiting is the last boot's.
            self.flush()
        self.last_ms = ms
        stamp = f"{seconds}.{fraction}"
        if POLL.match(text):
            self.own_polls = True
            relabelled = f"{stamp} DEBUG {text}" if level == "TRACE" else line
            self.waiting.append((ms, relabelled))
        elif p := POLLS.match(text):
            self.polls(ms, stamp, line, p)
        elif p := LEVEL.match(text):
            drive, gain, ambient, period, packed, movement = p.groups()
            samples = unpack(numbers(packed))
            mean = sum(samples) // len(samples) if samples else 0
            peak = max(samples, default=0)
            self.waiting.append((ms, f"{stamp} DEBUG heart: level Level {{ drive: {drive}, "
                                     f"gain: {gain} }}, mean {mean}, peak {peak}, "
                                     f"ambient {ambient}, period {period} µs"))
            self.waiting.append((ms, f"{stamp} TRACE {samples_line(samples, movement)}"))
        elif p := PACKED.match(text):
            packed, movement = p.groups()
            self.waiting.append((ms, f"{stamp} {level} {samples_line(unpack(numbers(packed)), movement)}"))
        else:
            self.waiting.append((ms, line))

    def polls(self, ms, stamp, line, p):
        if self.own_polls:
            # A capture build's: its polls are already here, each its own.
            self.waiting.append((ms, line))
            self.flush()
            return
        span = int(p[1])
        movement, steps, samples = numbers(p[2]), numbers(p[3]), numbers(p[4])
        counter = int(p[5])
        x, y, z = p[6], p[7], p[8]
        n = len(movement)
        # As many decimals as the log's own times (3, for ms).
        padding = "0" * (len(stamp.split(".")[1]) - 3)
        polls = []
        for i in range(n):
            at = ms - (round(span * (n - 1 - i) / (n - 1)) if n > 1 else 0)
            seconds = f"{at // 1000}.{at % 1000:03d}{padding}"
            # The counter as it was then: less the steps that came after.
            then = max(counter - sum(steps[i + 1:]), 0)
            polls.append((at, f"{seconds} DEBUG activity: {(samples[i] + SAMPLES_FROM) % 256} samples, "
                              f"movement {movement[i]}, {steps[i]} new steps (counter {then}), "
                              f"last ({x}, {y}, {z})"))
        # Each poll before the first waiting line that's later than it; the
        # minute's own, at its time, after everything waiting.
        merged, j = [], 0
        for wms, wline in self.waiting:
            while j < len(polls) - 1 and polls[j][0] < wms:
                merged.append(polls[j])
                j += 1
            merged.append((wms, wline))
        merged.extend(polls[j:])
        self.waiting = merged
        self.flush()

    def flush(self):
        for _, line in self.waiting:
            self.out.write(line + "\n")
        self.waiting = []
        self.own_polls = False


def main():
    source = open(sys.argv[1], encoding="utf-8") if len(sys.argv) > 1 else sys.stdin
    expander = Expander(sys.stdout)
    for line in source:
        expander.line(line.rstrip("\n"))
    expander.flush()


if __name__ == "__main__":
    main()
