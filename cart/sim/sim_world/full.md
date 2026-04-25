# sim_world — full spec

Deterministic 4×1 grid world used by the L1 e2e harness. State: `(x, y)` integers; starts at `(0, 0)`; goal at `(3, 0)`.

## Methods

- `step({dir: "north"|"east"|"south"|"west"})` — move one cell.
  Returns `{ok, x, y, at_goal}`.
- `observe()` — return current position + goal coordinates.
- `reset()` — return to `(0, 0)`.

## Used by

`tests/e2e/navigate_and_report.sh` — runs the daemon against a goal like *"reach (3,0)"*, expects 10 consecutive runs that emit `<|halt|>status=success` and pass the `at_goal == true` post-check.
