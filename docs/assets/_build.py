"""Generate Tron-monochrome SVG assets for llm_os docs.

Pure black + grayscale + cyan accent. Sets llm_os apart visually from
its sister repos:

  skillos_mini  → orange-Ares       (prefrontal cortex)
  RoClaw        → green-MCP         (cerebellum)
  llm_os        → white-on-black    (kernel · sub-zero)

Color palette:
  bg-deep    #000000
  bg-grid    #0a0a0a
  white      #ffffff     primary highlights · prompt
  pale       #e4e4e7     primary text
  mid        #a1a1aa     secondary text · frame
  dim        #52525b     dim cells / inactive
  deep       #27272a     darkest active state
  cyan       #00d4ff     [OK] · packets · scan line
  cyan-pale  #bff7ff     accent text
"""

from __future__ import annotations
from pathlib import Path

ASSETS = Path(__file__).parent

WHITE     = "#ffffff"
PALE      = "#e4e4e7"
MID       = "#a1a1aa"
DIM       = "#52525b"
DEEP      = "#27272a"
DEEPER    = "#18181b"
CYAN      = "#00d4ff"
CYAN_PALE = "#bff7ff"
BG        = "#000000"
MONO      = "'SF Mono','Menlo','Consolas','Courier New',monospace"

DEFS = f"""
  <defs>
    <filter id="glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="1.5" result="g"/>
      <feMerge><feMergeNode in="g"/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="strongGlow" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="3" result="g1"/>
      <feGaussianBlur stdDeviation="6" result="g2"/>
      <feMerge>
        <feMergeNode in="g2"/><feMergeNode in="g1"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
    <pattern id="scan" x="0" y="0" width="3" height="3" patternUnits="userSpaceOnUse">
      <rect width="3" height="1" fill="{WHITE}" opacity="0.04"/>
    </pattern>
  </defs>"""


def heatmap(x0, y0, cols, rows, cell, gap, seed=0, pulse=True):
    out = []
    rng = [seed * 9301 + 49297]
    def r():
        rng[0] = (rng[0] * 1103515245 + 12345) & 0x7fffffff
        return (rng[0] % 10000) / 10000.0
    for ry in range(rows):
        for cx in range(cols):
            x = x0 + cx * (cell + gap)
            y = y0 + ry * (cell + gap)
            w = r()
            if w > 0.85:
                fill = WHITE
            elif w > 0.65:
                fill = PALE
            elif w > 0.40:
                fill = MID
            elif w > 0.20:
                fill = DIM
            else:
                fill = DEEPER
            base = max(0.18, w)
            phase = (cx * 0.04 + ry * 0.13) % 2.4
            anim = ""
            if pulse:
                anim = (
                    f'<animate attributeName="opacity" '
                    f'values="{base:.2f};{min(1, base+0.35):.2f};{base:.2f}" '
                    f'dur="2.4s" begin="-{phase:.2f}s" repeatCount="indefinite"/>'
                )
            out.append(
                f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" '
                f'fill="{fill}" opacity="{base:.2f}">{anim}</rect>'
            )
    return "\n    ".join(out)


def activity_graph(x0, y0, weeks, days=7, cell=8, gap=2, seed=11):
    out = []
    rng = [seed * 12345]
    def r():
        rng[0] = (rng[0] * 1103515245 + 12345) & 0x7fffffff
        return (rng[0] % 10000) / 10000.0
    for cx in range(weeks):
        for ry in range(days):
            x = x0 + cx * (cell + gap)
            y = y0 + ry * (cell + gap)
            v = r()
            if v > 0.92:
                fill, op = WHITE, 0.95
            elif v > 0.80:
                fill, op = PALE, 0.85
            elif v > 0.62:
                fill, op = MID, 0.7
            elif v > 0.40:
                fill, op = DIM, 0.55
            else:
                fill, op = DEEPER, 1.0
            anim = ""
            if r() > 0.88:
                anim = (
                    f'<animate attributeName="opacity" '
                    f'values="{op:.2f};{max(0.15, op-0.5):.2f};{op:.2f}" '
                    f'dur="3.2s" begin="-{(r()*3.2):.2f}s" repeatCount="indefinite"/>'
                )
            out.append(
                f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" '
                f'fill="{fill}" opacity="{op:.2f}">{anim}</rect>'
            )
    return "\n    ".join(out)


