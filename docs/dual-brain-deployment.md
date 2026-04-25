# Dual-Brain Deployment Runbook

*Pi 5 cerebellum (local fast model, ~8 Hz) + cloud cortex (slow strong model, ~100 Hz inference but invoked sparingly). Mirrors the RoClaw Cortex/Cerebellum pattern (design §1.3). v0.01 wires the cloud path through `<|fault|>{"needs_cloud":true}` only; v0.1 adds proactive tier-based routing per refinement §2.5.*

---

## Topology

```
┌──────────────────────┐                     ┌────────────────────┐
│ Pi 5 (cerebellum)    │                     │ Cloud (cortex)     │
│                      │                     │                    │
│  llama-server        │   <|fault|> over    │  OpenAI-compatible │
│   ↑ (gemma-4-e2b)    │   HTTP from iod     │   /v1/chat/        │
│  iod ──────────────────────────────────────▶  completions       │
│   ↑                  │                     │                    │
│  bootloader          │                     │  GPT-5.4 / Opus    │
│                      │                     │  4.6 / Kimi K2.5   │
│  cart/io/roclaw  ────┼──── UDP :4210 ───── │                    │
└──────────┬───────────┘                     └────────────────────┘
           │
           ▼
       RoClaw robot
```

---

## Hardware

| Role | Spec | Notes |
|---|---|---|
| Pi 5 | 8GB minimum | 16GB strongly preferred for the v0.5 multi-task posture. Active cooler required (sustained inference loads thermal-throttle aggressively). |
| Storage | NVMe via PCIe HAT recommended | SD card works for v0.01 but model load times are 4× slower. |
| Network | Wired Ethernet preferred | Cloud cortex calls require a stable connection; WiFi works but adds 50–200 ms latency variance per fault. |
| Robot bus | UDP `127.0.0.1:4210` | Same address as RoClaw sim. For physical robot, set `LLM_OS_ROCLAW_ADDR=<robot-ip>:4210`. |

---

## Cerebellum (Pi 5) setup

```bash
# 0. Prereqs
sudo apt update && sudo apt install -y build-essential cmake git curl

# 1. Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Build llm_os via quickstart
cd ~/evolvingagents/llm_os
bash scripts/quickstart.sh

# 3. Download a kernel model
mkdir -p ~/models
# Recommendation: Gemma 4 E2B Q4 (~1.5 GB) — see tokenization-report.md
# for why <|think|> being a single Gemma token favors this over Qwen 3 2B.
curl -L -o ~/models/gemma-4-e2b-q4.gguf <YOUR_MIRROR_URL>
```

### Cerebellum env vars

| Var | Default | Purpose |
|---|---|---|
| `LLM_OS_MODEL` | (required) | Path to GGUF kernel. |
| `LLM_OS_LLAMA_SERVER` | `$(which llama-server)` | Server binary path. |
| `LLM_OS_PORT` | `8080` | llama-server listen port. |
| `LLM_OS_ROCLAW_ADDR` | `127.0.0.1:4210` | UDP target for the roclaw cartridge. |
| `RUST_LOG` | `info` | `debug` to trace each segment + opcode. |

### Cerebellum latency budget

- Per-syscall framing on Gemma 4 E2B: ~26 tokens (see `docs/tokenization-report.md`).
- Pi 5 8GB throughput: ~8 tok/s.
- Therefore: ~3.3 s per syscall. A 10-syscall task ≈ 33 s wall-clock.
- v0.01 acceptance test (`tests/e2e/navigate_and_report.sh`): 10 syscalls × 3.3s = 33s, well under the 600s budget.

---

## Cortex (cloud) setup

The daemon talks to any OpenAI-compatible `/v1/chat/completions` endpoint.

```bash
# Anthropic (Claude Opus 4.6) via their OpenAI-compat shim:
export LLM_OS_CLOUD_URL=https://api.anthropic.com/v1/openai
export LLM_OS_CLOUD_KEY=$ANTHROPIC_API_KEY
export LLM_OS_CLOUD_MODEL=claude-opus-4-6

# OpenAI:
export LLM_OS_CLOUD_URL=https://api.openai.com/v1
export LLM_OS_CLOUD_KEY=$OPENAI_API_KEY
export LLM_OS_CLOUD_MODEL=gpt-4o
# (or gpt-5-* once available)

# Self-hosted vLLM with auth disabled:
export LLM_OS_CLOUD_URL=http://your-gpu-box:8000/v1
unset LLM_OS_CLOUD_KEY
export LLM_OS_CLOUD_MODEL=Qwen/Qwen2.5-72B-Instruct
```

### When cloud fires

In v0.01, the cerebellum invokes the cortex *only* when the local model emits:

```
<|fault|>{"needs_cloud":true,"reason":"...","context":...}
```

The daemon compacts the local task context via `runtime/swap.rs`, POSTs to the cloud endpoint, and injects the response as `<|result|>{"cloud_response":"..."}<|/result|>`.

### Cortex latency budget

Cloud frontier models in 2026 emit ~60–200 tok/s. A 200-token strategic decision: 1–3 s wall-clock + network. Plan ≤ 5 s per fault. The cerebellum continues at 8 Hz between faults, so cortex is invoked tens of seconds apart in normal operation.

---

## Running end-to-end

```bash
# Terminal 1 (Pi 5): boot the server
~/evolvingagents/llm_os/runtime/bootloader \
    --model ~/models/gemma-4-e2b-q4.gguf \
    --port 8080

# Terminal 2 (Pi 5): run the daemon for a goal
cd ~/evolvingagents/llm_os
./runtime/target/release/iod \
    --server http://127.0.0.1:8080 \
    --grammar grammar/isa.gbnf \
    --cart    cart \
    --goal    "Drive the RoClaw forward 50 cm and stop."
```

Or the e2e harness (`bash tests/e2e/navigate_and_report.sh`) — that loops 10 times against `sim_world` and produces a pass/fail summary.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Bootloader prints `server did not bind within 30s` | Model load time exceeds boot timeout (cold SD card, cold CPU cache) | `--boot-timeout 120` |
| iod logs `parse error: malformed` after each segment | Grammar not actually applied — check `iod` logs for `grammar=N bytes` showing N>0 at startup | Verify `--grammar` path; re-run `bash scripts/validate_grammar.sh` |
| All RoClaw calls return `{"ok":false,"error":"send_to(...) failed"}` | UDP target unreachable | `LLM_OS_ROCLAW_ADDR=<robot-ip>:4210`; verify with `nc -u -l 4210` on the robot |
| Cloud fault returns `cloud fallback failed` | API key or URL wrong | `curl -H "Authorization: Bearer $LLM_OS_CLOUD_KEY" "$LLM_OS_CLOUD_URL/models"` to verify |
| Pi 5 crashes after ~10 minutes of inference | Thermal throttling escalated to lockup | Active cooler required; check `vcgencmd measure_temp` — should stay <70°C |
| Each segment takes 8+ seconds when it should take 3 | KV cache reuse broken (R1) | Daemon logs `cache_n` at debug level; should be ≥95% of prompt length on the second segment onward |

---

## Deferred to v0.5

- **Preemptive scheduler** that interrupts the cerebellum mid-stream when the cortex emits a higher-priority goal (currently the cerebellum runs to `<|halt|>` before cortex can intervene).
- **Multi-task isolation** — v0.01 runs one task at a time; v0.5 swaps KV caches per task.
- **Proactive tier routing** — v0.01 only escalates to cloud via explicit `<|fault|>`; v0.1 picks per-cartridge tier from the manifest.
