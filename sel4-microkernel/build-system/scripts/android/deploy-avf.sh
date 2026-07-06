#!/bin/sh
# Stage a seL4 Microkit system image on an Android device for AVF/crosvm.
#
# Pushes the image and the on-device launcher (run-crosvm.sh) to a
# staging directory via adb, and reports whether the device looks
# capable of the raw-crosvm path (AVF APEX present, root available).
#
# Usage:
#   deploy-avf.sh --image build/.../loader.img --stage-dir /data/local/tmp/sel4-agent
#
# Environment:
#   ADB             adb binary (default: adb)
#   ANDROID_SERIAL  device serial for multi-device hosts (optional)
#   CROSVM_BIN      crosvm path checked on the device
#                   (default: /apex/com.android.virt/bin/crosvm)

set -eu

ADB="${ADB:-adb}"
CROSVM_BIN="${CROSVM_BIN:-/apex/com.android.virt/bin/crosvm}"
IMAGE=""
STAGE_DIR="/data/local/tmp/sel4-agent"

usage() {
    sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --image)     IMAGE="$2"; shift 2 ;;
        --stage-dir) STAGE_DIR="$2"; shift 2 ;;
        -h|--help)   usage ;;
        *) echo "error: unknown argument: $1" >&2; usage 1 >&2 ;;
    esac
done

[ -n "$IMAGE" ] || { echo "error: --image is required" >&2; exit 1; }
[ -f "$IMAGE" ] || { echo "error: image not found: $IMAGE" >&2; exit 1; }

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
LAUNCHER="$SCRIPT_DIR/run-crosvm.sh"
[ -f "$LAUNCHER" ] || { echo "error: launcher not found: $LAUNCHER" >&2; exit 1; }

if ! command -v "$ADB" >/dev/null 2>&1; then
    echo "error: adb not found ('$ADB'). Install Android platform-tools." >&2
    exit 1
fi

adb_cmd() {
    if [ -n "${ANDROID_SERIAL:-}" ]; then
        "$ADB" -s "$ANDROID_SERIAL" "$@"
    else
        "$ADB" "$@"
    fi
}

state=$(adb_cmd get-state 2>/dev/null || true)
if [ "$state" != "device" ]; then
    echo "error: no Android device in 'device' state (adb get-state: '${state:-none}')." >&2
    echo "       Connect a device with USB debugging enabled, or set ANDROID_SERIAL." >&2
    exit 1
fi

echo "Staging to $STAGE_DIR"
adb_cmd shell mkdir -p "$STAGE_DIR"
adb_cmd push "$IMAGE" "$STAGE_DIR/loader.img"
adb_cmd push "$LAUNCHER" "$STAGE_DIR/run-crosvm.sh"

echo ""
echo "=== Device capability check ==="

if adb_cmd shell "test -x $CROSVM_BIN" >/dev/null 2>&1; then
    echo "  crosvm:  present ($CROSVM_BIN)"
else
    echo "  crosvm:  NOT FOUND at $CROSVM_BIN"
    echo "           The device needs Android 14+ with the AVF"
    echo "           virtualization APEX (com.android.virt)."
fi

# Raw crosvm (bypassing VirtualizationService) needs a root shell:
# rooted device, or a userdebug build where `adb root` works.
uid=$(adb_cmd shell id -u 2>/dev/null | tr -d '[:space:]' || echo "")
if [ "$uid" = "0" ]; then
    echo "  root:    adb shell is already root"
elif adb_cmd shell "command -v su >/dev/null 2>&1" >/dev/null 2>&1; then
    echo "  root:    available via su (launcher will use it)"
else
    echo "  root:    NOT AVAILABLE — raw crosvm launch will fail."
    echo "           Use a rooted or userdebug device, or use the"
    echo "           no-root Termux path: make ... termux-bundle"
fi

echo ""
echo "Staged. Boot with:"
echo "  make PRODUCT=<product> PLATFORM=android-avf run-avf"
echo "or on the device:"
echo "  adb shell sh $STAGE_DIR/run-crosvm.sh"