def progress_bar(x, y, length=32, label="", color=PALE):
    full_w = length * 8
    return f"""
    <text x="{x}" y="{y}" font-family="{MONO}" font-size="14"
          fill="{DIM}">{'░' * length}</text>
    <text x="{x}" y="{y}" font-family="{MONO}" font-size="14"
          fill="{color}" filter="url(#glow)">
      <animate attributeName="opacity" values="0;1;1;0;0"
               keyTimes="0;0.05;0.45;0.5;1" dur="6s" repeatCount="indefinite"/>
      {('█' * length)}
    </text>
    <text x="{x + full_w + 12}" y="{y}" font-family="{MONO}" font-size="14"
          fill="{CYAN}" filter="url(#glow)">{label}</text>"""


def blinking_cursor(x, y, w=9, h=16):
    return (
        f'<rect x="{x}" y="{y - h + 3}" width="{w}" height="{h}" '
        f'fill="{WHITE}" filter="url(#glow)">'
        f'<animate attributeName="opacity" values="1;0;1" dur="1.05s" '
        f'repeatCount="indefinite"/></rect>'
    )


def scan_line(width, height):
    return f"""
    <rect x="0" y="-2" width="{width}" height="2" fill="{CYAN}" opacity="0.4" filter="url(#glow)">
      <animate attributeName="y" values="-4;{height};-4" dur="7s" repeatCount="indefinite"/>
      <animate attributeName="opacity" values="0;0.45;0.45;0" keyTimes="0;0.1;0.9;1"
               dur="7s" repeatCount="indefinite"/>
    </rect>"""


def text(x, y, content, fill=PALE, size=14, glow=False, weight="400"):
    f = ' filter="url(#glow)"' if glow else ""
    return (
        f'<text x="{x}" y="{y}" font-family="{MONO}" font-size="{size}" '
        f'fill="{fill}" font-weight="{weight}"{f}>{content}</text>'
    )


# --- HERO ----------------------------------------------------------------
def make_hero():
    W, H = 1280, 360
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" '
        f'role="img" aria-label="llm_os terminal">',
        DEFS,
        f'<rect width="{W}" height="{H}" fill="{BG}"/>',
        text(20, 30, "┌─[ KERNEL://llm_os ]" + "─" * 70 + "[ ring 0 ]" + "─" * 18 + "┐",
             MID, 14, glow=True),
        text(20, H - 14, "└" + "─" * 76 + "[ 13 OPCODES · 8 Hz ]" + "─" * 47 + "┘",
             MID, 14, glow=True),
        # Side bars
        *[text(20, y, "│", MID, 14) for y in range(60, 340, 20)],
        # Boot log
        text(40, 60, "$", WHITE, 14, glow=True, weight="700"),
        text(60, 60, "iod boot --grammar=isa.gbnf --tier=local", PALE, 14),
        text(40, 84, "[", MID, 14),
        text(54, 84, "OK", CYAN, 14, glow=True),
        text(78, 84, "] llama.cpp        ▸  qwen-2.5-3b.gguf  ▸  CPU=0..3 · NEON",
             PALE, 14),
        text(40, 104, "[", MID, 14),
        text(54, 104, "OK", CYAN, 14, glow=True),
        text(78, 104, "] grammar          ▸  isa.gbnf · 13 opcodes · 12/12 fixtures",
             PALE, 14),
        text(40, 124, "[", MID, 14),
        text(54, 124, "OK", CYAN, 14, glow=True),
        text(78, 124, "] capability       ▸  ring 3 default · policy mask active",
             PALE, 14),
        text(40, 144, "[", MID, 14),
        text(54, 144, "OK", CYAN, 14, glow=True),
        text(78, 144, "] cartridges       ▸  6 mounted · system · io · sim · domestic",
             PALE, 14),
        # Section: logit distribution
        text(40, 188, "logit.dist[next_token]   ▸ probability over 13 ISA opcodes",
             PALE, 14, glow=True),
        heatmap(x0=40, y0=200, cols=60, rows=3, cell=14, gap=4, seed=11),
        # Activity graph
        text(40, 276, "syscalls.archive         ▸ dispatches across 52 weeks",
             PALE, 14, glow=True),
        activity_graph(x0=40, y0=288, weeks=52, days=7, cell=8, gap=2, seed=29),
        # Right: status panel
        text(840, 188, "│ TIER",         CYAN, 13),
        text(840, 204, "│ HZ",           CYAN, 13),
        text(840, 220, "│ KV CACHE",     CYAN, 13),
        text(840, 236, "│ RING",         CYAN, 13),
        text(990, 188, "LOCAL · pi-5",   PALE, 13, glow=True),
        text(990, 204, "8.2 Hz · 122ms", WHITE, 13, glow=True),
        text(990, 220, "62% · ok",       PALE, 13),
        text(990, 236, "3 · userland",   PALE, 13),
        # progress
        text(840, 276, "kv compaction ▸ swap",   CYAN, 13),
        progress_bar(840, 300, length=24, label="62%", color=PALE),
        # Final
        text(40, 332, "▸ syscall", CYAN, 14, glow=True),
        text(108, 332, "<|call|>roclaw.forward {\"left\":150,\"right\":150}", PALE, 14),
        blinking_cursor(720, 332),
        f'<rect width="{W}" height="{H}" fill="url(#scan)"/>',
        scan_line(W, H),
        '</svg>',
    ]
    return "\n  ".join(parts)


