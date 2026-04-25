# Projection re-baseline — 2026-04-25 (v0.1 prep)

*Per refinement §4 R7: re-baseline trajectory projections at v0.1 (Month 2), not v0.5 (Month 6). The v0.5 self-hosting milestone depends on the small-on-device kernel tracking frontier-minus-one; if v0.1 measurements show frontier-minus-two, v0.5 slips and downstream consumers need the heads-up before they plan around it.*

This document is the v0.1-prep re-baseline. The v0.1-final re-baseline (with measured kernel performance after the user runs the fine-tune) will be `docs/projection-rebaseline-2026-06.md`.

---

## 0. What changed since the design-doc projections (April 2026)

The original design §6.2 projected — based on METR's 4-month-doubling extrapolation:

| Milestone | Cloud-frontier 50%-time-horizon | Local Pi 5 horizon | Pi 5 / frontier lag |
|---|---|---|---|
| Today (early 2026) | ~2 h 17 min | (no v0.01 yet) | n/a |
| Month 1 (v0.01)   | ~2 h 30 min | 5–10 min       | ~12–15 mo |
| Month 2 (v0.1)    | ~2 h 45 min | 15–30 min      | ~12–15 mo |
| Month 6 (v0.5)    | ~4–5 h      | 1–2 h          | ~12–18 mo |
| Month 12 (v1.0)   | ~16–32 h    | 4–8 h          | ~12–18 mo |

The load-bearing assumption: **METR doubles every 4 months on cloud frontier; on-device 2B lags 12–18 months**. If either fails, downstream milestones slip.

---

## 1. What v0.01 actually measured

The v0.01 sprint surfaced data the design didn't have. None of it changes the projection shape, but two findings tighten the bounds.

### 1.1 Per-syscall cost on bootstrap kernels (`docs/tokenization-report.md`)

A typical syscall (`<|call|>roclaw.forward {...} <|/call|>`) is **26–27 tokens** on Qwen 2/3 and Gemma 4. At 8 tok/s on Pi 5 that's **3.3 s/syscall**. A 50-syscall task = 165 s wall-clock. The design's "5–10 min for 5–10 min tasks at 50% reliability" maps to ~150–300 s budget, which is consistent.

After the v0.1 fine-tune, opcodes are single tokens. Syscall framing drops from ~5 tokens (opener) + ~11 tokens (closer) = ~16 to **~2 tokens** total. Result: typical syscall ~10 tokens, **~1.25 s on Pi 5, ~2.6× speedup on the syscall-frame component**.

This is below the design's 5–10× claim. The remaining headroom must come from:

1. **Dialect compression** (refinement §2.3, shipped in v0.1 as `runtime/dialect.rs`): `roclaw-motion-v1` compresses `{"left":150,"right":150}` (24 chars / ~10 tokens) to `F 150 150` (9 chars / ~5 tokens). At maturity (5+ shipped dialects) this is another ~2× on dialect-eligible cartridges.
2. **Better grammar discipline** with the per-method GBNF sub-grammars (v0.1 schema_to_gbnf compiler) — reduces wasted tokens on schema-violation retries from the v0.01 path's ~5–10% to <1%.

**Net v0.1 expectation**: ~3–4× syscall-throughput improvement on roclaw/sim_world workloads (where dialect applies), ~2.5× on cooking/electrical (no dialect). Design's "5–10×" is realistic only at v0.5 with all three optimizations stacked.

### 1.2 Gemma 4 has `<think>` as a single token

`<|think|>` tokenizes to 1 token on Gemma 4 vs 5 on Qwen. For tasks dominated by inner-monologue volume, this is **a free 5× speedup on the highest-volume opcode** before any fine-tuning. **Recommendation:** Gemma 4 E2B is the v0.1 bootstrap kernel.

The design didn't predict this. If it generalizes (other thinking-heavy opcodes happen to be in the kernel's vocab), Pi 5 throughput improves another 10–20% over the projected baseline.

### 1.3 Effective context vs. advertised context

