# cooking — full spec

Ported from `skillos_mini/cartridges/cooking/`. Bodies stubbed in v0.01.

## Methods

### `plan_menu`

Produce a 7-day menu given household size and optional dietary notes. Result is a `WeeklyMenu` shape (see source schema in skillos_mini for v0.1 reference).

### `build_shopping_list`

Aggregate a weekly menu into an aisle-grouped shopping list with required aisles `produce`, `dairy`, `pantry`, `protein`, `other`.

### `write_recipe`

Author a recipe for a single meal slot. Returns `{name, ingredients, steps}`.

## Post-validators

- `menu_complete` — checks `weekly_menu` has 7 days × 3 slots and that any `recipes` entries reference valid days.
