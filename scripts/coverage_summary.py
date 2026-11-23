#!/usr/bin/env python3
"""Summarise scripts/coverage.sh's merged coverage as Markdown.

    coverage_summary.py ROOT COVERAGE.json

COVERAGE.json is `llvm-cov export -format=text` of every crate's tests
together. Prints, for src/ and sim/src/ only:

- the totals, of lines, branches and functions;
- each file's coverage, least covered (by lines) first;
- the functions no test reaches, file by file;
- the files of src/ with no coverage at all: built by no test (the
  hardware's), or with nothing to run in them.

Only the standard library is needed.
"""

import json
import re
import subprocess
import sys
from pathlib import Path


def percent(covered, count):
    return f"{100 * covered / count:.1f}%" if count else "-"


def main():
    root = Path(sys.argv[1]).resolve()
    with open(sys.argv[2]) as f:
        export = json.load(f)["data"][0]

    def ours(filename):
        """The path from the root, if under src/ or sim/src/."""
        path = Path(filename)
        if not path.is_relative_to(root):
            return None
        rel = path.relative_to(root).as_posix()
        return rel if rel.startswith(("src/", "sim/src/")) else None

    files = {}
    for entry in export["files"]:
        rel = ours(entry["filename"])
        if rel is not None:
            files[rel] = entry["summary"]

    # ==========================================================================
    # Functions, merged across the crates
    # ==========================================================================

    # Each crate that includes a module compiles its functions anew, under
    # its own crate's name (`alarm_test::alarm::…`, `ur_sim::alarm::…`),
    # and a generic function once per instantiation. So a function is
    # where its body starts in the source, and is reached if any of those
    # ran it. Its name is read from the source there, rather than
    # demangled, which would need a demangler llvm-tools doesn't ship.
    reached = {}
    for function in export["functions"]:
        rel = ours(function["filenames"][0])
        if rel is None:
            continue
        line, column = function["regions"][0][:2]
        key = (rel, line, column)
        reached[key] = reached.get(key, False) or function["count"] > 0

    sources = {}

    def name_at(rel, line):
        """`Type::name` or `name` for the `fn` at `line`, or None for a
        closure or an async block (no `fn` there)."""
        if rel not in sources:
            sources[rel] = (root / rel).read_text().splitlines()
        lines = sources[rel]
        text = lines[line - 1]
        found = re.search(r"\bfn\s+(\w+)", text)
        if not found:
            return None
        name = found.group(1)
        # A method: the nearest `impl` above, indented less.
        indent = len(text) - len(text.lstrip())
        for above in reversed(lines[: line - 1]):
            stripped = above.lstrip()
            if not stripped or len(above) - len(stripped) >= indent:
                continue
            impl = re.match(r"impl\b(?:\s*<[^{]*?>)?\s+(?:[^{]*?\bfor\s+)?(\w+)", stripped)
            if impl:
                return f"{impl.group(1)}::{name}"
            if not stripped.startswith(("#", "//", "}")):
                break
        return name

    unreached = {}
    closures = 0
    for (rel, line, _), was_reached in sorted(reached.items()):
        if was_reached:
            continue
        name = name_at(rel, line)
        if name is None:
            closures += 1
            continue
        unreached.setdefault(rel, []).append((line, name))

    # ==========================================================================
    # The report
    # ==========================================================================

    commit = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--short", "HEAD"],
        capture_output=True,
        text=True,
    ).stdout.strip()
    totals = export["totals"]
    lines, branches = totals["lines"], totals["branches"]
    functions_reached = sum(reached.values())

    print(f"# Coverage{f' at `{commit}`' if commit else ''}")
    print()
    print(
        "The firmware's modules of src/ through the host-test crates and the "
        "simulator, and the simulator's own code (sim/src/) through its tests, "
        "merged (`scripts/coverage.sh`)."
    )
    print()
    print("| | Covered | Of | |")
    print("|---|---:|---:|---:|")
    print(f"| Lines | {lines['covered']} | {lines['count']} | {percent(lines['covered'], lines['count'])} |")
    print(
        f"| Branches | {branches['covered']} | {branches['count']} | "
        f"{percent(branches['covered'], branches['count'])} |"
    )
    print(
        f"| Functions | {functions_reached} | {len(reached)} | "
        f"{percent(functions_reached, len(reached))} |"
    )
    print()

    print("## By file, least covered first")
    print()
    print("| File | Lines | Branches | Functions not reached |")
    print("|---|---:|---:|---:|")
    by_lines = sorted(
        files.items(),
        key=lambda item: (
            item[1]["lines"]["covered"] / max(item[1]["lines"]["count"], 1),
            item[0],
        ),
    )
    for rel, summary in by_lines:
        lines, branches = summary["lines"], summary["branches"]
        print(
            f"| `{rel}` | {percent(lines['covered'], lines['count'])} "
            f"({lines['count'] - lines['covered']} of {lines['count']} not run) "
            f"| {percent(branches['covered'], branches['count'])} "
            f"| {len(unreached.get(rel, []))} |"
        )
    print()

    print("## Functions no test reaches")
    print()
    print(
        f"By module, with the line each starts on. Not listed: the {closures} "
        "closures and async blocks no test reaches."
    )
    for rel, _ in by_lines:
        if rel not in unreached:
            continue
        print()
        print(f"### `{rel}`")
        print()
        for line, name in unreached[rel]:
            print(f"- `{name}` ({line})")
    print()

    print("## Not covered at all")
    print()
    print(
        "Files of src/ that no test builds (the hardware's, and the "
        "firmware's tasks around it), or that have no code to run "
        "(constants and types):"
    )
    print()
    absent = sorted(
        rel
        for rel in (path.relative_to(root).as_posix() for path in (root / "src").rglob("*.rs"))
        if rel not in files
    )
    print(", ".join(f"`{rel}`" for rel in absent) or "none")


if __name__ == "__main__":
    main()
