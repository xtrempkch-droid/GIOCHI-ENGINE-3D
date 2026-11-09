#!/usr/bin/env python3
"""Compares two directories of criterion estimates and fails on a regression.

Phase 18b. `cargo bench` prints a percentage, but criterion always exits 0: deciding
whether a change is a regression is left to the caller. This is that caller.

Both directories are criterion's output tree (`target/criterion` by default). Every
benchmark is identified by its path relative to the tree, so a benchmark that exists on
only one side is reported rather than silently ignored — a benchmark that disappeared is
a regression too, because it is coverage that was lost.

Usage:

    scripts/perf-compare.py --baseline /tmp/main-criterion --candidate target/criterion \\
        --threshold 10

Exit status is 1 if any benchmark's median is more than `--threshold` percent slower in
the candidate than in the baseline, or if the two trees do not have the same benchmarks.

Only the median is used: criterion's mean is dragged around by the occasional outlier
that a shared machine produces, and p99 is not what a benchmark of a fixed workload is
trying to report.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Criterion writes `<group>/<function>/<parameter>/new/estimates.json`.
NEW = "new/estimates.json"


def load_medians(root: Path) -> dict[str, float]:
    """Maps benchmark path -> median estimate, in nanoseconds."""
    estimates: dict[str, float] = {}
    for path in sorted(root.rglob(NEW)):
        relative = path.relative_to(root).parent.parent.as_posix()
        try:
            data = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError) as error:
            print(f"warning: {path}: {error}", file=sys.stderr)
            continue
        median = data.get("median", {}).get("point_estimate")
        if median is None:
            print(f"warning: {path}: no median", file=sys.stderr)
            continue
        estimates[relative] = float(median)
    return estimates


def format_duration(nanoseconds: float) -> str:
    if nanoseconds >= 1e9:
        return f"{nanoseconds / 1e9:.3f} s"
    if nanoseconds >= 1e6:
        return f"{nanoseconds / 1e6:.3f} ms"
    if nanoseconds >= 1e3:
        return f"{nanoseconds / 1e3:.3f} us"
    return f"{nanoseconds:.1f} ns"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument(
        "--threshold",
        type=float,
        default=10.0,
        help="percent slower than the baseline that counts as a regression (default 10)",
    )
    parser.add_argument(
        "--allow-missing",
        action="store_true",
        help="do not fail when a benchmark is present on only one side",
    )
    arguments = parser.parse_args()

    baseline = load_medians(arguments.baseline)
    candidate = load_medians(arguments.candidate)

    if not baseline:
        print(f"error: no estimates under {arguments.baseline}", file=sys.stderr)
        return 2
    if not candidate:
        print(f"error: no estimates under {arguments.candidate}", file=sys.stderr)
        return 2

    only_baseline = sorted(set(baseline) - set(candidate))
    only_candidate = sorted(set(candidate) - set(baseline))

    regressions: list[tuple[str, float, float, float]] = []
    improvements: list[tuple[str, float, float, float]] = []
    print(f"{'benchmark':<52} {'baseline':>10} {'candidate':>10} {'change':>9}")
    print("-" * 84)
    for name in sorted(set(baseline) & set(candidate)):
        before = baseline[name]
        after = candidate[name]
        # A baseline of zero would be a benchmark that measured nothing; report it rather
        # than dividing by it.
        if before <= 0.0:
            print(f"{name:<52} {'-':>10} {format_duration(after):>10} {'n/a':>9}")
            continue
        change = (after - before) / before * 100.0
        flag = " "
        if change > arguments.threshold:
            flag = "!"
            regressions.append((name, before, after, change))
        elif change < -arguments.threshold:
            flag = "+"
            improvements.append((name, before, after, change))
        print(
            f"{name:<52} {format_duration(before):>10} "
            f"{format_duration(after):>10} {change:>+8.1f}%{flag}"
        )

    for name in only_baseline:
        print(f"{name:<52} missing from the candidate", file=sys.stderr)
    for name in only_candidate:
        print(f"{name:<52} new in the candidate", file=sys.stderr)

    print(
        f"\nthreshold +/-{arguments.threshold:.1f}%: "
        f"{len(regressions)} regression(s), {len(improvements)} improvement(s)"
    )

    status = 0
    if regressions:
        print("\nregressions:", file=sys.stderr)
        for name, before, after, change in regressions:
            print(
                f"  {name}: {format_duration(before)} -> {format_duration(after)} "
                f"({change:+.1f}%)",
                file=sys.stderr,
            )
        status = 1
    if (only_baseline or only_candidate) and not arguments.allow_missing:
        print(
            "\nerror: the baseline and the candidate do not measure the same benchmarks",
            file=sys.stderr,
        )
        status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
