"""Tests for scripts/expand_log.py: packing a decoded log's lines as a field
build (0.12 on) logs them, then expanding them, gives back what the
scripts and replays read.

    python3 -I scripts/expand_log_test.py               # the built-in cases
    python3 -I scripts/expand_log_test.py decoded.log   # a 0.11.x log too

With a log (`defmt-print` output from a build before the packing), this
packs it as a field build would have logged it, expands that, and checks
that every poll's samples, movement, steps and counter, and every other
line's text, come back; it prints how far the polls' times moved.
"""
import importlib.util
import io
import re
import sys
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("expand_log", Path(__file__).with_name("expand_log.py"))
expand_log = importlib.util.module_from_spec(spec)
spec.loader.exec_module(expand_log)

POLL = re.compile(r"activity: (\d+) samples, movement (\d+), (\d+) new steps "
                  r"\(counter (\d+)\), last \((-?\d+), (-?\d+), (-?\d+)\)$")
LEVEL = re.compile(r"heart: level Level \{ drive: (\d+), gain: (\d+) \}, mean \d+, peak \d+, "
                   r"ambient (\d+), period (\S+) µs$")
SAMPLES = re.compile(r"heart: samples \[([\d, ]*)\] movement (\d+)$")

# The byte sequence host-tests/log_pack checks `log_pack::pack_samples`
# writes for these samples.
SHARED_SAMPLES = [3731, 3860, 3856, 3851, 3600, 3601, 3474, 3601, 3473]
SHARED_PACKED = [0x80, 0x93, 0x0E, 0x80, 0x14, 0x0F, 0xFC, 0xFB, 0x80, 0x10, 0x0E, 0x01, 0x81,
                 0x7F, 0x80, 0x91, 0x0D]


def pack(samples):
    """`log_pack::pack_samples`, for the tests."""
    out, before = [], None
    for sample in samples:
        delta = None if before is None else sample - before
        out += [delta % 256] if delta is not None and -127 <= delta <= 127 else [0x80, sample & 0xFF, sample >> 8]
        before = sample
    return out


def split(line):
    """(ms of uptime, the time as written, level, message), or None."""
    m = expand_log.LINE.match(line)
    if not m:
        return None
    seconds, fraction, level, text = m.groups()
    return int(seconds) * 1000 + int(fraction.ljust(3, "0")[:3]), f"{seconds}.{fraction[:3]}", level, text


def packed(lines):
    """A decoded log's lines as a field build logs them, with times in ms:
    each minute's polls as one line, each level check and the samples after
    it as one, each other part-second's samples packed. A minute left part
    full at the end is lost, as on the watch."""
    out, polls, i = [], [], 0
    while i < len(lines):
        parts = split(lines[i])
        if parts is None:
            out.append(lines[i])
            i += 1
            continue
        ms, stamp, level, text = parts
        if m := POLL.match(text):
            polls.append((ms, [int(g) for g in m.groups()]))
            if len(polls) == 30:
                last = polls[-1][1]
                movement = ", ".join(str(min(p[1], 65535)) for _, p in polls)
                steps = ", ".join(str(p[2]) for _, p in polls)
                samples = ", ".join(str((p[0] - 24) % 256) for _, p in polls)
                out.append(f"{stamp} DEBUG activity: polls over {ms - polls[0][0]} ms: movement "
                           f"[{movement}], steps [{steps}], samples-24 [{samples}], counter {last[3]}, "
                           f"last ({last[4]}, {last[5]}, {last[6]})")
                polls = []
        elif (m := LEVEL.match(text)) and i + 1 < len(lines) and (s := SAMPLES.search(lines[i + 1])):
            drive, gain, ambient, period = m.groups()
            bytes_ = ", ".join(map(str, pack(expand_log.numbers(s[1]))))
            out.append(f"{stamp} TRACE heart: level {drive}/{gain}, ambient {ambient}, period {period} µs, "
                       f"packed samples [{bytes_}] movement {s[2]}")
            i += 1
        elif s := SAMPLES.match(text):
            bytes_ = ", ".join(map(str, pack(expand_log.numbers(s[1]))))
            out.append(f"{stamp} {level} heart: packed samples [{bytes_}] movement {s[2]}")
        else:
            out.append(f"{stamp} {level} {text}")
        i += 1
    return out


def expanded(lines):
    out = io.StringIO()
    expander = expand_log.Expander(out)
    for line in lines:
        expander.line(line)
    expander.flush()
    return out.getvalue().splitlines()


