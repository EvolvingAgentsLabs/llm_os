# LLM-OS Deployment Modes

Two modes, same architecture. The binaries are identical — only the host
environment differs.

## Mode 1: Development (regular Linux / macOS)

```
┌─────────────────────────────────────────────────────┐
│  Host OS (Linux/macOS)                              │
│                                                     │
│  ┌──────────┐  fork   ┌──────────────┐              │
│  │bootloader├────────►│ llama-server │:8080          │
│  └──────────┘         └──────┬───────┘              │
│                              │ HTTP (loopback)      │
│  ┌──────────────────────────▼───────────────────┐  │
│  │ iod (Rust daemon)                             │  │
│  │  ├── parser.rs    (ISA token stream)          │  │
│  │  ├── dispatch.rs  (cartridge syscalls)        │  │
│  │  ├── scheduler.rs (budget enforcement)        │  │
│  │  └── cloud.rs     (optional fallback)  ──────────► Cloud API
│  └───────────────────────────────────────────────┘  │
│                                                     │
│  Model: ~/models/qwen2.5-0.5b-q4.gguf (dev)        │
│  Grammar: grammar/isa.gbnf (from repo)              │
│  Cartridges: cart/ (from repo)                       │
└─────────────────────────────────────────────────────┘
```

**Entry point:** `bash scripts/dev.sh --model <path> [--goal <text>]`

**Use for:** writing code, testing grammar changes, debugging the
dispatch loop, developing new cartridges. Fast iteration with small
models (0.5B–1.5B).

## Mode 2: Release (bootable SD card for Pi 5)

```
┌─────────────────────────────────────────────────────┐
│  Raspberry Pi 5 (bare metal → Linux kernel → init)  │
│                                                     │
│  PID 1: /init (shell script)                        │
│   ├── mount /model (GGUF partition)                 │
│   ├── arm watchdog (bcm2835-wdt, 15s)               │
│   ├── start networking (loopback + optional eth/wfi) │
│   │                                                 │
│   ├── exec bootloader                               │
│   │    └── forks llama-server, waits <|ready|>      │
│   │                                                 │
│   └── exec iod (replaces init)                      │
│        ├── parser.rs                                │
│        ├── dispatch.rs                              │
│        ├── scheduler.rs                             │
│        └── cloud.rs ───────────────────────────────► Cloud API
│                                                     │
│  SD card layout:                                    │
│   p1: boot   (FAT32, 128M) — kernel, DTBs, firmware│
│   p2: rootfs (ext4,  64M)  — read-only, static bins│
│   p3: model  (ext4,  auto) — GGUF + goal.txt       │
│                                                     │
│  Model: qwen2.5-3b-q4.gguf (release sweet spot)    │
│  Grammar: /etc/isa.gbnf (baked into rootfs)         │
│  Cartridges: /etc/cart/ (baked into rootfs)          │
└─────────────────────────────────────────────────────┘
```

**Entry point:** `bash image/build.sh --model <path> && bash image/flash.sh /dev/sdX`

**Use for:** production deployment. Power on → tokens in ~5 seconds.
Read-only rootfs (power-cut safe). Hardware watchdog auto-reboots on
crash. No SSH, no apt, no attack surface.

## Dual-Brain (both modes)

Both dev and release support cloud fallback for tasks that exceed the
local model's capability.

```
                    ┌──────────────────┐
    ┌───────────────┤   iod (local)    │
    │               └────────┬─────────┘
    │                        │
    │  <|call|>         <|fault|>{needs_cloud:true}
    │  (local syscall)       │
    │                        ▼
    ▼               ┌──────────────────┐
 cartridge          │  cloud.rs        │
 handler            │  → Gemini / GPT  │
                    │  → Claude / vLLM │
                    └──────────────────┘
```

**Configuration:**

Dev mode (env vars or CLI flags):
```bash
bash scripts/dev.sh --model ~/models/qwen2.5-0.5b-q4.gguf \
    --cloud-url "https://generativelanguage.googleapis.com/v1beta" \
    --cloud-key "$GEMINI_API_KEY" \
    --cloud-model "gemini-2.5-flash"
```

Release mode (config.sh on model partition):
```bash
# /model/config.sh (written by build.sh --cloud-* flags)
export LLM_OS_CLOUD_URL="https://generativelanguage.googleapis.com/v1beta"
export LLM_OS_CLOUD_KEY="sk-..."
export LLM_OS_CLOUD_MODEL="gemini-2.5-flash"
```

## Release Features

| Feature | Purpose | Implementation |
|---|---|---|
| Read-only rootfs | Power-cut safety | ext4 mounted ro; traces go to tmpfs |
| Hardware watchdog | Auto-reboot on crash | bcm2835-wdt, 15s timeout, fed by init |
| UART console | Debug without display | stderr → /dev/ttyAMA0; `screen /dev/ttyUSB0 115200` |
| Ethernet DHCP | Cloud fallback | udhcpc on eth0 (auto if cable present) |
| WiFi | Cloud fallback (wireless) | wpa_supplicant + udhcpc on wlan0 |
| Minimal attack surface | Security | No SSH, no shell (except emergency), no package manager |
| Static binaries | No dependency issues | llama-server + iod + bootloader all statically linked |

## Build Matrix

| What | Dev mode | Release mode |
|---|---|---|
| Host | Linux x86_64 / macOS arm64 | Linux x86_64 (cross-compile) |
| Target | Same as host | aarch64 (Pi 5) |
| Compiler | System cc + cargo | Buildroot cross-toolchain + musl |
| Linking | Dynamic (default) | Static (musl libc) |
| Model | 0.5B–1.5B (speed) | 3B–7B (quality) |
| Context | 4096 (dev default) | 32768 (full) |
| Init system | Host OS | Custom /init (PID 1) |
| Grammar | From repo (live) | Baked into rootfs |
| Cartridges | From repo (live) | Baked into rootfs |
