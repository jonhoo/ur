#!/usr/bin/env python3
"""Fetch a watch's field log (see src/field_log.rs) over Bluetooth.

Writes the raw defmt stream, one file per firmware version found in the log
(`field-log-<version>.defmt`), oldest first, and prints how to decode each:

    defmt-print -e ur-<version>.elf < field-log-<version>.defmt

The ELF must be the exact build that wrote the log; `scripts/package.py`
keeps one next to each package it makes.

Only sectors written since the last fetch from the same watch cross the
air: every sector fetched is kept in a cache (`--cache`; by default under
`$XDG_CACHE_HOME/ur/field-log/`, one directory per watch), and the files
written are assembled from it, so they hold the whole log as before. A
full log takes about three minutes to fetch; an hour's new sectors, a few
seconds. `--full` fetches every sector again.

Needs `bleak`, and the computer paired with the watch (see scripts/dfu.py).

Usage: fetch-log.py --address <MAC> [--out DIR] [--cache DIR] [--full]
"""

import argparse
import asyncio
import os
import struct
import sys
from pathlib import Path

from bleak import BleakClient, BleakScanner
from bleak.backends.device import BLEDevice

CONTROL = "75726c00-6f67-4c6f-8000-000000000002"
DATA = "75726c00-6f67-4c6f-8000-000000000003"
SECTORS = 512

# A cached sector: the header the watch sent (present, sequence, version,
# length) and its whole frames.
CACHED = struct.Struct("<BIII")


def default_cache(address: str) -> Path:
    base = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    return base / "ur" / "field-log" / address.upper().replace(":", "")


def load(cache: Path) -> dict[int, tuple[bool, int, int, bytes]]:
    """The cached sectors by index: (present, sequence, version, frames)."""
    sectors = {}
    for path in cache.glob("*.sector"):
        raw = path.read_bytes()
        present, sequence, version, _ = CACHED.unpack_from(raw)
        sectors[int(path.stem)] = (bool(present), sequence, version, raw[CACHED.size :])
    return sectors


def store(cache: Path, index: int, present: bool, sequence: int, version: int, data: bytes) -> None:
    # Written whole, then renamed: a fetch cut off mid-write (the link
    # dropping, say) leaves the previous copy rather than half a sector.
    path = cache / f"{index}.sector"
    partial = path.with_suffix(".partial")
    partial.write_bytes(CACHED.pack(present, sequence, version, len(data)) + data)
    partial.replace(path)


def version_name(word: int) -> str:
    major, minor, revision = word & 0xFF, (word >> 8) & 0xFF, word >> 16
    return f"{major}.{minor}.{revision}"


async def fetch(address: str, out: Path, cache: Path, full: bool) -> None:
    cache.mkdir(parents=True, exist_ok=True)
    cached = {} if full else load(cache)

    # The watch fills its ring in index order, each sector numbered one past
    # the one before (a boot opens a new sector, numbered on), so what's new
    # since the last fetch starts at the newest sector we have (which may
    # have grown since) and runs on from there, wrapping, until a sector
    # that isn't the next number: one older than the last fetch. If even
    # the newest we have is gone (the ring wrapped all the way, or the log
    # was erased), everything is fetched.
    present = {i: c[1] for i, c in cached.items() if c[0]}
    if present:
        start = max(present, key=lambda i: present[i])
        order = [(start + k) % SECTORS for k in range(SECTORS)]
    else:
        start, order = None, list(range(SECTORS))
    incremental = start is not None

    device = await BleakScanner.find_device_by_address(address, timeout=10.0)
    if device is None and sys.platform == "linux":
        # Already connected (e.g. to BlueZ): see scripts/dfu.py.
        path = f"/org/bluez/hci0/dev_{address.upper().replace(':', '_')}"
        device = BLEDevice(address, None, {"path": path, "props": {}})
    notifications: asyncio.Queue[bytes] = asyncio.Queue()
    fetched = 0
    previous = None
    async with BleakClient(device or address, timeout=30.0) as client:
        await client.start_notify(DATA, lambda _, data: notifications.put_nowait(bytes(data)))
        for index in order:
            await client.write_gatt_char(CONTROL, struct.pack("<H", index), response=True)
            header = await asyncio.wait_for(notifications.get(), 10.0)
            _, got, present_byte, sequence, version, length = struct.unpack("<BHBIII", header)
            assert got == index, f"asked for sector {index}, got {got}"
            data = bytearray(length)
            while True:
                message = await asyncio.wait_for(notifications.get(), 10.0)
                if message[0] == 0x02:
                    break
                (offset,) = struct.unpack_from("<H", message, 1)
                data[offset : offset + len(message) - 3] = message[3:]
            # A sector cut off by a reset has no length; keep whole frames
            # only (each ends in a zero byte).
            frames = bytes(data[: data.rfind(b"\0") + 1]) if present_byte else b""
            store(cache, index, bool(present_byte), sequence, version, frames)
            fetched += 1
            print(f"\rsector {index}, {fetched} fetched", end="", flush=True)
            if incremental:
                if index == start and (not present_byte or sequence != present[start]):
                    print(f"\nsector {index} isn't the one cached; fetching everything")
                    incremental = False
                elif index != start and (
                    not present_byte or sequence != (previous + 1) & 0xFFFFFFFF
                ):
                    break
            previous = sequence
    print()

    sectors = sorted(
        (sequence, version, frames)
        for present_flag, sequence, version, frames in load(cache).values()
        if present_flag
    )
    out.mkdir(parents=True, exist_ok=True)
    streams: dict[int, bytearray] = {}
    for _, version, data in sectors:
        streams.setdefault(version, bytearray()).extend(data)
    for version, stream in streams.items():
        name = version_name(version)
        path = out / f"field-log-{name}.defmt"
        path.write_bytes(stream)
        print(f"{path}: {len(stream)} bytes; decode with:")
        print(f"    defmt-print -e ur-{name}.elf < {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--address", required=True)
    parser.add_argument("--out", type=Path, default=Path("."))
    parser.add_argument("--cache", type=Path, help="where fetched sectors are kept")
    parser.add_argument("--full", action="store_true", help="fetch every sector again")
    args = parser.parse_args()
    cache = args.cache or default_cache(args.address)
    asyncio.run(fetch(args.address, args.out, cache, args.full))


if __name__ == "__main__":
    main()