def compare(original, back):
    """Checks `back` (packed, then expanded) against `original`, and
    returns how far each poll's time moved, in ms."""
    def polls(lines):
        return [(split(l)[0], [int(g) for g in POLL.match(split(l)[3]).groups()])
                for l in lines if split(l) and POLL.match(split(l)[3])]

    def others(lines):
        return [(split(l)[2], split(l)[3]) if split(l) else (None, l)
                for l in lines if not (split(l) and POLL.match(split(l)[3]))]

    before, after = polls(original), polls(back)
    # The polls of a last, part-full minute are lost.
    assert len(before) - len(after) < 30, (len(before), len(after))
    for (_, b), (_, a) in zip(before, after):
        assert b[:4] == a[:4], (b, a)
    assert others(original) == others(back)
    return [abs(b[0] - a[0]) for b, a in zip(before, after)]


class Tests(unittest.TestCase):
    def test_unpacks_the_shared_vector(self):
        self.assertEqual(expand_log.unpack(SHARED_PACKED), SHARED_SAMPLES)
        self.assertEqual(pack(SHARED_SAMPLES), SHARED_PACKED)

    def test_round_trips_a_session_and_two_minutes(self):
        lines = ["0.000 INFO  ur 0.12.0 booting", "1.500 INFO  clock: 1794597622 UTC = 2026-11-13 20:20:22 local"]
        counter = 100
        for poll in range(61):
            t = 2000 + 2009 * poll
            steps = 4 if poll % 7 == 0 else 0
            counter += steps
            lines.append(f"{t // 1000}.{t % 1000:03d} DEBUG activity: {24 + (poll % 9 == 0) - (poll % 11 == 0)} "
                         f"samples, movement {poll * 371 % 2500}, {steps} new steps (counter {counter}), "
                         f"last (865, -134, {590 + poll})")
            if 20 <= poll < 23:
                samples = [3800 + (k * 37) % 300 - (k * 13) % 200 for k in range(27 - poll + 20)]
                lines.append(f"{t // 1000}.{t % 1000 + 5:03d} DEBUG heart: level Level {{ drive: 2, gain: 1 }}, "
                             f"mean {sum(samples) // len(samples)}, peak {max(samples)}, ambient 68, "
                             f"period 36722.945 µs")
                lines.append(f"{t // 1000}.{t % 1000 + 5:03d} TRACE heart: samples "
                             f"[{', '.join(map(str, samples))}] movement 42")
            if poll == 23:
                lines.append(f"{t // 1000}.{t % 1000 + 7:03d} TRACE heart: samples [] movement 42")
                lines.append(f"{t // 1000}.{t % 1000 + 7:03d} INFO  heart: 74 BPM after 40 s")
        condensed = packed(lines)
        self.assertEqual(sum("polls over" in l for l in condensed), 2)
        self.assertEqual(sum("packed samples" in l for l in condensed), 4)
        back = expanded(condensed)
        moved = compare(lines, back)
        self.assertEqual(len(moved), 60)
        self.assertLessEqual(max(moved), 1)
        # The other lines in their places: the reading after the session's
        # last samples, and both after the poll before them.
        reading = next(i for i, l in enumerate(back) if "BPM" in l)
        self.assertIn("heart: samples []", back[reading - 1])
        self.assertIn("activity:", back[reading - 2])

    def test_passes_an_old_log_through(self):
        lines = ["27919.673919 DEBUG activity: 25 samples, movement 76, 0 new steps (counter 2712), last (865, -134, 592)",
                 "27922.012176 DEBUG activity: still",
                 "└─ ur::activity @ src/activity.rs:1096",
                 "27921.683319 DEBUG heart: level Level { drive: 2, gain: 1 }, mean 3820, peak 3860, ambient 68, period 36722.945 µs",
                 "1.000000 INFO  ur 0.11.1 booting"]
        self.assertEqual(expanded(lines), lines)

    def test_leaves_a_capture_builds_minutes(self):
        lines = [f"{10 + 2 * k}.000 TRACE activity: 24 samples, movement {k}, 0 new steps (counter 5), last (1, 2, 3)"
                 for k in range(30)]
        minute = ("68.000 DEBUG activity: polls over 58000 ms: movement [" + ", ".join(map(str, range(30)))
                  + "], steps [" + ", ".join(["0"] * 30) + "], samples-24 [" + ", ".join(["0"] * 30)
                  + "], counter 5, last (1, 2, 3)")
        back = expanded(lines + [minute])
        self.assertEqual(back, [l.replace("TRACE", "DEBUG") for l in lines] + [minute])


def main():
    if len(sys.argv) > 1:
        lines = [l.rstrip("\n") for l in open(sys.argv[1], encoding="utf-8")]
        # As a field build stamps them: ms.
        lines = [f"{p[1]} {p[2]} {p[3]}" if (p := split(l)) else l for l in lines]
        condensed = packed(lines)
        moved = sorted(compare(lines, expanded(condensed)))
        print(f"{len(lines)} lines packed into {len(condensed)}, expanded back: polls' samples, "
              f"movement, steps and counter, and every other line's text, as they were; "
              f"{len(moved)} polls' times moved by {moved[len(moved) // 2]} ms (median), "
              f"{moved[-1]} ms at most")
    else:
        unittest.main()


if __name__ == "__main__":
    main()
