# residential_electrical — full spec

Ported from `skillos_mini/cartridges/residential-electrical/`. Bodies stubbed in v0.01.

## Methods

### `analyze_load`

Compute total connected load and per-room subtotals from a `LoadProfile` (voltage, frequency, list of rooms with appliances).

### `design_circuits`

Produce a circuit list compliant with IEC 60364 — breaker amperage from `{6, 10, 13, 16, 20, 25, 32, 40, 50, 63}`, wire from `{1.0, 1.5, 2.5, 4, 6, 10, 16}` mm². RCD assignment per circuit type.

## Post-validators

- `compliance_checker` — deterministic IEC 60364 subset check. Stubbed in v0.01.
