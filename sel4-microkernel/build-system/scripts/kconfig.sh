#!/bin/sh
# kconfig.sh — minimal Kconfig-style configuration tool for the build system
#
# Subcommands:
#
#   resolve   --kconfig <Kconfig> --defconfig <file>
#             [--set CONFIG_NAME=y|n]...
#             --out-config <.config> --out-mk <config.mk>
#
#     Resolve option values in three layers (later wins): `default` lines
#     in the Kconfig declaration file, the defconfig, then --set overrides
#     (which come from CONFIG_*=y|n on the make command line). Validates
#     that every assigned option is declared, that values are y|n, and
#     that `depends on` constraints hold. Writes the canonical .config
#     and a make-includable config.mk; output files are only rewritten
#     when their content changes, so make dependencies stay stable.
#
#   gensystem --config <.config> --in <template.system> --out <file>
#
#     Copy a Microkit .system description, keeping or dropping blocks
#     guarded by XML-comment markers according to the configuration:
#
#         <!-- @if CONFIG_INPUT_USB_KEYBOARD -->
#         ...kept only when the option is y (markers always dropped)...
#         <!-- @endif -->
#
#     `@if !CONFIG_X` inverts the test; blocks nest. Referencing an
#     option that is not in .config is an error (catches typos), as is
#     an unbalanced @if/@endif.
#
# The Kconfig language subset understood by `resolve`:
#
#     config NAME            bool "prompt"        default y|n
#     depends on [!]A [&& [!]B ...]               help (text ignored)
#     menu "..." / endmenu / comment "..."        # comments
#
# Everything is POSIX sh + awk; no python, no kconfig-frontends.

set -eu

die() {
    echo "kconfig: error: $*" >&2
    exit 1
}

