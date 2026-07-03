#!/bin/sh
# test-kconfig.sh — self-test for the Kconfig-style configuration tool
#
# Exercises scripts/kconfig.sh (resolve + gensystem) against synthetic
# fixtures and the real Kconfig/defconfig/.system files in the repo.
# Runs on any POSIX shell with awk; no other dependencies.
#
#   ./test-kconfig.sh          run all tests, exit non-zero on failure

set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
KC="$HERE/kconfig.sh"
BS=$(dirname "$HERE")                       # build-system/
ROOT=$(dirname "$BS")                       # sel4-microkernel/

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

PASS=0
FAIL=0

ok() {
    PASS=$((PASS + 1))
    printf 'ok   %s\n' "$1"
}

fail() {
    FAIL=$((FAIL + 1))
    printf 'FAIL %s\n' "$1" >&2
}

# assert_eq <label> <expected> <actual>
assert_eq() {
    if [ "$2" = "$3" ]; then ok "$1"; else
        fail "$1 (expected '$2', got '$3')"
    fi
}

# expect_fail <label> — command must exit non-zero and print an error
expect_fail() {
    label=$1
    shift
    if "$@" >/dev/null 2>"$TMP/err"; then
        fail "$label (expected failure, got success)"
    elif ! grep -q "kconfig: error:" "$TMP/err"; then
        fail "$label (failed without a kconfig error message)"
    else
        ok "$label"
    fi
}

# config_value <.config> <NAME> — prints y or n
config_value() {
    if grep -q "^CONFIG_$2=y$" "$1"; then echo y
    elif grep -q "^# CONFIG_$2 is not set$" "$1"; then echo n
    else echo missing
    fi
}

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

cat >"$TMP/Kconfig" <<'EOF'
menu "Test options"

config ALPHA
	bool "Alpha"
	default y
	help
	  First test option.

config BETA
	bool "Beta"
	default n

config GAMMA
	bool "Gamma, needs beta"
	default n
	depends on BETA

config DELTA
	bool "Delta, needs beta and not alpha"
	default n
	depends on BETA && !ALPHA

endmenu
EOF

cat >"$TMP/defconfig" <<'EOF'
# comment and blank lines are fine

CONFIG_BETA=y
# CONFIG_ALPHA is not set
EOF

resolve() {
    "$KC" resolve --kconfig "$TMP/Kconfig" "$@" \
        --out-config "$TMP/.config" --out-mk "$TMP/config.mk"
}

# ---------------------------------------------------------------------------
# resolve: layering
# ---------------------------------------------------------------------------

resolve || fail "resolve with defaults only"
assert_eq "default y is applied"            y "$(config_value "$TMP/.config" ALPHA)"
assert_eq "default n is applied"            n "$(config_value "$TMP/.config" BETA)"

resolve --defconfig "$TMP/defconfig" || fail "resolve with defconfig"
assert_eq "defconfig =y overrides default"          y "$(config_value "$TMP/.config" BETA)"
assert_eq "defconfig 'is not set' overrides default" n "$(config_value "$TMP/.config" ALPHA)"

resolve --defconfig "$TMP/defconfig" --set CONFIG_BETA=n --set CONFIG_ALPHA=y \
    || fail "resolve with overrides"
assert_eq "--set overrides defconfig (to n)" n "$(config_value "$TMP/.config" BETA)"
assert_eq "--set overrides defconfig (to y)" y "$(config_value "$TMP/.config" ALPHA)"

grep -q '^CONFIG_ALPHA := y$' "$TMP/config.mk" && ok "config.mk contains y value" \
    || fail "config.mk contains y value"
grep -q '^CONFIG_BETA := n$' "$TMP/config.mk" && ok "config.mk contains n value" \
    || fail "config.mk contains n value"

# ---------------------------------------------------------------------------
# resolve: validation
# ---------------------------------------------------------------------------

expect_fail "unknown option in --set is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --set CONFIG_NOPE=y \
        --out-config "$TMP/x" --out-mk "$TMP/y"

echo "CONFIG_NOPE=y" >"$TMP/bad_defconfig"
expect_fail "unknown option in defconfig is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --defconfig "$TMP/bad_defconfig" \
        --out-config "$TMP/x" --out-mk "$TMP/y"

expect_fail "non-bool override value is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --set CONFIG_ALPHA=maybe \
        --out-config "$TMP/x" --out-mk "$TMP/y"

echo "CONFIG_ALPHA=true" >"$TMP/bad_defconfig2"
expect_fail "non-bool defconfig value is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --defconfig "$TMP/bad_defconfig2" \
        --out-config "$TMP/x" --out-mk "$TMP/y"

printf 'config DUP\n\tbool "a"\nconfig DUP\n\tbool "b"\n' >"$TMP/dup_kconfig"
expect_fail "duplicate declaration is rejected" \
    "$KC" resolve --kconfig "$TMP/dup_kconfig" \
        --out-config "$TMP/x" --out-mk "$TMP/y"

# depends on
expect_fail "unsatisfied depends is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --set CONFIG_GAMMA=y \
        --out-config "$TMP/x" --out-mk "$TMP/y"

resolve --set CONFIG_BETA=y --set CONFIG_GAMMA=y \
    && ok "satisfied depends is accepted" || fail "satisfied depends is accepted"

expect_fail "negated depends (!ALPHA with ALPHA=y) is rejected" \
    "$KC" resolve --kconfig "$TMP/Kconfig" --set CONFIG_BETA=y --set CONFIG_DELTA=y \
        --out-config "$TMP/x" --out-mk "$TMP/y"

