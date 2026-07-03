# Kconfig-style configuration integration
#
# Declarations live in $(BUILD_SYSTEM_DIR)/Kconfig; per-product defaults in
# configs/<product>_defconfig. Options can be overridden on the make command
# line, e.g.:
#
#   make PRODUCT=photoframe PLATFORM=rpi4 CONFIG_INPUT_USB_KEYBOARD=y sdcard
#
# Resolution happens at make parse time (kconfig-tool resolve) so the
# CONFIG_* values can steer rules and prerequisites. The resolved config is
# written to $(BUILD_DIR)/.config; kconfig-tool only rewrites it when the
# content changes, so it is safe as a rule prerequisite.
#
# Consumers wired up here:
#   - input_pd cargo features (uart/usb) from CONFIG_INPUT_*
#   - the product .system description is preprocessed with
#     `kconfig-tool gensystem`, which keeps or strips
#     <!-- @if CONFIG_X --> ... <!-- @endif --> blocks. Device MMIO is
#     therefore only mapped into a PD when the driver is compiled in.
#
# Products without a defconfig are untouched by this file.

KCONFIG_FILE := $(BUILD_SYSTEM_DIR)/Kconfig
KCONFIG_TOOL_DIR := $(BUILD_SYSTEM_DIR)/kconfig-tool
KCONFIG_TOOL_BIN := $(KCONFIG_TOOL_DIR)/target/release/kconfig-tool
DEFCONFIG ?= $(BUILD_SYSTEM_DIR)/configs/$(PRODUCT)_defconfig

ifneq ($(wildcard $(DEFCONFIG)),)

KCONFIG_ENABLED := 1
DOT_CONFIG := $(BUILD_DIR)/.config
CONFIG_MK := $(BUILD_DIR)/config.mk

# Collect CONFIG_* variables given on the make command line as overrides.
KCONFIG_OVERRIDES := $(foreach v,$(filter CONFIG_%,$(.VARIABLES)),\
	$(if $(filter command line,$(origin $(v))),--set $(v)=$($(v))))

# Build the tool lazily (cached no-op after the first build; the tool has
# its own rust-toolchain.toml pinning stable, independent of the repo-wide
# nightly pin) and resolve now, at parse time. Errors land on stderr; the
# OK sentinel distinguishes success from a silent failure inside $(shell).
KCONFIG_STATUS := $(shell mkdir -p $(BUILD_DIR) && \
	cargo build -q --release --manifest-path $(KCONFIG_TOOL_DIR)/Cargo.toml && \
	$(KCONFIG_TOOL_BIN) resolve \
		--kconfig $(KCONFIG_FILE) \
		--defconfig $(DEFCONFIG) \
		$(strip $(KCONFIG_OVERRIDES)) \
		--out-config $(DOT_CONFIG) \
		--out-mk $(CONFIG_MK) && echo OK)
ifneq ($(KCONFIG_STATUS),OK)
$(error kconfig: configuration resolution failed (see error above))
endif

include $(CONFIG_MK)

# --- Input PD cargo features ------------------------------------------------
# The Kconfig-driven build always passes an explicit feature set so the
# defconfig, not the crate's default features, decides what is compiled in.
ifdef INPUT_PD_ELF

KCONFIG_EMPTY :=
KCONFIG_SPACE := $(KCONFIG_EMPTY) $(KCONFIG_EMPTY)
KCONFIG_COMMA := ,

INPUT_PD_FEATURE_LIST :=
ifeq ($(CONFIG_INPUT_UART),y)
INPUT_PD_FEATURE_LIST += uart
endif
ifeq ($(CONFIG_INPUT_USB_KEYBOARD),y)
INPUT_PD_FEATURE_LIST += usb
endif

INPUT_PD_FEATURES := --no-default-features \
	$(if $(strip $(INPUT_PD_FEATURE_LIST)),\
		--features $(subst $(KCONFIG_SPACE),$(KCONFIG_COMMA),$(strip $(INPUT_PD_FEATURE_LIST))))

# Inject into the input_pd build recipe via a target-specific variable
# (same pattern networking.mk uses for the graphics PD features).
$(INPUT_PD_ELF): CARGO_BUILD_STD += $(INPUT_PD_FEATURES)

endif # INPUT_PD_ELF

# --- Configured system description -------------------------------------------
# Preprocess the product's .system template into the build directory,
# resolving @if CONFIG_X blocks, and point the Microkit rules at the result.
KCONFIG_SYSTEM_SRC := $(SYSTEM_DESC)
GEN_SYSTEM_DESC := $(BUILD_DIR)/$(notdir $(SYSTEM_DESC))

$(GEN_SYSTEM_DESC): $(KCONFIG_SYSTEM_SRC) $(DOT_CONFIG) | $(BUILD_DIR)
	@echo "=== Generating system description ($(notdir $(KCONFIG_SYSTEM_SRC))) ==="
	@cargo build -q --release --manifest-path $(KCONFIG_TOOL_DIR)/Cargo.toml
	$(KCONFIG_TOOL_BIN) gensystem --config $(DOT_CONFIG) --in $(KCONFIG_SYSTEM_SRC) --out $@

SYSTEM_DESC := $(GEN_SYSTEM_DESC)

endif # defconfig exists
