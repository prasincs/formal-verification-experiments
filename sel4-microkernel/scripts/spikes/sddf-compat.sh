#!/usr/bin/env bash
#
# WP-0 substrate spike: is au-ts/sDDF adoptable against THIS repo's pinned
# Microkit (2.1.0) and toolchain, or does it force a version bump?
#
# The spike is deliberately layered so that each answer is *evidence*, not
# inference:
#
#   1. record sDDF's declared Microkit-SDK requirement vs. our pin;
#   2. probe whether the pinned Microkit 2.1.0 SDK is even fetchable here
#      (a full example `make` needs it);
#   3. compile sDDF's device-independent C against its own `extern`
#      "bring-your-own-OS" shim with our clang, NO Microkit SDK — this is
#      the surface that could be lifted into our tree, so we prove it
#      builds in isolation;
#   4. print the `extern` FFI surface a foreign OS (our Rust PDs) would
#      have to provide.
#
# Exit codes:
#   0  probe ran; findings printed (this is a report, not a gate)
#   1  environment problem (no git / no clang / clone failed)
#   2  --expect-incompatible was passed AND sDDF now builds an example
#      against our pinned Microkit unaided (i.e. the documented mismatch
#      has gone stale and this memo should be revisited)
#
# Reproducibility: sDDF is pinned by commit; the Microkit requirement is
# read from sDDF's own README. Re-running on a machine WITH the Microkit
# 2.1.0 SDK on $MICROKIT_SDK will additionally attempt an example build.

set -u

# --- pins (update deliberately; see docs/substrate-decision.md) -------------
SDDF_REPO="https://github.com/au-ts/sddf.git"
SDDF_COMMIT="e7788aad7db2db74091f5153aa9ed6b121229944"   # 2026-07, post-0.6.0
OUR_MICROKIT="2.1.0"                                     # versions.mk
SDDF_SDFGEN_PIN="0.33.0"                                 # sDDF README metaprogram

EXPECT_INCOMPATIBLE=0
[ "${1:-}" = "--expect-incompatible" ] && EXPECT_INCOMPATIBLE=1

say() { printf '%s\n' "$*"; }
hr()  { printf -- '---- %s\n' "$*"; }

command -v git   >/dev/null || { say "FATAL: git not found";   exit 1; }
command -v clang >/dev/null || { say "FATAL: clang not found"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
SDDF="$WORK/sddf"

hr "cloning sDDF @ ${SDDF_COMMIT:0:12}"
if ! git clone --quiet --depth 64 "$SDDF_REPO" "$SDDF" 2>/dev/null \
   || ! git -C "$SDDF" checkout --quiet "$SDDF_COMMIT" 2>/dev/null; then
    say "FATAL: could not clone/checkout sDDF (offline?) — cannot run spike"
    exit 1
fi
say "sDDF HEAD: $(git -C "$SDDF" rev-parse HEAD)"

# --- 1. declared Microkit requirement vs. our pin ---------------------------
hr "Microkit version coupling"
SDDF_MK="$(grep -oE 'Microkit SDK [0-9]+\.[0-9]+\.[0-9]+' "$SDDF/README.md" \
           | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
say "  sDDF requires Microkit SDK : ${SDDF_MK:-unknown}"
say "  this repo pins Microkit    : ${OUR_MICROKIT}"
say "  sDDF config metaprogram    : sdfgen==${SDDF_SDFGEN_PIN} (auto-generates .system + per-PD config)"
MISMATCH=1
[ "$SDDF_MK" = "$OUR_MICROKIT" ] && MISMATCH=0

# --- 2. is the pinned SDK fetchable in this environment? --------------------
hr "Microkit ${OUR_MICROKIT} SDK reachability (needed for a full example build)"
SDK_URL="https://github.com/seL4/microkit/releases/download/${OUR_MICROKIT}/microkit-sdk-${OUR_MICROKIT}-linux-x86-64.tar.gz"
SDK_CODE="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 25 -L "$SDK_URL" 2>/dev/null || echo "000")"
say "  GET $SDK_URL"
say "  -> HTTP ${SDK_CODE}$( [ "$SDK_CODE" = 200 ] && echo ' (available)' || echo ' (not fetchable here — full make/boot spike cannot run)')"

# --- 3. compile the liftable surface against the extern OS shim -------------
hr "compile sDDF device-independent C against the extern (BYO-OS) shim, no SDK"
CC=(clang --target=aarch64-none-elf -ffreestanding -c -I"$SDDF/include/extern" -I"$SDDF/include")
CLEAN=0; DIRTY=0
for f in util/fsmalloc.c util/bitarray.c util/printf.c util/cache.c; do
    [ -f "$SDDF/$f" ] || continue
    if err="$("${CC[@]}" -o /dev/null "$SDDF/$f" 2>&1)"; then
        say "  OK   $f"; CLEAN=$((CLEAN+1))
    else
        first="$(printf '%s\n' "$err" | grep -m1 -oE "'[^']+' file not found|error:.*" | head -1)"
        say "  DEP  $f  (needs OS/seL4 headers: ${first})"; DIRTY=$((DIRTY+1))
    fi