resolve --defconfig "$TMP/defconfig" --set CONFIG_DELTA=y \
    && ok "negated depends (!ALPHA with ALPHA=n) is accepted" \
    || fail "negated depends (!ALPHA with ALPHA=n) is accepted"

# ---------------------------------------------------------------------------
# resolve: output mtime stability
# ---------------------------------------------------------------------------

resolve --defconfig "$TMP/defconfig" || fail "resolve for mtime test"
touch -t 200001010000 "$TMP/.config" "$TMP/config.mk"
resolve --defconfig "$TMP/defconfig" || fail "re-resolve for mtime test"
# Files must not have been rewritten (same config → mtime untouched)
if [ "$TMP/.config" -nt "$TMP/Kconfig" ]; then
    fail "unchanged .config is not rewritten"
else
    ok "unchanged .config is not rewritten"
fi

# ---------------------------------------------------------------------------
# gensystem
# ---------------------------------------------------------------------------

cat >"$TMP/template.system" <<'EOF'
<system>
    <always />
    <!-- @if CONFIG_BETA -->
    <beta-only />
    <!-- @if CONFIG_ALPHA -->
    <alpha-and-beta />
    <!-- @endif -->
    <!-- @endif -->
    <!-- @if !CONFIG_ALPHA -->
    <no-alpha />
    <!-- @endif -->
</system>
EOF

resolve --set CONFIG_BETA=y --set CONFIG_ALPHA=y || fail "resolve for gensystem"
"$KC" gensystem --config "$TMP/.config" --in "$TMP/template.system" --out "$TMP/out.system" \
    || fail "gensystem runs"
grep -q "<beta-only />" "$TMP/out.system" && ok "enabled block kept" || fail "enabled block kept"
grep -q "<alpha-and-beta />" "$TMP/out.system" && ok "nested enabled block kept" \
    || fail "nested enabled block kept"
grep -q "<no-alpha />" "$TMP/out.system" && fail "negated block stripped" || ok "negated block stripped"
grep -q "@if\|@endif" "$TMP/out.system" && fail "markers removed" || ok "markers removed"

resolve --set CONFIG_BETA=n --set CONFIG_ALPHA=n || fail "resolve for gensystem (off)"
"$KC" gensystem --config "$TMP/.config" --in "$TMP/template.system" --out "$TMP/out2.system" \
    || fail "gensystem runs (off)"
grep -q "beta-only\|alpha-and-beta" "$TMP/out2.system" && fail "disabled blocks stripped" \
    || ok "disabled blocks stripped"
grep -q "<no-alpha />" "$TMP/out2.system" && ok "negated block kept when option off" \
    || fail "negated block kept when option off"
grep -q "<always />" "$TMP/out2.system" && ok "unguarded content kept" || fail "unguarded content kept"

printf '<!-- @if CONFIG_MISSING -->\n<!-- @endif -->\n' >"$TMP/badref.system"
expect_fail "gensystem rejects unknown option in marker" \
    "$KC" gensystem --config "$TMP/.config" --in "$TMP/badref.system" --out "$TMP/x.system"

printf '<!-- @if CONFIG_ALPHA -->\n' >"$TMP/unbalanced.system"
expect_fail "gensystem rejects unterminated @if" \
    "$KC" gensystem --config "$TMP/.config" --in "$TMP/unbalanced.system" --out "$TMP/x.system"

printf '<!-- @endif -->\n' >"$TMP/stray.system"
expect_fail "gensystem rejects stray @endif" \
    "$KC" gensystem --config "$TMP/.config" --in "$TMP/stray.system" --out "$TMP/x.system"

# ---------------------------------------------------------------------------
# Real repository files: Kconfig, defconfigs, and .system templates
# ---------------------------------------------------------------------------

for defc in "$BS"/configs/*_defconfig; do
    name=$(basename "$defc")
    if "$KC" resolve --kconfig "$BS/Kconfig" --defconfig "$defc" \
        --out-config "$TMP/real.config" --out-mk "$TMP/real.mk" 2>"$TMP/err"; then
        ok "repo defconfig resolves: $name"
    else
        fail "repo defconfig resolves: $name ($(cat "$TMP/err"))"
    fi
done

for sys in "$ROOT/rpi4-photoframe/photoframe.system" \
           "$ROOT/rpi4-graphics/tvdemo-input.system" \
           "$ROOT/rpi4-graphics/tvdemo-network.system"; do
    name=$(basename "$sys")
    for usb in y n; do
        "$KC" resolve --kconfig "$BS/Kconfig" --set CONFIG_INPUT_USB_KEYBOARD=$usb \
            --out-config "$TMP/real.config" --out-mk "$TMP/real.mk" || fail "resolve usb=$usb"
        if "$KC" gensystem --config "$TMP/real.config" --in "$sys" --out "$TMP/real.system" \
            2>"$TMP/err"; then
            ok "gensystem processes $name (usb=$usb)"
        else
            fail "gensystem processes $name (usb=$usb) ($(cat "$TMP/err"))"
            continue
        fi
        usb_count=$(grep -c 'mr="usb_' "$TMP/real.system" || true)
        if [ "$usb" = y ]; then
            [ "$usb_count" -gt 0 ] && ok "$name maps USB when enabled" \
                || fail "$name maps USB when enabled"
        else
            [ "$usb_count" -eq 0 ] && ok "$name omits USB when disabled" \
                || fail "$name omits USB when disabled"
        fi
        grep -q "@if\|@endif" "$TMP/real.system" && fail "$name markers removed (usb=$usb)" \
            || ok "$name markers removed (usb=$usb)"
    done
done

# ---------------------------------------------------------------------------

echo ""
echo "test-kconfig: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