# --- DIVIDER -------------------------------------------------------------
def make_divider():
    W, H = 1280, 28
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}"
     role="img" aria-label="divider">
  {DEFS.strip()}
  <rect width="{W}" height="{H}" fill="{BG}"/>
  <text x="0" y="20" font-family="{MONO}" font-size="14" fill="{MID}"
        opacity="0.65" filter="url(#glow)">
    {"─ " * 78}
  </text>
  <text x="0" y="20" font-family="{MONO}" font-size="14" fill="{WHITE}"
        filter="url(#glow)">
    <tspan x="0">▸</tspan><tspan dx="6">{"─" * 18}</tspan>
    <tspan dx="8">[ KERNEL ]</tspan><tspan dx="8">{"─" * 18}</tspan>
    <tspan dx="8">▸</tspan>
  </text>
  <text x="0" y="20" font-family="{MONO}" font-size="14" fill="{CYAN_PALE}"
        filter="url(#glow)">
    <tspan>▰▰▰</tspan>
    <animate attributeName="x" values="-40;{W + 10};-40" dur="5.5s"
             repeatCount="indefinite"/>
  </text>
  <text x="0" y="20" font-family="{MONO}" font-size="14" fill="{PALE}"
        opacity="0.6">
    <tspan>·</tspan>
    <animate attributeName="x" values="-10;{W};-10" dur="3.2s"
             repeatCount="indefinite"/>
  </text>
</svg>"""


# --- MARK ----------------------------------------------------------------
def make_mark():
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96"
     role="img" aria-label="llm_os mark">
  {DEFS.strip()}
  <rect width="96" height="96" fill="{BG}"/>
  <text x="48" y="24" text-anchor="middle" font-family="{MONO}" font-size="14"
        fill="{MID}" filter="url(#glow)">┌──────┐</text>
  <text x="48" y="44" text-anchor="middle" font-family="{MONO}" font-size="14"
        fill="{MID}" filter="url(#glow)">│</text>
  <text x="48" y="46" text-anchor="middle" font-family="{MONO}" font-size="18"
        fill="{WHITE}" filter="url(#strongGlow)" font-weight="700">
    ◇
    <animate attributeName="opacity" values="0.6;1;0.6" dur="2.4s"
             repeatCount="indefinite"/>
  </text>
  <text x="48" y="60" text-anchor="middle" font-family="{MONO}" font-size="14"
        fill="{MID}" filter="url(#glow)">│</text>
  <text x="48" y="60" text-anchor="middle" font-family="{MONO}" font-size="14"
        fill="{CYAN_PALE}" filter="url(#glow)">●</text>
  <text x="48" y="76" text-anchor="middle" font-family="{MONO}" font-size="14"
        fill="{MID}" filter="url(#glow)">└──────┘</text>
</svg>"""