done
say "  => ${CLEAN} pure-C TU(s) build unaided; ${DIRTY} need the OS/seL4 integration layer"

# --- 4. the extern FFI surface a foreign OS must implement ------------------
hr "extern shim FFI surface (what our Rust PDs would provide to host sDDF)"
grep -oE 'extern [a-zA-Z_0-9 ]+\**sddf_[a-zA-Z_0-9]+' "$SDDF/include/extern/os/sddf.h" \
    | sed 's/^/  /' | head -20

# --- optional: full example build if an SDK is present ----------------------
BUILT_UNAIDED=0
if [ -n "${MICROKIT_SDK:-}" ] && [ -d "${MICROKIT_SDK:-/nonexistent}" ]; then
    hr "MICROKIT_SDK present — attempting a real example build"
    if make -C "$SDDF/examples/serial" \
            MICROKIT_SDK="$MICROKIT_SDK" MICROKIT_BOARD=qemu_virt_aarch64 \
            >/"$WORK/build.log" 2>&1; then
        say "  example built against MICROKIT_SDK=$MICROKIT_SDK"
        BUILT_UNAIDED=1
    else
        say "  example build FAILED — first error:"
        grep -m1 -iE 'error|not found|version' "$WORK/build.log" | sed 's/^/    /'
    fi
else
    say ""
    say "(no MICROKIT_SDK in env — skipping the full example build; set MICROKIT_SDK"
    say " to a Microkit ${OUR_MICROKIT} SDK to attempt examples/serial for qemu_virt_aarch64)"
fi

hr "verdict"
if [ "$MISMATCH" = 1 ]; then
    say "  DECISION SUPPORT: sDDF HEAD targets Microkit ${SDDF_MK:-?}, repo pins ${OUR_MICROKIT}."
    say "  Substrate migration is out for Wave 1 (ground rule 1 forbids the Microkit bump);"
    say "  device-independent sDDF C is liftable behind the extern shim (see counts above)."
else
    say "  sDDF's declared Microkit matches our pin — revisit docs/substrate-decision.md."
fi

if [ "$EXPECT_INCOMPATIBLE" = 1 ]; then
    if [ "$MISMATCH" = 0 ]; then
        say ""
        say "TRIPWIRE: --expect-incompatible set, but sDDF now declares Microkit"
        say "${SDDF_MK} == our pin ${OUR_MICROKIT}. The documented mismatch is gone;"
        say "revisit docs/substrate-decision.md."
        exit 2
    fi
    if [ "$BUILT_UNAIDED" = 1 ]; then
        say ""
        say "TRIPWIRE: --expect-incompatible set, yet an sDDF example built against our"
        say "pinned Microkit. The documented mismatch is stale; update the memo."
        exit 2
    fi
fi
exit 0
