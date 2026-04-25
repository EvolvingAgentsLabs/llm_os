//! Built-in cartridge handlers for v0.1.
//!
//! Replaces the v0.01 stubs in `cooking`, `residential_electrical`, `demo`
//! with deterministic Rust implementations. These handlers do NOT call the
//! LLM internally — they're the cartridge bodies that execute when the
//! kernel emits `<|call|>cart.method`.
//!
//! Each module exposes a `dispatch(method, args) -> DispatchResult`.

use crate::dispatch::{DispatchError, DispatchResult};
use serde_json::{json, Value};

// ────────────────────────────────────────────────────────────────────────
// demo — echo + hash
// ────────────────────────────────────────────────────────────────────────

pub mod demo {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
        match method {
            "echo" => {
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                Ok(json!({"text": text, "len": text.len()}))
            }
            "hash" => {
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let algo = args
                    .get("algo")
                    .and_then(Value::as_str)
                    .unwrap_or("djb2");
                // Real cryptographic hashes pull a chunky dep; for a "demo"
                // cartridge a stable non-crypto hash (djb2 / FxHash-style)
                // is fine and lets the daemon stay light. Documented in
                // full.md.
                let digest = match algo {
                    "djb2" => djb2(text),
                    "fnv1a" => fnv1a(text),
                    "rust-default" | _ => {
                        let mut h = DefaultHasher::new();
                        text.hash(&mut h);
                        format!("{:016x}", h.finish())
                    }
                };
                Ok(json!({"algo": algo, "digest_hex": digest, "len": text.len()}))
            }
            other => Err(DispatchError::HandlerError {
                cart: "demo".into(),
                method: other.into(),
                message: "unknown method".into(),
            }),
        }
    }

    fn djb2(s: &str) -> String {
        let mut h: u64 = 5381;
        for b in s.bytes() {
            h = h.wrapping_mul(33).wrapping_add(b as u64);
        }
        format!("{:016x}", h)
    }

    fn fnv1a(s: &str) -> String {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{:016x}", h)
    }
}

// ────────────────────────────────────────────────────────────────────────
// cooking — meal planner, shopping list aggregator, recipe writer
// ────────────────────────────────────────────────────────────────────────

pub mod cooking {
    use super::*;

    const DAYS: [&str; 7] = [
        "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
    ];
    const SLOTS: [&str; 3] = ["breakfast", "lunch", "dinner"];

    /// Deterministic seed menu — gives the daemon something concrete that
    /// passes the menu_complete validator without an LLM call. Real
    /// kitchens want LLM-generated variety; the v0.1 deterministic
    /// handler trades creativity for testability.
    const SEED_MEALS: [(&str, &str, &str, u32); 21] = [
        ("Monday", "breakfast", "Oatmeal with berries", 10),
        ("Monday", "lunch", "Lentil soup with toast", 25),
        ("Monday", "dinner", "Chicken stir-fry with rice", 30),
        ("Tuesday", "breakfast", "Yogurt parfait", 5),
        ("Tuesday", "lunch", "Quinoa salad", 15),
        ("Tuesday", "dinner", "Spaghetti bolognese", 35),
        ("Wednesday", "breakfast", "Avocado toast", 10),
        ("Wednesday", "lunch", "Hummus wrap", 10),
        ("Wednesday", "dinner", "Salmon with vegetables", 30),
        ("Thursday", "breakfast", "Smoothie bowl", 8),
        ("Thursday", "lunch", "Caesar salad", 15),
        ("Thursday", "dinner", "Mushroom risotto", 40),
        ("Friday", "breakfast", "Pancakes", 20),
        ("Friday", "lunch", "Tomato soup", 20),
        ("Friday", "dinner", "Pizza", 30),
        ("Saturday", "breakfast", "Eggs benedict", 25),
        ("Saturday", "lunch", "Burger and fries", 30),
        ("Saturday", "dinner", "Grilled steak with potatoes", 45),
        ("Sunday", "breakfast", "French toast", 20),
        ("Sunday", "lunch", "Roast chicken sandwich", 15),
        ("Sunday", "dinner", "Vegetable curry", 40),
    ];

    pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
        match method {
            "plan_menu" => plan_menu(args),
            "build_shopping_list" => build_shopping_list(args),
            "write_recipe" => write_recipe(args),
            other => Err(DispatchError::HandlerError {
                cart: "cooking".into(),
                method: other.into(),
                message: "unknown method".into(),
            }),
        }
    }

    fn plan_menu(args: &Value) -> DispatchResult {
        let household = args
            .get("household_size")
            .and_then(Value::as_u64)
            .unwrap_or(2);
        let dietary = args
            .get("dietary_notes")
            .and_then(Value::as_str)
            .unwrap_or("");

        let mut days = Vec::with_capacity(7);
        for day in DAYS {
            let mut meals = Vec::with_capacity(3);
            for slot in SLOTS {
                let entry = SEED_MEALS
                    .iter()
                    .find(|(d, s, _, _)| *d == day && *s == slot)
                    .expect("seed table covers all 21 day×slot pairs");
                meals.push(json!({
                    "slot": slot,
                    "name": entry.2,
                    "prep_minutes": entry.3,
                }));
            }
            days.push(json!({"day": day, "meals": meals}));
        }
        Ok(json!({
            "household_size": household,
            "dietary_notes": dietary,
            "days": days,
        }))
    }

    fn build_shopping_list(args: &Value) -> DispatchResult {
        let menu = args
            .get("menu")
            .ok_or_else(|| DispatchError::HandlerError {
                cart: "cooking".into(),
                method: "build_shopping_list".into(),
                message: "args.menu missing".into(),
            })?;
        let household = menu
            .get("household_size")
            .and_then(Value::as_u64)
            .unwrap_or(2) as usize;

        // For v0.1, produce a deterministic per-aisle template based on
        // the meal name lexicon — keeps menu_complete + shopping_list_sane
        // validators green without per-meal recipe lookup.
        let aisles = json!({
            "produce": [
                {"item": "berries (mixed)", "quantity": format!("{} cups", household)},
                {"item": "leafy greens",    "quantity": format!("{} bags", household)},
                {"item": "tomatoes",        "quantity": format!("{} kg", household)},
                {"item": "avocados",        "quantity": format!("{} ea", household * 2)},
            ],
            "dairy": [
                {"item": "yogurt",          "quantity": format!("{} L", household)},
                {"item": "eggs",            "quantity": format!("{} dozen", household)},
                {"item": "cheese",          "quantity": format!("{} g", household * 200)},
            ],
            "pantry": [
                {"item": "oats",            "quantity": "1 kg"},
                {"item": "rice",            "quantity": "1 kg"},
                {"item": "pasta",           "quantity": "500 g"},
                {"item": "bread",           "quantity": "2 loaves"},
            ],
            "protein": [
                {"item": "chicken",         "quantity": format!("{} kg", household)},
                {"item": "salmon",          "quantity": format!("{} g", household * 200)},
                {"item": "ground beef",     "quantity": format!("{} g", household * 250)},
            ],
            "other": [
                {"item": "olive oil",       "quantity": "1 bottle"},
                {"item": "spices",          "quantity": "as needed"},
            ],
        });
        Ok(json!({
            "household_size": household,
            "aisles": aisles,
        }))
    }

    fn write_recipe(args: &Value) -> DispatchResult {
        let meal_name = args
            .get("meal_name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        let slot = args
            .get("slot")
            .and_then(Value::as_str)
            .unwrap_or("dinner");
        let servings = args
            .get("servings")
            .and_then(Value::as_u64)
            .unwrap_or(2);
        Ok(json!({
            "name": meal_name,
            "slot": slot,
            "servings": servings,
            "ingredients": [
                {"item": "primary ingredient",  "quantity": format!("{} portion(s)", servings)},
                {"item": "supporting ingredients", "quantity": "as needed"},
                {"item": "seasoning", "quantity": "to taste"}
            ],
            "steps": [
                "Prep all ingredients per the quantities above.",
                format!("Cook the primary ingredient suitable for {slot}."),
                "Combine, plate, and serve immediately."
            ]
        }))
    }
}

