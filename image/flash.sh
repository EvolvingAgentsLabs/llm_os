#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# Flash LLM-OS SD card image to a target device.
#
# Usage:
#   bash image/flash.sh /dev/sdX          # Linux
#   bash image/flash.sh /dev/disk4        # macOS (unmounts first)
#
# WARNING: This will ERASE the target device. Double-check the path.
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="$ROOT/image/output/sdcard.img"

if [ $# -ne 1 ]; then
    echo "Usage: $0 /dev/sdX"
    echo ""
    echo "List available devices:"
    if [ "$(uname)" = "Darwin" ]; then
        echo "  diskutil list"
    else
        echo "  lsblk"
    fi
    exit 1
fi

DEVICE="$1"

if [ ! -f "$IMAGE" ]; then
    echo "error: image not found at $IMAGE"
    echo "Run 'bash image/build.sh --model <path>' first."
    exit 1
fi

if [ ! -b "$DEVICE" ]; then
    echo "error: $DEVICE is not a block device"
    exit 1
fi

# Safety check — refuse to write to the boot disk
BOOT_DISK=""
if [ "$(uname)" = "Darwin" ]; then
    BOOT_DISK=$(diskutil info / | grep "Device Node" | awk '{print $NF}' | sed 's/s[0-9]*$//')
else
    BOOT_DISK=$(findmnt -no SOURCE / | sed 's/[0-9]*$//' | sed 's/p[0-9]*$//')
fi

if [ "$DEVICE" = "$BOOT_DISK" ]; then
    echo "error: refusing to write to boot disk ($BOOT_DISK)"
    exit 1
fi

IMAGE_SIZE=$(stat -c%s "$IMAGE" 2>/dev/null || stat -f%z "$IMAGE")
IMAGE_SIZE_MB=$(( IMAGE_SIZE / 1048576 ))

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  LLM-OS Flash                                              ║"
echo "║                                                            ║"
echo "║  Image:   $(basename "$IMAGE") (${IMAGE_SIZE_MB}MB)"
echo "║  Target:  $DEVICE"
echo "║                                                            ║"
echo "║  WARNING: ALL DATA ON $DEVICE WILL BE ERASED"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
printf "Type 'yes' to proceed: "
read -r CONFIRM
if [ "$CONFIRM" != "yes" ]; then
    echo "Aborted."
    exit 0
fi

# Unmount on macOS
if [ "$(uname)" = "Darwin" ]; then
    diskutil unmountDisk "$DEVICE" 2>/dev/null || true
    RAW_DEVICE=$(echo "$DEVICE" | sed 's|/dev/disk|/dev/rdisk|')
    echo "Flashing to $RAW_DEVICE (raw device for speed)..."
    sudo dd if="$IMAGE" of="$RAW_DEVICE" bs=4m status=progress
    sync
    diskutil eject "$DEVICE"
else
    # Unmount all partitions on Linux
    for p in "${DEVICE}"*; do
        sudo umount "$p" 2>/dev/null || true
    done
    echo "Flashing to $DEVICE..."
    sudo dd if="$IMAGE" of="$DEVICE" bs=4M status=progress conv=fsync
    sync
fi

echo ""
echo "Done. Insert SD card into Pi 5 and power on."
echo "Serial console: screen /dev/ttyUSB0 115200  (or minicom)"