# --- BANNER --------------------------------------------------------------
def make_banner(title, doc_id, subtitle, seed, accent_lines):
    W, H = 1280, 240
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" '
        f'role="img" aria-label="{title}">',
        DEFS,
        f'<rect width="{W}" height="{H}" fill="{BG}"/>',
        text(20, 26, "┌─[ " + doc_id + " ]" + "─" * (90 - len(doc_id)) + "┐",
             MID, 13, glow=True),
        text(20, H - 8, "└" + "─" * 110 + "┘", MID, 13, glow=True),
        f'<text x="{W//2}" y="100" text-anchor="middle" font-family="{MONO}" '
        f'font-size="48" fill="{WHITE}" filter="url(#strongGlow)" '
        f'font-weight="700" letter-spacing="6">{title}</text>',
        f'<text x="{W//2}" y="118" text-anchor="middle" font-family="{MONO}" '
        f'font-size="14" fill="{MID}" filter="url(#glow)">'
        f'{"─" * (len(title) + 6)}</text>',
        f'<text x="{W//2}" y="142" text-anchor="middle" font-family="{MONO}" '
        f'font-size="12" fill="{CYAN_PALE}" filter="url(#glow)" '
        f'letter-spacing="3">{subtitle}</text>',
        heatmap(40, 168, cols=22, rows=4, cell=8, gap=2, seed=seed),
        activity_graph(W - 40 - 24 * 10, 168, weeks=24, days=4, cell=8, gap=2, seed=seed * 3),
        text(40, 50, "$ llmos render --doc=" + doc_id.lower(), PALE, 13),
        text(40, 70, "[ OK ] context loaded · 13 opcodes · ring 0 · grammar=isa.gbnf",
             MID, 12, glow=True),
    ]
    for i, line in enumerate(accent_lines):
        parts.append(text(290, 178 + i * 16, line, PALE, 12))
    parts.append(scan_line(W, H))
    parts.append('</svg>')
    return "\n  ".join(parts)


