#!/usr/bin/env python3
"""Check on a dev kit that a crash keeps the activity the phone hasn't fetched.

    crash-keeps-activity.py ELF ADDRESS [--steps N] [--field-log]

ELF is a build with the `debug-input` feature, already flashed and running
(`cargo run --release --features debug-input`, then Ctrl-C once it boots),
and ADDRESS the dev kit's Bluetooth address, with this computer paired with
it (see scripts/dfu.py). This stands in for Gadgetbridge, through
scripts/gb-peer.py, and:

1. sets the dev kit's clock to this computer's, and fetches its activity,
   as Gadgetbridge does on connecting, so the watch knows where the
   phone's copy of its history ends;
2. walks N steps (input 15) in the minute under way;
3. waits for the next checkpoint (one every `CHECKPOINT_MINUTES`, read from
   src/snapshot.rs: see there), and a little longer;
4. crashes the dev kit with a HardFault (input 26, a precise bus fault),
   which resets it through the bootloader, as on a sealed watch;
5. reads the new boot's log over RTT, which must say that the run before
   hard-faulted, the clock resumed, today's steps resumed, and the
   activity came back from a checkpoint;
6. keeps walking a little, a few steps a minute (see below), and once
   the walk's 5-minute bucket is over, fetches the activity again, from
   before the walk, as Gadgetbridge would after the crash: the bucket
   must have the steps.

A build that keeps its log in the SPI flash (the `field-log` feature, with
`debug-input`) has moved the boot's lines there by the time step 5 looks,
and RTT has none of them: with `--field-log`, step 5 comes last instead,
and reads them from the log fetched over Bluetooth (scripts/fetch-log.py;
the first fetch from a watch takes minutes).

The steps come back whether the phone had fetched them or not: the walk's
minutes are in a checkpoint until their bucket is over, and from then on
in the activity log (see src/spill.rs), which a boot reads back whole.

Why it keeps walking: a fetch holds back the minutes from the start of
any stillness the wearer is in now, as that may yet turn out the start
of a sleep (see src/sleep_hold.rs), and a dev kit lies still on a desk.
Its heart rate sensor tells it that it isn't worn, which is never still,
but only in a dim room: by daylight (or a desk lamp) enough light
reaches the sensor of an open dev kit to pass for skin, and then the
minute after the walk starts a stillness, and the walk's bucket is held
back with it, for 25 minutes or so (this check then got nothing, where
it had passed the night before). A stillness ends with more than
`sleep::STIR` (3) minutes on the move, so the four minutes up to the
fetch get a few steps each. Those that land in the walk's bucket are
expected in it too. (At least those: the bucket may hold steps from
before this check, another run's, which it can't know of.)

A dev kit taken for asleep (its sensor told that the desk is skin, input
23, for 20 minutes) holds everything back until 2¼ hours after it
"wakes": this check fails then, and says so.

A dev kit has no night to keep; the simulator's
`a_crash_mid_night_keeps_the_night_and_the_unsynced_minutes` checks that.

Exits non-zero if anything above doesn't hold. Needs `esprobe`, `llvm-nm`,
`defmt-print` and `uv` (which brings bleak for gb-peer.py) on PATH.
"""

import argparse
import json
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ["esprobe", "--target", "nRF52832_xxAA"]
# Debug inputs (see src/debug.rs): steps walked, `x` + 256 × `y`; and a
# HardFault, precise with `x` 0.
STEPS = 15
HARDFAULT = 26
# How long the bootloader and the boot take, before the log's worth reading.
BOOT_SECONDS = 15
# The most the dev kit's clock is behind after the crash: it resumes from
# where it was (`clock::restore`), less the bootloader's seconds (its logo,
# and checking the image), which nothing counts; some 9 s. The phone puts
# it right when it next sets the clock.
BEHIND_SECONDS = 15
# Gadgetbridge's buckets, in minutes (`gb_buckets::MINUTES`).
BUCKET = 5
# Minutes on the move, up to the one under way, that end a stillness for
# what a fetch holds back: one more than a stir (`sleep::STIR`).
MOVING_MINUTES = 4
# Steps in each of them: any would do, and these also count as walking
# (`sleep::WAKE_STEPS`).
MOVING_STEPS = 12


