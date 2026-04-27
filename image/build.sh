#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# LLM-OS image builder
#
# Builds a bootable SD card image for Raspberry Pi 5 that runs
# llama-server + iod as PID 1. The entire machine becomes the LLM-OS.
#
# Prerequisites:
#   - Linux x86_64 host (Buildroot cross-compiles for aarch64)
#   - ~8 GB disk for Buildroot toolchain + build artifacts
#   - A GGUF model file to include on the model partition
#
# Usage:
#   bash image/build.sh --model ~/models/qwen2.5-3b-q4.gguf
#   bash image/build.sh --model ~/models/qwen2.5-3b-q4.gguf --goal "echo via demo"
#   bash image/build.sh --model ~/models/qwen2.5-3b-q4.gguf --wifi-ssid MyNet --wifi-pass secret
#
# Output:
#   image/output/sdcard.img   — ready to flash with image/flash.sh
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE_DIR="$ROOT/image"
BUILD_DIR="$IMAGE_DIR/buildroot-build"
OVERLAY_DIR="$IMAGE_DIR/overlay"
OUTPUT_DIR="$IMAGE_DIR/output"
BUILDROOT_VERSION="2024.02.9"
BUILDROOT_URL="https://buildroot.org/downloads/buildroot-${BUILDROOT_VERSION}.tar.xz"
BUILDROOT_DIR="$IMAGE_DIR/buildroot-${BUILDROOT_VERSION}"

# ── Parse args ───────────────────────────────────────────────────────
MODEL_FILE=""
GOAL="await"
WIFI_SSID=""
WIFI_PASS=""
MODEL_PARTITION_SIZE="auto"
CLOUD_URL=""
CLOUD_KEY=""
CLOUD_MODEL=""

while [ $# -gt 0 ]; do
    case "$1" in
        --model)       MODEL_FILE="$2"; shift 2 ;;
        --goal)        GOAL="$2"; shift 2 ;;
        --wifi-ssid)   WIFI_SSID="$2"; shift 2 ;;
        --wifi-pass)   WIFI_PASS="$2"; shift 2 ;;
        --partition-size) MODEL_PARTITION_SIZE="$2"; shift 2 ;;
        --cloud-url)   CLOUD_URL="$2"; shift 2 ;;
        --cloud-key)   CLOUD_KEY="$2"; shift 2 ;;
        --cloud-model) CLOUD_MODEL="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 --model PATH [--goal TEXT] [--wifi-ssid SSID --wifi-pass PASS]"
            echo "       [--cloud-url URL --cloud-key KEY --cloud-model MODEL]"
            exit 0 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

if [ -z "$MODEL_FILE" ]; then
    echo "error: --model is required (path to a .gguf file)"
    exit 1
fi

if [ ! -f "$MODEL_FILE" ]; then
    echo "error: model file not found: $MODEL_FILE"
    exit 1
fi

# ── Compute model partition size ─────────────────────────────────────
MODEL_SIZE_MB=$(( $(stat -c%s "$MODEL_FILE" 2>/dev/null || stat -f%z "$MODEL_FILE") / 1048576 ))
if [ "$MODEL_PARTITION_SIZE" = "auto" ]; then
    # Model + 256MB headroom for traces and config
    MODEL_PARTITION_SIZE=$(( MODEL_SIZE_MB + 256 ))
fi
echo "[1/8] model: $MODEL_FILE (${MODEL_SIZE_MB}MB, partition: ${MODEL_PARTITION_SIZE}MB)"

# ── 1. Download Buildroot ────────────────────────────────────────────
if [ ! -d "$BUILDROOT_DIR" ]; then
    echo "[2/8] downloading Buildroot ${BUILDROOT_VERSION}"
    mkdir -p "$IMAGE_DIR"
    curl -L "$BUILDROOT_URL" | tar -xJ -C "$IMAGE_DIR"
else
    echo "[2/8] Buildroot already at $BUILDROOT_DIR — skipping"
fi

# ── 2. Cross-compile llama-server (static, aarch64) ─────────────────
echo "[3/8] cross-compiling llama-server for aarch64"
LLAMA_DIR="${LLAMA_DIR:-$ROOT/../llama.cpp}"
if [ ! -d "$LLAMA_DIR" ]; then
    git clone --depth 1 https://github.com/ggerganov/llama.cpp.git "$LLAMA_DIR"
fi

# Use Buildroot's cross-compiler if available, else system aarch64-linux-gnu-
CROSS="${BUILDROOT_DIR}/output/host/bin/aarch64-buildroot-linux-gnu-"
if [ ! -f "${CROSS}gcc" ]; then
    CROSS="aarch64-linux-gnu-"
fi

(cd "$LLAMA_DIR" \
    && cmake -B build-aarch64 \
        -DCMAKE_C_COMPILER="${CROSS}gcc" \
        -DCMAKE_CXX_COMPILER="${CROSS}g++" \
        -DCMAKE_SYSTEM_NAME=Linux \
        -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
        -DLLAMA_CURL=OFF \
        -DBUILD_SHARED_LIBS=OFF \
        -DLLAMA_NATIVE=OFF \
    && cmake --build build-aarch64 --config Release --target llama-server -j"$(nproc)")
