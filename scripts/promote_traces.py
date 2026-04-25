#!/usr/bin/env python3
"""
v0.5-D — Self-hosting trace promotion pipeline.

The Linux-0.11 moment: the OS curates its own training data. Reads the
JSONL traces written by `iod --trace`, filters successful trajectories,
groups by goal, and emits DPO-ready triples consumable by
`docs/fine-tune-recipe.md` §3.

Usage:
    python3 scripts/promote_traces.py \\
        --traces traces/*.jsonl \\
        --out    out/dpo.jsonl \\
        [--min-success 3]                 # require N+ success runs per goal
        [--require-cartridges roclaw,sim_world]  # filter by cartridges used
        [--dry-run]                       # report counts only

Input shape (one per line in --traces):
    {"goal": "...", "stream": "...", "status": "success"|"partial"|"failure",
     "steps": int, "wall_seconds": float, "cartridges": ["..."],
     "prompt": "...", "ts": int}

Output shape (one per line in --out — DPO format):
    {"prompt": "Goal: ...\\n",
     "chosen":   "<successful stream>",
     "rejected": "<failure stream>",
     "metadata": {"goal": "...", "chosen_steps": int, ...}}

Idempotent: running twice produces the same output bit-for-bit (sorted +
deduped by content hash).
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import itertools
import json
import sys
from collections import defaultdict
from pathlib import Path


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--traces", nargs="+", required=True, help="trace JSONL files (globs ok)")
    p.add_argument("--out", required=True, help="output DPO JSONL path")
    p.add_argument("--min-success", type=int, default=2,
                   help="require at least N successful runs per goal before emitting any pair")
    p.add_argument("--require-cartridges", default="",
                   help="comma-separated list; trace must use at least one to be considered")
    p.add_argument("--max-pairs-per-goal", type=int, default=10,
                   help="cap pairs per goal to keep dataset balanced")
    p.add_argument("--dry-run", action="store_true",
                   help="report counts; write nothing")
    return p.parse_args()


def load_traces(patterns: list[str]) -> list[dict]:
    paths: list[Path] = []
    for pat in patterns:
        for match in glob.glob(pat):
            paths.append(Path(match))
    if not paths:
        print(f"error: no trace files matched {patterns}", file=sys.stderr)
        sys.exit(1)
    out: list[dict] = []
    for p in paths:
        with p.open("r", encoding="utf-8") as f:
            for lineno, raw in enumerate(f, start=1):
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    out.append(json.loads(raw))
                except json.JSONDecodeError as e:
                    print(f"warn: {p}:{lineno}: bad JSON ({e}); skipping", file=sys.stderr)
    return out


def passes_cartridge_filter(trace: dict, required: set[str]) -> bool:
    if not required:
        return True
    used = set(trace.get("cartridges") or [])
    return bool(used & required)


def short_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def main() -> int:
    args = parse_args()
    required_carts = {c.strip() for c in args.require_cartridges.split(",") if c.strip()}

    traces = load_traces(args.traces)
    print(f"[promote_traces] loaded {len(traces)} traces", file=sys.stderr)

    # Filter by cartridge requirement.
    traces = [t for t in traces if passes_cartridge_filter(t, required_carts)]
    print(f"[promote_traces] after cartridge filter: {len(traces)}", file=sys.stderr)

    # Group by goal.
    by_goal: dict[str, list[dict]] = defaultdict(list)
    for t in traces:
        goal = t.get("goal", "").strip()
        if goal:
            by_goal[goal].append(t)

    eligible_goals = []
    for goal, ts in by_goal.items():
        ok = [t for t in ts if t.get("status") == "success"]
        bad = [t for t in ts if t.get("status") in ("partial", "failure")]
        if len(ok) >= args.min_success and bad:
            eligible_goals.append((goal, ok, bad))
    print(
        f"[promote_traces] {len(eligible_goals)} goals have ≥{args.min_success} success + ≥1 failure",
        file=sys.stderr,
    )

    # Build DPO pairs: cross-product per goal, capped + deduped.
    pairs: list[dict] = []
    seen_hashes: set[str] = set()
    for goal, ok, bad in eligible_goals:
        # Sort for determinism.
        ok_sorted = sorted(ok, key=lambda t: t.get("ts", 0))
        bad_sorted = sorted(bad, key=lambda t: t.get("ts", 0))
        per_goal_pairs = []
        for c, r in itertools.product(ok_sorted, bad_sorted):
            chosen = c.get("stream", "")
            rejected = r.get("stream", "")
            if not chosen or not rejected:
                continue
            h = short_hash(f"{goal}\n{chosen}\n{rejected}")
            if h in seen_hashes:
                continue
            seen_hashes.add(h)
            per_goal_pairs.append({
                "prompt":   f"Goal: {goal}\n",
                "chosen":   chosen,
                "rejected": rejected,
                "metadata": {
                    "goal":           goal,
                    "chosen_steps":   c.get("steps"),
                    "rejected_steps": r.get("steps"),
                    "chosen_wall":    c.get("wall_seconds"),
                    "rejected_wall":  r.get("wall_seconds"),
                    "chosen_status":  c.get("status"),
                    "rejected_status": r.get("status"),
                    "hash":           h,
                },
            })
            if len(per_goal_pairs) >= args.max_pairs_per_goal:
                break
        pairs.extend(per_goal_pairs)

    print(f"[promote_traces] emitted {len(pairs)} DPO pairs across {len(eligible_goals)} goals",
          file=sys.stderr)

    if args.dry_run:
        print("[promote_traces] dry-run; not writing", file=sys.stderr)
        return 0

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    # Sort by hash for deterministic output ordering.
    pairs.sort(key=lambda p: p["metadata"]["hash"])
    with out_path.open("w", encoding="utf-8") as f:
        for p in pairs:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")
    print(f"[promote_traces] wrote {out_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