def symbol(elf: Path, name: str) -> int:
    out = subprocess.run(["llvm-nm", str(elf)], capture_output=True, text=True, check=True)
    for line in out.stdout.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[2] == name:
            return int(parts[0], 16)
    if name == "UR_INPUT":
        sys.exit(f"no input in {elf}: build it with `--features debug-input`")
    sys.exit(f"no symbol {name} in {elf}")


def checkpoint_minutes() -> int:
    source = (ROOT / "src" / "snapshot.rs").read_text()
    found = re.search(r"pub const CHECKPOINT_MINUTES: i64 = (\d+);", source)
    if found is None:
        sys.exit("no CHECKPOINT_MINUTES in src/snapshot.rs")
    return int(found.group(1))


def inject(elf: Path, kind: int, x: int = 0, y: int = 0) -> None:
    value = kind | x << 8 | y << 16
    subprocess.run(
        TARGET + ["core", "write", hex(symbol(elf, "UR_INPUT")), hex(value)],
        capture_output=True,
        check=True,
    )


def walk(elf: Path, steps: int, walked: dict[int, int], behind: int = 0) -> int:
    """Walks `steps` now, well inside a minute by this computer's clock
    (which the dev kit's was set to, and may since have fallen `behind`
    by up to that many seconds), so that it's known which minute's they
    are: that minute, with the steps added to it in `walked`."""
    # Not in a minute's first seconds or its last: the input is taken
    # within a tenth of a second, but the clocks may differ by one.
    second = time.time() % 60
    if second < 3 + behind:
        time.sleep(3 + behind - second)
    elif second > 55:
        time.sleep(63 + behind - second)
    minute = int(time.time() // 60)
    inject(elf, STEPS, steps & 0xFF, steps >> 8)
    walked[minute] = walked.get(minute, 0) + steps
    return minute


def boot_log(elf: Path) -> list[str]:
    """The dev kit's log since it booted, as far as RTT's buffer kept it
    (defmt-rtt drops what doesn't fit, so the start survives)."""
    control_block = hex(symbol(elf, "_SEGGER_RTT"))
    rtt = subprocess.Popen(
        TARGET + ["rtt", "--control-block", control_block, "--idle-ms", "1500"],
        stdout=subprocess.PIPE,
    )
    decoded = subprocess.run(
        ["defmt-print", "-e", str(elf)],
        stdin=rtt.stdout,
        capture_output=True,
        text=True,
        check=False,
    )
    rtt.wait()
    return decoded.stdout.splitlines()


def fetched_log(elf: Path, address: str) -> list[str]:
    """The last boot's lines of the dev kit's field log, fetched over
    Bluetooth (scripts/fetch-log.py, which keeps what it fetched in its
    cache, so only the first fetch from a watch is long)."""
    with tempfile.TemporaryDirectory() as out:
        subprocess.run(
            ["uv", "run", "--quiet", "--with", "bleak", "python"]
            + [str(ROOT / "scripts" / "fetch-log.py"), "--address", address, "--out", out],
            stdout=subprocess.DEVNULL,
            check=True,
        )
        # One file per firmware version in the log, written oldest first:
        # the last is this build's.
        files = sorted(Path(out).glob("field-log-*.defmt"), key=lambda f: f.stat().st_mtime_ns)
        if not files:
            return []
        with files[-1].open("rb") as stream:
            decoded = subprocess.run(
                ["defmt-print", "-e", str(elf)],
                stdin=stream,
                capture_output=True,
                text=True,
                check=False,
            )
    lines = decoded.stdout.splitlines()
    boots = [i for i, line in enumerate(lines) if line.rstrip().endswith(" booting")]
    return lines[boots[-1] :] if boots else lines


def check_boot(log: list[str], failures: list[str], where: str) -> None:
    """What the boot after the crash must have logged."""
    for line in log:
        if any(k in line for k in ("reset reason", "hard-faulted", "resumed", "snapshot:", "spill:")):
            print(f"  {line}", flush=True)
    for expected in (
        "the previous run hard-faulted",
        "clock: resumed after reset",
        "activity: resumed with",
        "(a checkpoint)",
    ):
        if not any(expected in line for line in log):
            failures.append(f"the boot's log has no {expected!r} ({where})")


class Peer:
    """scripts/gb-peer.py, as Gadgetbridge: lines to the watch, and the
    lines it sends back."""

    def __init__(self, address: str) -> None:
        self.child = subprocess.Popen(
            ["uv", "run", "--quiet", "--with", "bleak", "python"]
            + [str(ROOT / "scripts" / "gb-peer.py"), address],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        self.said: queue.Queue[str] = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.partial = b""
        self.lines: list[str] = []

    def _read(self) -> None:
        assert self.child.stdout is not None
        for line in self.child.stdout:
            self.said.put(line.strip())

    def _take(self, said: str) -> bool:
        """Files what gb-peer.py said; whether it answered a request."""
        if said.startswith("rx "):
            self.partial += bytes.fromhex(said[3:])
            *whole, self.partial = self.partial.split(b"\n")
            self.lines += [line.decode("latin-1").strip() for line in whole]
            return False
        return said == "ok" or said.startswith("error")

    def request(self, command: str, timeout: float = 60) -> None:
        assert self.child.stdin is not None
        self.child.stdin.write(command + "\n")
        self.child.stdin.flush()
        deadline = time.monotonic() + timeout
        while True:
            said = self.said.get(timeout=max(0.1, deadline - time.monotonic()))
            if self._take(said):
                if said != "ok":
                    sys.exit(f"gb-peer.py: {command.split()[0]}: {said}")
                return

    def send(self, line: str) -> None:
        wire = b"\x10" + line.encode("latin-1") + b"\n"
        self.request(f"send {wire.hex()}")

    def wait_for(self, needle: str, timeout: float = 60) -> list[str]:
        """The lines the watch sent up to one containing `needle`."""
        deadline = time.monotonic() + timeout
        while not any(needle in line for line in self.lines):
            try:
                self._take(self.said.get(timeout=max(0.1, deadline - time.monotonic())))
            except queue.Empty:
                sys.exit(f"the watch never sent {needle!r}; it sent: {self.lines}")
        end = next(i for i, line in enumerate(self.lines) if needle in line)
        taken, self.lines = self.lines[: end + 1], self.lines[end + 1 :]
        return taken

    def quit(self) -> None:
        try:
            self.request("quit", timeout=10)
        finally:
            self.child.wait(timeout=10)


def set_time(peer: Peer) -> None:
    """Gadgetbridge's line setting the clock, as it sends it on connecting."""
    utc = int(time.time())
    zone = -time.altzone if time.localtime().tm_isdst > 0 else -time.timezone
    hours = zone / 3600
    peer.send(
        f"setTime({utc});E.setTimeZone({hours:.1f});(s=>s&&(s.timezone={hours:.1f},"
        "require('Storage').write('setting.json',s)))(require('Storage').readJSON('setting.json',1))"
    )


def fetch(peer: Peer, since_ms: int) -> list[dict]:
    """An activity fetch from `since_ms`, as Gadgetbridge asks for one: the
    samples the watch sends."""
    request = json.dumps({"t": "actfetch", "ts": since_ms}, separators=(",", ":"))
    peer.send(f"GB({request})")
    lines = peer.wait_for('"t":"actfetch","state":"end"')
    samples = []
    for line in lines:
        start = line.find("{")
        if start >= 0 and line[start:].startswith('{"t":"act",'):
            samples.append(json.loads(line[start:]))
    return samples


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf", type=Path)
    parser.add_argument("address")
    parser.add_argument("--steps", type=int, default=1234, help="steps to walk (up to 65535)")
    parser.add_argument(
        "--field-log",
        action="store_true",
        help="read the boot's lines from the field log (a `field-log` build)",
    )
    args = parser.parse_args()
    if not 0 < args.steps < 1 << 16:
        sys.exit("--steps: 1 to 65535")
    every = checkpoint_minutes()
    failures: list[str] = []

    print("connecting, setting the clock, and fetching", flush=True)
    peer = Peer(args.address)
    peer.request("connect")
    set_time(peer)
    time.sleep(2)
    fetch(peer, int(time.time() - 3600) * 1000)

    # Every step walked, by minute.
    walked: dict[int, int] = {}
    walked_minute = walk(args.elf, args.steps, walked)
    print(f"walking {args.steps} steps at minute {walked_minute}", flush=True)

    # The first checkpoint after the walk is at the next minute that's a
    # multiple of `every` (the minute's second poll, two seconds in).
    due = (walked_minute // every + 1) * every
    wait = due * 60 + 20 - time.time()
    print(f"waiting {wait:.0f} s for the checkpoint at minute {due}", flush=True)
    time.sleep(max(0, wait))

    print("crashing it (a HardFault)", flush=True)
    peer.request("disconnect")
    inject(args.elf, HARDFAULT)
    time.sleep(BOOT_SECONDS)
    # On the move from here to the fetch (see the module docs), starting
    # with what's left of this minute: only after the crash, which would
    # lose the steps since the checkpoint, and so muddle the count.
    moving_from = walk(args.elf, MOVING_STEPS, walked, BEHIND_SECONDS)
    if not args.field_log:
        check_boot(boot_log(args.elf), failures, "or RTT's buffer dropped it")

    # Gadgetbridge's fetch sends whole buckets, so the fetch waits for the
    # walk's to end, and for enough minutes on the move.
    bucket_end = (walked_minute // BUCKET + 1) * BUCKET
    fetch_minute = max(bucket_end, moving_from + MOVING_MINUTES - 1)
    print(
        f"walking {MOVING_STEPS} steps a minute until the fetch at minute {fetch_minute}, "
        "so the dev kit isn't lying still",
        flush=True,
    )
    for minute in range(moving_from + 1, fetch_minute + 1):
        time.sleep(max(0, minute * 60 - time.time()))
        walk(args.elf, MOVING_STEPS, walked, BEHIND_SECONDS)
    # With the last of them counted, and the dev kit's clock in this
    # minute too.
    time.sleep(max(0, fetch_minute * 60 + BEHIND_SECONDS + 10 - time.time()))
    if int(time.time() // 60) != fetch_minute:
        failures.append("this computer was too slow to fetch in the last minute walked")
    print("reconnecting, and fetching from before the walk", flush=True)
    peer.request("connect")
    time.sleep(2)
    samples = fetch(peer, (walked_minute - 10) * 60_000)
    peer.quit()
    if args.field_log:
        print("fetching the field log, for the boot's lines", flush=True)
        time.sleep(5)
        check_boot(fetched_log(args.elf, args.address), failures, "in the fetched field log")
    bucket = range(walked_minute // BUCKET * BUCKET, bucket_end)
    bucket_ms = (bucket.start * 60_000, bucket.stop * 60_000)
    in_bucket = [s for s in samples if bucket_ms[0] <= s.get("ts", 0) < bucket_ms[1]]
    steps = sum(s.get("stp", 0) for s in in_bucket)
    expected = sum(walked.get(minute, 0) for minute in bucket)
    print(f"the walk's bucket has {steps} steps, of {expected} walked here", flush=True)
    if not in_bucket:
        failures.append(
            f"the fetch has no bucket for the walk: {samples}. If the dev kit was taken "
            "for asleep (its sensor told the desk is skin?), the fetch holds everything "
            "back until 2¼ hours after: see src/sleep_hold.rs"
        )
    elif steps < expected:
        failures.append(f"the walk's bucket has {steps} steps, not {expected}: {samples}")

    for failure in failures:
        print(f"FAILED: {failure}", flush=True)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