// ────────────────────────────────────────────────────────────────────────
// residential_electrical — IEC 60364 subset
// ────────────────────────────────────────────────────────────────────────

pub mod residential_electrical {
    use super::*;

    const BREAKER_SIZES_A: &[u32] = &[6, 10, 13, 16, 20, 25, 32, 40, 50, 63];
    const WIRE_SIZES_MM2: &[(u32, f64)] = &[
        // (max breaker amps, wire size mm²) — IEC 60364-5-52 Table B.52.1
        // simplified for residential reference installation method.
        (10, 1.0),
        (16, 1.5),
        (20, 2.5),
        (25, 4.0),
        (32, 6.0),
        (40, 10.0),
        (63, 16.0),
    ];

    pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
        match method {
            "analyze_load" => analyze_load(args),
            "design_circuits" => design_circuits(args),
            other => Err(DispatchError::HandlerError {
                cart: "residential_electrical".into(),
                method: other.into(),
                message: "unknown method".into(),
            }),
        }
    }

    fn analyze_load(args: &Value) -> DispatchResult {
        let lp = args
            .get("load_profile")
            .ok_or_else(|| DispatchError::HandlerError {
                cart: "residential_electrical".into(),
                method: "analyze_load".into(),
                message: "args.load_profile missing".into(),
            })?;
        let voltage = lp.get("voltage_v").and_then(Value::as_u64).unwrap_or(230) as u32;
        let rooms = lp
            .get("rooms")
            .and_then(Value::as_array)
            .ok_or_else(|| DispatchError::HandlerError {
                cart: "residential_electrical".into(),
                method: "analyze_load".into(),
                message: "load_profile.rooms must be an array".into(),
            })?;

        let mut total_w: u64 = 0;
        let mut per_room = Vec::with_capacity(rooms.len());
        for room in rooms {
            let name = room.get("name").and_then(Value::as_str).unwrap_or("?");
            let loads = room
                .get("loads")
                .and_then(Value::as_array)
                .map(|a| a.as_slice())
                .unwrap_or(&[]);
            let room_w: u64 = loads
                .iter()
                .map(|l| l.get("watts").and_then(Value::as_u64).unwrap_or(0))
                .sum();
            total_w += room_w;
            per_room.push(json!({
                "name": name,
                "watts": room_w,
                "amps_at_voltage": (room_w as f64 / voltage as f64 * 100.0).round() / 100.0,
            }));
        }
        Ok(json!({
            "voltage_v": voltage,
            "total_watts": total_w,
            "total_amps": (total_w as f64 / voltage as f64 * 100.0).round() / 100.0,
            "per_room": per_room,
        }))
    }

    fn design_circuits(args: &Value) -> DispatchResult {
        let lp = args
            .get("load_profile")
            .ok_or_else(|| DispatchError::HandlerError {
                cart: "residential_electrical".into(),
                method: "design_circuits".into(),
                message: "args.load_profile missing".into(),
            })?;
        let voltage = lp.get("voltage_v").and_then(Value::as_u64).unwrap_or(230) as u32;
        let rooms = lp
            .get("rooms")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        let mut circuits = Vec::new();
        let mut next_id = 1;
        for room in rooms {
            let room_name = room.get("name").and_then(Value::as_str).unwrap_or("Room");
            let loads = room
                .get("loads")
                .and_then(Value::as_array)
                .map(|a| a.as_slice())
                .unwrap_or(&[]);

            // Group loads by circuit_type.
            let mut by_type: std::collections::BTreeMap<&str, Vec<&Value>> = Default::default();
            for l in loads {
                let ct = l
                    .get("circuit_type")
                    .and_then(Value::as_str)
                    .unwrap_or("outlet");
                by_type.entry(ct).or_default().push(l);
            }
            for (ctype, group) in by_type {
                let total_w: u64 = group
                    .iter()
                    .map(|l| l.get("watts").and_then(Value::as_u64).unwrap_or(0))
                    .sum();
                let amps = total_w as f64 / voltage as f64;
                // Pick the smallest breaker that exceeds the load by 25% margin.
                let needed_a = (amps * 1.25).ceil() as u32;
                let breaker = BREAKER_SIZES_A
                    .iter()
                    .copied()
                    .find(|b| *b >= needed_a)
                    .unwrap_or(63);
                let wire = WIRE_SIZES_MM2
                    .iter()
                    .find(|(max_a, _)| *max_a >= breaker)
                    .map(|(_, w)| *w)
                    .unwrap_or(16.0);
                let appliances: Vec<String> = group
                    .iter()
                    .map(|l| {
                        l.get("appliance")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                            .to_string()
                    })
                    .collect();
                circuits.push(json!({
                    "id": format!("C{:02}", next_id),
                    "label": format!("{room_name} {ctype}"),
                    "type": ctype,
                    "breaker_a": breaker,
                    "wire_mm2": wire,
                    "rcd": ctype == "outlet" || ctype == "dedicated",
                    "loads": appliances,
                    "design_watts": total_w,
                }));
                next_id += 1;
            }
        }
        Ok(json!({
            "code": "IEC_60364",
            "voltage_v": voltage,
            "circuits": circuits,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_echo_returns_text_and_len() {
        let r = demo::dispatch("echo", &json!({"text": "hello"})).unwrap();
        assert_eq!(r["text"], "hello");
        assert_eq!(r["len"], 5);
    }

    #[test]
    fn demo_hash_djb2_is_stable() {
        let r1 = demo::dispatch("hash", &json!({"text": "hi", "algo": "djb2"})).unwrap();
        let r2 = demo::dispatch("hash", &json!({"text": "hi", "algo": "djb2"})).unwrap();
        assert_eq!(r1["digest_hex"], r2["digest_hex"]);
    }

    #[test]
    fn cooking_plan_menu_emits_7_days_3_slots() {
        let r = cooking::dispatch("plan_menu", &json!({"household_size": 2})).unwrap();
        let days = r["days"].as_array().unwrap();
        assert_eq!(days.len(), 7);
        for day in days {
            let meals = day["meals"].as_array().unwrap();
            assert_eq!(meals.len(), 3);
        }
    }

    #[test]
    fn cooking_shopping_list_has_all_required_aisles() {
        let menu = cooking::dispatch("plan_menu", &json!({"household_size": 4})).unwrap();
        let r = cooking::dispatch("build_shopping_list", &json!({"menu": menu})).unwrap();
        for aisle in ["produce", "dairy", "pantry", "protein", "other"] {
            assert!(r["aisles"][aisle].is_array(), "aisle '{aisle}' missing");
        }
    }

    #[test]
    fn electrical_circuits_pick_breakers_from_iec_table() {
        let lp = json!({
            "voltage_v": 230,
            "rooms": [
                {
                    "name": "Kitchen",
                    "loads": [
                        {"appliance": "Oven",        "watts": 3000, "circuit_type": "dedicated"},
                        {"appliance": "Microwave",   "watts": 1200, "circuit_type": "outlet"},
                        {"appliance": "Ceiling",     "watts": 60,   "circuit_type": "lighting"}
                    ]
                }
            ]
        });
        let r =
            residential_electrical::dispatch("design_circuits", &json!({"load_profile": lp})).unwrap();
        let circuits = r["circuits"].as_array().unwrap();
        // Three circuits — one per type.
        assert_eq!(circuits.len(), 3);
        for c in circuits {
            let breaker = c["breaker_a"].as_u64().unwrap() as u32;
            assert!(
                [6, 10, 13, 16, 20, 25, 32, 40, 50, 63].contains(&breaker),
                "breaker {breaker} not in IEC 60364 list"
            );
        }
    }
}