usage() {
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

# Write stdin to $1 only if the content differs (keeps mtimes stable).
write_if_changed() {
    _dst=$1
    _tmp=$(mktemp "${_dst}.XXXXXX")
    cat >"$_tmp"
    if [ -f "$_dst" ] && cmp -s "$_tmp" "$_dst"; then
        rm -f "$_tmp"
    else
        mv "$_tmp" "$_dst"
    fi
}

cmd_resolve() {
    kconfig= defconfig= out_config= out_mk= overrides=
    while [ $# -gt 0 ]; do
        case "$1" in
            --kconfig)    kconfig=$2;    shift 2 ;;
            --defconfig)  defconfig=$2;  shift 2 ;;
            --set)        overrides="$overrides $2"; shift 2 ;;
            --out-config) out_config=$2; shift 2 ;;
            --out-mk)     out_mk=$2;     shift 2 ;;
            *) die "resolve: unknown argument '$1'" ;;
        esac
    done
    [ -n "$kconfig" ] || die "resolve: --kconfig is required"
    [ -f "$kconfig" ] || die "resolve: Kconfig file not found: $kconfig"
    [ -n "$out_config" ] || die "resolve: --out-config is required"
    [ -n "$out_mk" ] || die "resolve: --out-mk is required"
    if [ -n "$defconfig" ] && [ ! -f "$defconfig" ]; then
        die "resolve: defconfig not found: $defconfig"
    fi

    # Resolve in awk. Output: "NAME value" per declared option, in
    # declaration order. Errors go to stderr with a non-zero exit.
    resolved=$(awk -v overrides="$overrides" -v defconfig_name="${defconfig:-<none>}" '
        function fail(msg) {
            printf "kconfig: error: %s\n", msg | "cat >&2"
            close("cat >&2")
            exit 1
        }

        # --- pass 1: Kconfig declarations ---
        FNR == NR {
            if ($0 ~ /^config[ \t]+[A-Za-z0-9_]+[ \t]*$/) {
                name = $2
                if (name in declared) fail("duplicate option " name " in Kconfig")
                declared[name] = 1
                order[++nopts] = name
                value[name] = "n"      # options default to n unless said otherwise
                current = name
                next
            }
            if ($0 ~ /^[ \t]+default[ \t]+/) {
                if (current == "") fail("Kconfig: default outside config block")
                v = $2
                if (v != "y" && v != "n") fail("Kconfig: option " current " has non-bool default \x27" v "\x27")
                value[current] = v
                next
            }
            if ($0 ~ /^[ \t]+depends on[ \t]+/) {
                if (current == "") fail("Kconfig: depends outside config block")
                expr = $0
                sub(/^[ \t]+depends on[ \t]+/, "", expr)
                depends[current] = expr
                next
            }
            # bool/help/menu/comment lines carry no resolution semantics
            next
        }

        # --- pass 2: defconfig assignments ---
        {
            line = $0
            sub(/\r$/, "", line)
            if (line ~ /^#[ \t]*CONFIG_[A-Za-z0-9_]+[ \t]+is not set[ \t]*$/) {
                n = line
                sub(/^#[ \t]*CONFIG_/, "", n)
                sub(/[ \t]+is not set.*$/, "", n)
                assign(n, "n", defconfig_name)
                next
            }
            if (line ~ /^[ \t]*(#|$)/) next
            if (line ~ /^CONFIG_[A-Za-z0-9_]+=/) {
                n = line; sub(/^CONFIG_/, "", n); sub(/=.*$/, "", n)
                v = line; sub(/^[^=]*=/, "", v)
                assign(n, v, defconfig_name)
                next
            }
            fail(defconfig_name ": unrecognized line: " line)
        }

        function assign(n, v, src) {
            if (!(n in declared)) fail(src ": unknown option CONFIG_" n " (not declared in Kconfig)")
            if (v != "y" && v != "n") fail(src ": CONFIG_" n " must be y or n, got \x27" v "\x27")
            value[n] = v
        }

        END {
            # --- layer 3: command-line overrides ---
            no = split(overrides, ov, /[ \t]+/)
            for (i = 1; i <= no; i++) {
                if (ov[i] == "") continue
                if (ov[i] !~ /^CONFIG_[A-Za-z0-9_]+=(y|n)$/)
                    fail("override \x27" ov[i] "\x27 is not of the form CONFIG_NAME=y|n")
                n = ov[i]; sub(/^CONFIG_/, "", n); sub(/=.*$/, "", n)
                v = ov[i]; sub(/^[^=]*=/, "", v)
                assign(n, v, "command line")
            }

            # --- validate depends-on constraints ---
            for (i = 1; i <= nopts; i++) {
                n = order[i]
                if (value[n] != "y" || !(n in depends)) continue
                nterm = split(depends[n], terms, /&&/)
                for (t = 1; t <= nterm; t++) {
                    term = terms[t]
                    gsub(/^[ \t]+|[ \t]+$/, "", term)
                    neg = sub(/^!/, "", term)
                    if (!(term in declared))
                        fail("Kconfig: CONFIG_" n " depends on undeclared option " term)
                    sat = (value[term] == "y")
                    if (neg) sat = !sat
                    if (!sat)
                        fail("CONFIG_" n "=y requires " (neg ? "!" : "") "CONFIG_" term \
                             " (currently CONFIG_" term "=" value[term] ")")
                }
            }

            for (i = 1; i <= nopts; i++) print order[i], value[order[i]]
        }
    ' "$kconfig" ${defconfig:+"$defconfig"}) || exit 1

    printf '%s\n' "# Automatically generated by kconfig.sh; do not edit.
# Layers: Kconfig defaults <- ${defconfig:-<no defconfig>} <- command line
$(printf '%s\n' "$resolved" | while read -r name val; do
        if [ "$val" = y ]; then
            printf 'CONFIG_%s=y\n' "$name"
        else
            printf '# CONFIG_%s is not set\n' "$name"
        fi
    done)" | write_if_changed "$out_config"

    printf '%s\n' "# Automatically generated by kconfig.sh; do not edit.
$(printf '%s\n' "$resolved" | while read -r name val; do
        printf 'CONFIG_%s := %s\n' "$name" "$val"
    done)" | write_if_changed "$out_mk"
}

cmd_gensystem() {
    config= infile= outfile=
    while [ $# -gt 0 ]; do
        case "$1" in
            --config) config=$2;  shift 2 ;;
            --in)     infile=$2;  shift 2 ;;
            --out)    outfile=$2; shift 2 ;;
            *) die "gensystem: unknown argument '$1'" ;;
        esac
    done
    [ -f "${config:-}" ] || die "gensystem: --config file not found: ${config:-<unset>}"
    [ -f "${infile:-}" ] || die "gensystem: --in file not found: ${infile:-<unset>}"
    [ -n "${outfile:-}" ] || die "gensystem: --out is required"

    awk '
        function fail(msg) {
            printf "kconfig: error: %s\n", msg | "cat >&2"
            close("cat >&2")
            exit 1
        }

        # pass 1: read .config values
        FNR == NR {
            if ($0 ~ /^CONFIG_[A-Za-z0-9_]+=y[ \t]*$/) {
                n = $0; sub(/^CONFIG_/, "", n); sub(/=.*$/, "", n)
                cfg[n] = "y"
            } else if ($0 ~ /^#[ \t]*CONFIG_[A-Za-z0-9_]+[ \t]+is not set/) {
                n = $0; sub(/^#[ \t]*CONFIG_/, "", n); sub(/[ \t]+is not set.*$/, "", n)
                cfg[n] = "n"
            }
            next
        }

        # pass 2: template
        {
            if (match($0, /<!--[ \t]*@if[ \t]+!?CONFIG_[A-Za-z0-9_]+[ \t]*-->/)) {
                cond = substr($0, RSTART, RLENGTH)
                sub(/<!--[ \t]*@if[ \t]+/, "", cond)
                sub(/[ \t]*-->/, "", cond)
                neg = sub(/^!/, "", cond)
                sub(/^CONFIG_/, "", cond)
                if (!(cond in cfg))
                    fail(FILENAME ":" FNR ": @if references CONFIG_" cond ", which is not in the .config")
                depth++
                if (!suppress) {
                    sat = (cfg[cond] == "y")
                    if (neg) sat = !sat
                    if (!sat) suppress = depth
                }
                next   # marker line is never emitted
            }
            if ($0 ~ /<!--[ \t]*@endif([ \t]|-->)/) {
                if (depth == 0)
                    fail(FILENAME ":" FNR ": @endif without matching @if")
                if (suppress == depth) suppress = 0
                depth--
                next
            }
            if (!suppress) print
        }

        END {
            if (depth != 0) fail(FILENAME ": unterminated @if block (missing @endif)")
        }
    ' "$config" "$infile" >"${outfile}.tmp" || { rm -f "${outfile}.tmp"; exit 1; }
    mv "${outfile}.tmp" "$outfile"
}

[ $# -ge 1 ] || usage
cmd=$1
shift
case "$cmd" in
    resolve)   cmd_resolve "$@" ;;
    gensystem) cmd_gensystem "$@" ;;
    -h|--help|help) usage ;;
    *) die "unknown subcommand '$cmd' (expected: resolve, gensystem)" ;;
esac
