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
6. once the walk's 5-minute bucket is over, fetches the activity again,
   from before the walk, as Gadgetbridge would after the crash: it must
   have the steps.

A build that keeps its log in the SPI flash (the `field-log` feature, with
`debug-input`) has moved the boot's lines there by the time step 5 looks,
and RTT has none of them: with `--field-log`, step 5 comes last instead,
and reads them from the log fetched over Bluetooth (scripts/fetch-log.py;
the first fetch from a watch takes minutes).

The steps come back whether the phone had fetched them or not: the walk's
minutes are in a checkpoint until their bucket is over, and from then on
in the activity log (see src/spill.rs), which a boot reads back whole.

A dev kit lies on a desk, so it has no night to keep; the simulator's
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
# Gadgetbridge's buckets, in minutes (`gb_buckets::MINUTES`).
BUCKET = 5


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

    walked = time.time()
    walked_minute = int(walked // 60)
    print(f"walking {args.steps} steps at minute {walked_minute}", flush=True)
    inject(args.elf, STEPS, args.steps & 0xFF, args.steps >> 8)

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
    if not args.field_log:
        check_boot(boot_log(args.elf), failures, "or RTT's buffer dropped it")

    # Gadgetbridge's fetch sends whole buckets, so wait for the walk's to end.
    bucket_end = (walked_minute // BUCKET + 1) * BUCKET
    wait = bucket_end * 60 + 10 - time.time()
    if wait > 0:
        print(f"waiting {wait:.0f} s for the walk's bucket to end", flush=True)
        time.sleep(wait)
    print("reconnecting, and fetching from before the walk", flush=True)
    peer.request("connect")
    time.sleep(2)
    samples = fetch(peer, (walked_minute - 10) * 60_000)
    peer.quit()
    if args.field_log:
        print("fetching the field log, for the boot's lines", flush=True)
        time.sleep(5)
        check_boot(fetched_log(args.elf, args.address), failures, "in the fetched field log")
    bucket_ms = (walked_minute // BUCKET * BUCKET * 60_000, bucket_end * 60_000)
    steps = sum(s.get("stp", 0) for s in samples if bucket_ms[0] <= s.get("ts", 0) < bucket_ms[1])
    print(f"the walk's bucket has {steps} steps, of {args.steps}", flush=True)
    if steps < args.steps:
        failures.append(f"the walk's bucket has {steps} steps, not {args.steps}: {samples}")

    for failure in failures:
        print(f"FAILED: {failure}", flush=True)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