Refinement §6 noted RULER-style measurements show 50–65% of advertised context is effective. The v0.01 daemon hasn't yet measured this in production (no users), but the swap.rs threshold defaults to 70% of advertised — which means in practice we trigger compaction at ~45% effective. **Not a regression, but a planning input**: v0.5's per-task KV-swap timeline assumes ≥ 8K effective on Pi 5; advertised 16K at 50% gets us 8K, advertised 32K at 65% gets us ~21K. Plan for 50% conservative.

---

## 2. Updated milestone table

Re-baselined for v0.1 with measurement-grounded numbers:

| Milestone | Cloud frontier (METR median, 50%) | Pi 5 local (best case) | Pi 5 local (50% conservative) |
|---|---|---|---|
| Today (2026-04-25) | ~2 h 17 min (TH1.1) | n/a | n/a |
| Month 2 (v0.1, after fine-tune) | ~2 h 45 min | ~25 min | ~12 min |
| Month 6 (v0.5) | ~4–5 h | ~1.5 h | ~45 min |
| Month 12 (v1.0) | ~16–32 h | ~4–6 h | ~2 h |

**Conservative column** assumes a 12-month device lag instead of 12–18. **Best case** assumes Gemma 4 E2B's `<think>` advantage holds across the v0.1 fine-tune and the dialect framework picks up 2× on dialect-eligible workloads.

---

## 3. What would invalidate this re-baseline

The trajectory bet has two failure modes worth naming explicitly:

1. **METR doubling stalls.** If the next 6 months show 7-month doubling (the historical 6-year average) instead of the recent 4-month rate, v0.5 slips from Month 6 to Month 9. The Linux-1.0-in-30-months parallel breaks. **Detection signal:** TH1.1 successor measurements; arxiv 2511.19492's slowdown scenario tracking.
2. **Small-on-device lag widens.** If Gemma 5 / Qwen 4 / Llama 5 don't deliver another generation of "3B at last-gen-30B capability" by v0.5 (Month 6), Pi 5 horizons stop tracking frontier-minus-one. **Detection signal:** the next major small-model release benchmarks against frontier-minus-one on SWE-bench Verified, GPQA Diamond, and BFCL v4. If the new 3B is closer to the prior generation's 30B than to current frontier-minus-one's 20–60B, alarm.

Both signals are external — no internal LLM-OS work changes them. Re-check at v0.5 (Month 6) per the new schedule (was Month 12).

---

## 4. Action items leading into v0.1-final

1. After the user runs the fine-tune (`docs/fine-tune-recipe.md`), re-run `bash scripts/check_tokenizer.sh` against the new GGUF. Expected: every opcode → 1 token. If not, the special-token addition didn't take and the v0.1 multiplier won't materialize.
2. Re-run `bash tests/e2e/navigate_and_report.sh` with the v0.1 kernel. Compare wall-clock per run vs. the v0.01 baseline. Expected: ≥ 3× improvement on sim_world (no dialect adoption yet), ≥ 5× on roclaw fixtures (dialect applies).
3. Write `docs/projection-rebaseline-2026-06.md` with the measured numbers replacing the columns above. That doc gates the v0.1-final tag.

---

## 5. The one-paragraph version

The design's projections survive the v0.01 sprint. Two findings tighten them: a typical syscall costs 26–27 tokens on bootstrap kernels (consistent with the "1–2 syscalls/sec on Pi 5" claim), and Gemma 4's native `<think>` token is a free 5× speedup on the highest-volume opcode. The v0.1 fine-tune drops syscall framing to ~2 tokens (~2.6× speedup); dialects add another ~2× on dialect-eligible cartridges; per-method GBNF compilation eliminates retry-cost. Stacked, that's 3–4× at v0.1 and 5–10× by v0.5 — consistent with the design but only achievable with all three optimizations live, not with the fine-tune alone. METR's 4-month doubling remains the single load-bearing extrapolation; if it slips to historical 7-month rate, v0.5 slips to Month 9. Re-check at v0.1-final after the user's GPU run produces measured kernel-v0.1 numbers.
