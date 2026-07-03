# Kconfig-Style Build Configuration

## Summary

The build system has a Linux-Kconfig-inspired configuration layer for
feature options. Options are declared once in `build-system/Kconfig`, get
per-product defaults from `build-system/configs/<product>_defconfig`, and can
be overridden per build on the make command line:

```bash
# Photoframe ships serial-input-only; opt into the USB keyboard for one build:
make PRODUCT=photoframe PLATFORM=rpi4 CONFIG_INPUT_USB_KEYBOARD=y sdcard

# tvdemo enables USB by default; strip it back out:
make PRODUCT=tvdemo PLATFORM=rpi4 ISOLATED=1 CONFIG_INPUT_USB_KEYBOARD=n sdcard

# Inspect the resolved configuration:
make PRODUCT=photoframe PLATFORM=rpi4 info
```

A resolved option steers **two** artifacts, and this pairing is the point of
the system:

1. **What code is compiled** — `CONFIG_INPUT_*` options map to cargo features
   of `rpi4-input-pd` (`uart`, `usb`), passed as
   `--no-default-features --features ...`.
2. **What memory the seL4 kernel lets that code touch** — the product's
   Microkit `.system` description is preprocessed, and device mappings guarded
   by `<!-- @if CONFIG_X -->` markers are stripped unless the option is
   enabled.

Because both derive from the same `.config`, a driver is never compiled in
without its MMIO mapped (which would fault), and — more importantly for this
project's security story — **a PD is never granted device MMIO for a driver
that is not compiled in**. Turning `CONFIG_INPUT_USB_KEYBOARD` off does not
just remove code; it removes the Input PD's *capability* to touch the USB
controller at all.

## The pieces

| Piece | Path | Role |
|-------|------|------|
| Declarations | `build-system/Kconfig` | Every `CONFIG_*` option: type, default, dependencies, help |
| Product defaults | `build-system/configs/<product>_defconfig` | Kconfig-idiom assignments (`CONFIG_X=y`, `# CONFIG_X is not set`) |
| Resolver | `build-system/kconfig-tool resolve` | Layers defaults ← defconfig ← command line; validates; writes `.config` + `config.mk` |
| System preprocessor | `build-system/kconfig-tool gensystem` | Copies a `.system` template, keeping/stripping `@if` blocks |
| Make glue | `build-system/config/kconfig.mk` | Builds the tool, runs the resolver at parse time, maps options to cargo features, swaps `SYSTEM_DESC` for the generated file |
| Self-test | `build-system/kconfig-tool` (`cargo test`) | Unit tests over the resolver/preprocessor logic plus end-to-end CLI tests against the real repo Kconfig/defconfigs/.system files; run by the `kconfig` CI job |

`kconfig-tool` is a small std-only Rust host crate (`build-system/kconfig-tool/`,
zero dependencies) — no python, no `kconfig-frontends` dependency. It builds
with stable Rust via its own `rust-toolchain.toml`, independent of the
nightly pin the seL4 Microkit crates use, and `config/kconfig.mk` builds it
lazily inside `$(shell ...)` at make parse time (a `cargo build -q --release`
no-op after the first build).

## Declaration language

A deliberate subset of the Linux Kconfig language:

```
config INPUT_USB_KEYBOARD
	bool "USB HID keyboard (DWC2)"
	default n
	depends on SOME_OPTION && !OTHER_OPTION
	help
	  Free-form help text.
```

Only `bool` options exist today. `depends on` accepts `&&`-conjunctions of
(optionally `!`-negated) option names and is enforced at resolve time: setting
an option whose dependencies are unsatisfied is a hard error with a message
naming the missing option, not a silent auto-disable. `menu`/`endmenu` and
`comment` are accepted for grouping and otherwise ignored.

## Resolution layers

`kconfig-tool resolve` computes each option's value from three layers, later
wins:

1. `default y|n` in `Kconfig`
2. the product defconfig (`CONFIG_X=y` / `# CONFIG_X is not set`)
3. `--set CONFIG_X=y|n` arguments, which `config/kconfig.mk` collects from
   `CONFIG_*` variables given on the make command line

Unknown option names, non-boolean values, duplicate declarations, and
dependency violations are all rejected. The outputs are:

- `$(BUILD_DIR)/.config` — canonical resolved config, Kconfig idiom
- `$(BUILD_DIR)/config.mk` — the same values as `CONFIG_X := y|n` make
  variables, included into the build

Both files are only rewritten when their content changes, so make rules that
depend on `.config` (the generated `.system`) rebuild exactly when the
configuration actually changed.

## Conditional system descriptions

Microkit `.system` files are XML, so the guard markers are XML comments and
the checked-in templates remain valid, buildable system descriptions on their
own:

```xml
<!-- @if CONFIG_INPUT_USB_KEYBOARD -->
<map mr="usb_regs" vaddr="0x5_0500_0000" perms="rw" cached="false" />
<map mr="usb_dma" vaddr="0x5_0600_0000" perms="rw" cached="false" />
<!-- @endif -->
```

`kconfig-tool gensystem` copies the template into `$(BUILD_DIR)`, dropping the
marker lines and, when the option is `n`, everything between them. Blocks
nest, and `@if !CONFIG_X` inverts the test. Referencing an option that is not
in the `.config` is an error, as is an unbalanced `@if`/`@endif` — typos fail
the build rather than silently granting or withholding a mapping.

The build then points the Microkit tool at the generated file; the source
template is never consumed directly by a configured build.

## Current options

| Option | Default | tvdemo | photoframe | Effect |
|--------|---------|--------|------------|--------|
| `CONFIG_INPUT_UART` | y | y | y | `uart` feature in `input_pd`: mini-UART serial input |
| `CONFIG_INPUT_USB_KEYBOARD` | n | y | n | `usb` feature in `input_pd` + DWC2 MMIO/DMA mappings: USB HID keyboard (see `usb-keyboard-input.md`) |

Products without a defconfig (`hello`, `graphics`, `netdemo`, `tpmtest`) are
untouched by the configuration layer.

## Adding an option

1. Declare it in `build-system/Kconfig` (prompt, default, `depends on`,
   help).
2. Set it in the defconfigs where the default should differ.
3. Consume it: map it to a cargo feature and/or guard `.system` blocks in
   `config/kconfig.mk` (see the `CONFIG_INPUT_*` handling as the template).
4. `build-system/kconfig-tool/tests/cli.rs` picks up new defconfigs and the
   guarded `.system` templates automatically (it globs `configs/*_defconfig`
   and lists the known `.system` files); extend it if the option has
   interesting invariants.

The existing `NET_DRIVER`/`NET_STACK`/`ISOLATED` make variables predate this
system and still work; migrating them to Kconfig options is a natural
follow-up but was left alone to avoid churning the networking CI.