cp "$LLAMA_DIR/build-aarch64/bin/llama-server" "$OVERLAY_DIR/usr/bin/llama-server"

# ── 3. Cross-compile bootloader (static C) ──────────────────────────
echo "[4/8] cross-compiling bootloader for aarch64"
${CROSS}gcc -O2 -Wall -Wextra -static \
    -o "$OVERLAY_DIR/usr/bin/bootloader" \
    "$ROOT/runtime/bootloader.c"

# ── 4. Cross-compile iod (static Rust, musl) ────────────────────────
echo "[5/8] cross-compiling iod for aarch64-unknown-linux-musl"
rustup target add aarch64-unknown-linux-musl 2>/dev/null || true
(cd "$ROOT/runtime" \
    && CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="${CROSS}gcc" \
       cargo build --release --target aarch64-unknown-linux-musl --bin iod)
cp "$ROOT/runtime/target/aarch64-unknown-linux-musl/release/iod" \
   "$OVERLAY_DIR/usr/bin/iod"

# ── 5. Copy grammar + cartridges to overlay ─────────────────────────
echo "[6/8] copying grammar and cartridges to overlay"
cp "$ROOT/grammar/isa.gbnf" "$OVERLAY_DIR/etc/isa.gbnf"
cp -r "$ROOT/cart/"* "$OVERLAY_DIR/etc/cart/"

# ── 6. Generate WiFi config (optional) ──────────────────────────────
if [ -n "$WIFI_SSID" ] && [ -n "$WIFI_PASS" ]; then
    echo "[6.5/8] generating wpa_supplicant.conf for $WIFI_SSID"
    cat > "$OVERLAY_DIR/etc/wpa_supplicant.conf" <<WPAEOF
ctrl_interface=/run/wpa_supplicant
update_config=0
country=US

network={
    ssid="$WIFI_SSID"
    psk="$WIFI_PASS"
    key_mgmt=WPA-PSK
}
WPAEOF
fi

# ── 7. Build Buildroot image ────────────────────────────────────────
echo "[7/8] building Buildroot image (this may take a while on first run)"

# Patch genimage.cfg with computed partition size
sed "s/size = 2048M/size = ${MODEL_PARTITION_SIZE}M/" \
    "$IMAGE_DIR/genimage.cfg" > "$BUILD_DIR/genimage.cfg" 2>/dev/null || \
    sed "s/size = 2048M/size = ${MODEL_PARTITION_SIZE}M/" \
    "$IMAGE_DIR/genimage.cfg" > "/tmp/llm_os_genimage.cfg"

cp "$IMAGE_DIR/buildroot_defconfig" \
   "$BUILDROOT_DIR/configs/llm_os_rpi5_defconfig"

(cd "$BUILDROOT_DIR" \
    && make llm_os_rpi5_defconfig O="$BUILD_DIR" \
    && make -j"$(nproc)" O="$BUILD_DIR")

# ── 8. Inject model into model partition ─────────────────────────────
echo "[8/8] injecting model + goal into SD card image"
mkdir -p "$OUTPUT_DIR"
cp "$BUILD_DIR/images/sdcard.img" "$OUTPUT_DIR/sdcard.img"

# Mount model partition (partition 3), copy model + goal, unmount
# This uses a loop device — requires root.
LOOP=$(sudo losetup --find --show --partscan "$OUTPUT_DIR/sdcard.img")
MODEL_PART="${LOOP}p3"

sudo mkdir -p /mnt/llm_os_model
sudo mount "$MODEL_PART" /mnt/llm_os_model
sudo cp "$MODEL_FILE" /mnt/llm_os_model/
echo "$GOAL" | sudo tee /mnt/llm_os_model/goal.txt > /dev/null
sudo mkdir -p /mnt/llm_os_model/traces

# Cloud config (optional)
if [ -n "$CLOUD_URL" ]; then
    sudo mkdir -p /mnt/llm_os_model/.config
    cat <<CLOUDEOF | sudo tee /mnt/llm_os_model/config.sh > /dev/null
# Cloud fallback (dual-brain)
export LLM_OS_CLOUD_URL="$CLOUD_URL"
export LLM_OS_CLOUD_KEY="$CLOUD_KEY"
export LLM_OS_CLOUD_MODEL="$CLOUD_MODEL"
CLOUDEOF
fi

sudo umount /mnt/llm_os_model
sudo losetup -d "$LOOP"

echo ""
echo "================================================================"
echo "  LLM-OS SD card image ready:"
echo "    $OUTPUT_DIR/sdcard.img"
echo ""
echo "  Flash with:  bash image/flash.sh /dev/sdX"
echo "  Model:       $(basename "$MODEL_FILE") (${MODEL_SIZE_MB}MB)"
echo "  Goal:        $GOAL"
echo "================================================================"