# --- STACK MOCKUP --------------------------------------------------------
def make_stack_mockup():
    """OS-style stack diagram showing CPU/ISA/RAM/MMU mapping."""
    W, H = 1280, 720
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}"
     role="img" aria-label="llm_os stack">
  {DEFS.strip()}
  <rect width="{W}" height="{H}" fill="{BG}"/>
  <rect width="{W}" height="{H}" fill="url(#scan)"/>

  <!-- frame -->
  <text x="40" y="40" font-family="{MONO}" font-size="13" fill="{MID}" filter="url(#glow)">┌─[ KERNEL://stack ]──────────────────────── llm_os v0.5 · ring0..ring3 ──────┐</text>
  <text x="40" y="700" font-family="{MONO}" font-size="13" fill="{MID}" filter="url(#glow)">└─────── fetch · decode · execute ────── 13 ISA OPCODES · GBNF ENFORCED ────┘</text>

  <!-- ring labels -->
  <g font-family="{MONO}" font-size="11">
    <text x="60" y="92" fill="{PALE}" font-weight="700">RING</text>
    <text x="60" y="148" fill="{WHITE}" filter="url(#glow)">3</text>
    <text x="60" y="240" fill="{WHITE}" filter="url(#glow)">2</text>
    <text x="60" y="368" fill="{WHITE}" filter="url(#glow)">1</text>
    <text x="60" y="488" fill="{WHITE}" filter="url(#glow)">0</text>
    <text x="60" y="600" fill="{CYAN}" filter="url(#glow)">HW</text>
  </g>

  <!-- RING 3 · cartridges -->
  <g transform="translate(110, 70)">
    <rect width="1100" height="80" rx="10" fill="{DEEPER}" stroke="{WHITE}" stroke-width="1.5" opacity="0.95" filter="url(#glow)"/>
    <text x="20" y="26" font-family="{MONO}" font-size="13" font-weight="700" fill="{PALE}">⌬ ring 3 · cartridges (userland)</text>
    <text x="20" y="46" font-family="{MONO}" font-size="11" fill="{PALE}" opacity="0.85">cart/system/{{summarize,demo}}  ·  cart/io/roclaw  ·  cart/sim/sim_world  ·  cart/domestic/{{cooking,residential-electrical}}</text>
    <text x="20" y="66" font-family="{MONO}" font-size="11" fill="{MID}">▸ each cartridge = manifest.json + schemas/*.schema.json + dialect.gbnf + handler.rs (or .wasm in v1.0)</text>
  </g>

  <!-- RING 2 · scheduler / iod -->
  <g transform="translate(110, 170)">
    <rect width="1100" height="100" rx="10" fill="{DEEPER}" stroke="{WHITE}" stroke-width="1.5" filter="url(#glow)"/>
    <text x="20" y="26" font-family="{MONO}" font-size="13" font-weight="700" fill="{PALE}">⌬ ring 2 · iod (I/O daemon) + scheduler + multitask</text>
    <g transform="translate(20, 40)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">runtime/iod.rs · /dispatch endpoint</text>
      <text x="0" y="20" font-family="{MONO}" font-size="11" fill="{PALE}">▸ parser · decode tokens →    {'<|call|>cart.method <json>'}</text>
      <text x="0" y="36" font-family="{MONO}" font-size="11" fill="{PALE}">▸ dispatch · validate JSON vs schema · route to handler</text>
      <text x="0" y="52" font-family="{MONO}" font-size="11" fill="{WHITE}">▸ inject · {'<|result|>{"ok":true,...}<|/result|>'}  →  re-feed sampler</text>
    </g>
    <g transform="translate(720, 40)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">scheduler · multitask</text>
      <text x="0" y="20" font-family="{MONO}" font-size="11" fill="{PALE}">▸ {'<|yield|>'} → context-switch slot</text>
      <text x="0" y="36" font-family="{MONO}" font-size="11" fill="{PALE}">▸ {'<|fork|>'} → branched KV cache</text>
      <text x="0" y="52" font-family="{MONO}" font-size="11" fill="{PALE}">▸ {'<|wait|>'} → blocked queue</text>
    </g>
  </g>

  <!-- RING 1 · grammar / capability / dialect -->
  <g transform="translate(110, 290)">
    <rect width="1100" height="120" rx="10" fill="{DEEPER}" stroke="{WHITE}" stroke-width="1.5" filter="url(#glow)"/>
    <text x="20" y="26" font-family="{MONO}" font-size="13" font-weight="700" fill="{PALE}">⌬ ring 1 · grammar (ISA) + capability (logit bias) + dialect (compression)</text>

    <g transform="translate(20, 40)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">grammar/isa.gbnf · 13 opcodes</text>
      <text x="0" y="18" font-family="{MONO}" font-size="11" fill="{PALE}">read · write · call · yield · fork</text>
      <text x="0" y="34" font-family="{MONO}" font-size="11" fill="{PALE}">wait · loop · break · halt · think</text>
      <text x="0" y="50" font-family="{MONO}" font-size="11" fill="{PALE}">commit · fault · policy</text>
      <text x="0" y="68" font-family="{MONO}" font-size="10" fill="{CYAN}">12/12 fixtures green · llama-gbnf-validator</text>
    </g>

    <g transform="translate(380, 40)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">capability · logit bias mask</text>
      <text x="0" y="18" font-family="{MONO}" font-size="11" fill="{PALE}">▸ {'<|call|>'}     → allowed cartridges</text>
      <text x="0" y="34" font-family="{MONO}" font-size="11" fill="{PALE}">▸ banned ops    → −∞ logit at sampler</text>
      <text x="0" y="50" font-family="{MONO}" font-size="11" fill="{PALE}">▸ {'<|policy|>'}  → mutate mask</text>
      <text x="0" y="68" font-family="{MONO}" font-size="10" fill="{CYAN}">backed by daemon-side reject (v1.0)</text>
    </g>

    <g transform="translate(740, 40)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">dialect · compression layer</text>
      <text x="0" y="18" font-family="{MONO}" font-size="11" fill="{PALE}">▸ verbose: {'{"left":150,"right":150}'}</text>
      <text x="0" y="34" font-family="{MONO}" font-size="11" fill="{PALE}">▸ dialect: F 150 150</text>
      <text x="0" y="50" font-family="{MONO}" font-size="11" fill="{WHITE}">▸ saves ~7× tokens / call</text>
      <text x="0" y="68" font-family="{MONO}" font-size="10" fill="{CYAN}">8 Hz Pi 5 budget made possible</text>
    </g>
  </g>

  <!-- RING 0 · kv cache + compaction (RAM + swap) -->
  <g transform="translate(110, 430)">
    <rect width="1100" height="80" rx="10" fill="{DEEPER}" stroke="{WHITE}" stroke-width="1.5" filter="url(#glow)"/>
    <text x="20" y="26" font-family="{MONO}" font-size="13" font-weight="700" fill="{PALE}">⌬ ring 0 · KV cache (= RAM) + compaction (= swap)</text>

    <!-- KV bar visualization -->
    <g transform="translate(20, 36)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">kv layout · 8192 ctx</text>
      <g transform="translate(0, 14)">
        <!-- system prompt -->
        <rect width="80" height="20" fill="{WHITE}" opacity="0.85"/>
        <text x="40" y="14" text-anchor="middle" font-size="9" font-family="{MONO}" fill="{BG}">system</text>
        <!-- grammar / dialect -->
        <rect x="84" width="50" height="20" fill="{PALE}" opacity="0.7"/>
        <text x="109" y="14" text-anchor="middle" font-size="9" font-family="{MONO}" fill="{BG}">isa</text>
        <!-- working set -->
        <rect x="138" width="280" height="20" fill="{MID}" opacity="0.6"/>
        <text x="278" y="14" text-anchor="middle" font-size="9" font-family="{MONO}" fill="{BG}">working set · 62%</text>
        <!-- compacted summary -->
        <rect x="422" width="70" height="20" fill="{DIM}" stroke="{CYAN}" stroke-dasharray="3 2" stroke-width="1"/>
        <text x="457" y="14" text-anchor="middle" font-size="9" font-family="{MONO}" fill="{PALE}">summary</text>
        <!-- free -->
        <rect x="496" width="200" height="20" fill="none" stroke="{DIM}" stroke-width="1" stroke-dasharray="2 2"/>
        <text x="596" y="14" text-anchor="middle" font-size="9" font-family="{MONO}" fill="{DIM}">free</text>
      </g>
      <text x="0" y="56" font-family="{MONO}" font-size="9" fill="{MID}">compaction triggers @ 70% util · ISA-aware (preserves loop depth + pending result expectations)</text>
    </g>

    <g transform="translate(820, 50)">
      <text font-family="{MONO}" font-size="10" fill="{MID}">paging</text>
      <text x="0" y="16" font-family="{MONO}" font-size="11" fill="{WHITE}" filter="url(#glow)">page in · page out</text>
      <rect x="0" y="-3" width="14" height="6" fill="{CYAN}" filter="url(#glow)">
        <animate attributeName="x" values="-20;360;-20" dur="3.2s" repeatCount="indefinite"/>
      </rect>
      <line x1="0" y1="0" x2="340" y2="0" stroke="{MID}" stroke-width="1" opacity="0.5"/>
    </g>
  </g>

  <!-- HW · llama.cpp + bootloader.c -->
  <g transform="translate(110, 530)">
    <rect width="1100" height="80" rx="10" fill="{DEEPER}" stroke="{CYAN}" stroke-width="1.5" filter="url(#glow)"/>
    <text x="20" y="26" font-family="{MONO}" font-size="13" font-weight="700" fill="{CYAN_PALE}">◆ hw · llama.cpp (= CPU) + bootloader.c</text>
    <text x="20" y="46" font-family="{MONO}" font-size="11" fill="{CYAN_PALE}" opacity="0.85">sampler loop = fetch · decode · execute  ·  GBNF = MMU type safety  ·  bias mask = ring transitions</text>
    <text x="20" y="66" font-family="{MONO}" font-size="11" fill="{CYAN_PALE}">▸ pi 5: 8 Hz (4×NEON · qwen-2.5-3b-q4)  ·  cloud A100: 200 Hz (gemini 2.5 flash via http)</text>
    <circle cx="1060" cy="40" r="6" fill="{CYAN}" filter="url(#strongGlow)">
      <animate attributeName="opacity" values="0.4;1;0.4" dur="1.6s" repeatCount="indefinite"/>
    </circle>
    <text x="1078" y="44" font-family="{MONO}" font-size="10" fill="{CYAN_PALE}">8.2 Hz</text>
  </g>

  <!-- arrows -->
  <g stroke="{MID}" stroke-width="1" stroke-dasharray="4 3" fill="none" opacity="0.55" filter="url(#glow)">
    <path d="M 660 152 L 660 168"/>
    <path d="M 660 270 L 660 290"/>
    <path d="M 660 410 L 660 430"/>
    <path d="M 660 510 L 660 530"/>
  </g>
  <g fill="{MID}" filter="url(#glow)">
    <polygon points="657,168 660,160 663,168"/>
    <polygon points="657,290 660,282 663,290"/>
    <polygon points="657,430 660,422 663,430"/>
    <polygon points="657,530 660,522 663,530"/>
  </g>

  <!-- live LED -->
  <circle cx="50" cy="690" r="3" fill="{WHITE}" filter="url(#strongGlow)">
    <animate attributeName="opacity" values="0.4;1;0.4" dur="1.4s" repeatCount="indefinite"/>
  </circle>
  <text x="62" y="694" font-family="{MONO}" font-size="10" fill="{PALE}" opacity="0.7">live</text>

  <!-- scan line -->
  <rect x="0" y="-2" width="1280" height="2" fill="{CYAN}" opacity="0.35" filter="url(#glow)">
    <animate attributeName="y" values="-4;720;-4" dur="9s" repeatCount="indefinite"/>
    <animate attributeName="opacity" values="0;0.4;0.4;0" keyTimes="0;0.1;0.9;1" dur="9s" repeatCount="indefinite"/>
  </rect>
</svg>"""


def main():
    (ASSETS / "hero.svg").write_text(make_hero())
    (ASSETS / "divider.svg").write_text(make_divider())
    (ASSETS / "mark.svg").write_text(make_mark())
    (ASSETS / "stack-mockup.svg").write_text(make_stack_mockup())

    (ASSETS / "banner-architecture.svg").write_text(make_banner(
        title="ARCHITECTURE",
        doc_id="doc//arch",
        subtitle="LLM=CPU · GBNF=ISA · KV=RAM · COMPACT=SWAP",
        seed=5,
        accent_lines=[
            "▸ ring 3   userland       ▸  cartridges · per-method GBNF · dialect compression",
            "▸ ring 1-2 kernel         ▸  iod daemon · scheduler · capability mask",
            "▸ ring 0   hw             ▸  llama.cpp sampler loop · KV cache · GBNF MMU",
        ],
    ))
    (ASSETS / "banner-usage.svg").write_text(make_banner(
        title="USAGE GUIDE",
        doc_id="doc//use",
        subtitle="BOOT · MOUNT · DISPATCH · INSPECT",
        seed=7,
        accent_lines=[
            "▸ boot       $ ./scripts/quickstart.sh    ▸  llama.cpp · iod · grammar=isa.gbnf",
            "▸ dispatch   POST /dispatch                ▸  goal → tokens → call → result",
            "▸ inspect    GET /trace                    ▸  KV utilization · faults · faults",
        ],
    ))
    (ASSETS / "banner-tutorial.svg").write_text(make_banner(
        title="TUTORIAL",
        doc_id="doc//build",
        subtitle="WRITE YOUR FIRST CARTRIDGE · 30 MIN",
        seed=11,
        accent_lines=[
            "▸ scaffold   $ mkdir cart/system/timer/{schemas}",
            "▸ author     $ vim manifest.json     ▸  3 syscalls · set · list · cancel",
            "▸ run        POST /dispatch          ▸  '<|call|>timer.set {dur:30s}'",
        ],
    ))
    (ASSETS / "banner-roadmap.svg").write_text(make_banner(
        title="NEXT STEPS",
        doc_id="doc//roadmap",
        subtitle="GRAMMAR · COMPACT · CAPS · WASM",
        seed=17,
        accent_lines=[
            "▸ grammar    in-llama.cpp swap            ▸  3 HTTP → 1 sampler hook",
            "▸ compact    ISA-aware swap               ▸  preserve loop depth + pending state",
            "▸ caps       three-strikes + WASM         ▸  daemon-side reject · ring 3 sandbox",
        ],
    ))

    for f in sorted(ASSETS.glob("*.svg")):
        kb = f.stat().st_size / 1024
        print(f"  ✓ {f.name:32s} {kb:6.1f} KB")


if __name__ == "__main__":
    main()
