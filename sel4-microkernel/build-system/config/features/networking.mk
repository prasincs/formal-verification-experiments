# Feature: Networking Support for seL4 Microkit
#
# This feature module adds networking capabilities to seL4 products.
# It supports both Ethernet (BCM54213PE) and WiFi (CYW43455) on Raspberry Pi 4.
#
# Usage:
#   make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=ethernet
#   make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=wifi
#   make PRODUCT=tvdemo PLATFORM=rpi4 NET_DRIVER=both
#
# Options:
#   NET_DRIVER   - ethernet, wifi, both, or none (default: none)
#   NET_STACK    - lwip or picotcp (default: lwip)

# Default values
NET_DRIVER ?= none
NET_STACK ?= lwip

# Validate NET_DRIVER option
ifneq ($(filter $(NET_DRIVER),none ethernet wifi both),$(NET_DRIVER))
$(error Invalid NET_DRIVER='$(NET_DRIVER)'. Valid options: none, ethernet, wifi, both)
endif

# Validate NET_STACK option
ifneq ($(filter $(NET_STACK),lwip picotcp),$(NET_STACK))
$(error Invalid NET_STACK='$(NET_STACK)'. Valid options: lwip, picotcp)
endif

# Skip networking setup if disabled
ifneq ($(NET_DRIVER),none)

# Enable networking flag
NETWORKING_ENABLED := 1

# Network PD source directory
NETWORK_PD_SRC_DIR := $(ROOT_DIR)/rpi4-network

# Network PD binary name
NETWORK_PD_NAME := network_pd
NETWORK_PD_ELF := $(BUILD_DIR)/$(NETWORK_PD_NAME).elf

# Cargo features based on driver selection
NETWORK_FEATURES :=

ifeq ($(NET_DRIVER),ethernet)
NETWORK_FEATURES += net-ethernet
endif

ifeq ($(NET_DRIVER),wifi)
NETWORK_FEATURES += net-wifi
endif

ifeq ($(NET_DRIVER),both)
NETWORK_FEATURES += net-ethernet net-wifi
endif

# Add IP stack feature
ifeq ($(NET_STACK),lwip)
NETWORK_FEATURES += net-stack-lwip
else ifeq ($(NET_STACK),picotcp)
NETWORK_FEATURES += net-stack-picotcp
endif

# Convert features to Cargo format
NETWORK_CARGO_FEATURES := $(if $(NETWORK_FEATURES),--features "$(NETWORK_FEATURES)")

# Hardware memory regions for Microkit system descriptor
# These are included when networking is enabled

# Ethernet (GENET) memory regions
ifeq ($(filter ethernet both,$(NET_DRIVER)),$(NET_DRIVER))
GENET_BASE := 0xfd580000
GENET_SIZE := 0x10000
endif

# WiFi (SDIO) memory regions
ifeq ($(filter wifi both,$(NET_DRIVER)),$(NET_DRIVER))
SDIO_BASE := 0xfe340000
SDIO_SIZE := 0x1000
SDHOST_BASE := 0xfe202000
SDHOST_SIZE := 0x100
endif

# Network PD source files for dependency tracking
NETWORK_SOURCES := $(wildcard $(NETWORK_PD_SRC_DIR)/src/*.rs) \
                   $(wildcard $(NETWORK_PD_SRC_DIR)/src/**/*.rs) \
                   $(NETWORK_PD_SRC_DIR)/Cargo.toml

# Build rule for Network PD
$(NETWORK_PD_ELF): $(NETWORK_SOURCES) | $(BUILD_DIR)
	@echo "=== Building Network PD ($(NET_DRIVER), $(NET_STACK)) ==="
	cd $(NETWORK_PD_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		$(NETWORK_CARGO_FEATURES) \
		$(CARGO_BUILD_STD)
	cp $(NETWORK_PD_SRC_DIR)/target/$(CARGO_TARGET)/release/$(NETWORK_PD_NAME).elf $@
	@echo "Built: $@"

# Add Network PD to build dependencies
.PHONY: build-network-pd
build-network-pd: $(NETWORK_PD_ELF)

# Print networking configuration
.PHONY: print-network-config
print-network-config:
	@echo "Networking Configuration:"
	@echo "  NET_DRIVER: $(NET_DRIVER)"
	@echo "  NET_STACK:  $(NET_STACK)"
	@echo "  Features:   $(NETWORK_FEATURES)"

endif # NET_DRIVER != none
