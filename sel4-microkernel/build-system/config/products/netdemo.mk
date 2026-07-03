# Product: netdemo (Virtio Network Demo for QEMU)
#
# Supported platforms: qemu-aarch64
#
# Two-PD system exercising the network stack in QEMU for CI:
#   - network_pd:   virtio-net driver + shared-memory ring server
#   - netclient_pd: minimal client (ARP probe via the TX ring, logs RX)
#
#   make PRODUCT=netdemo PLATFORM=qemu-aarch64        - build loader.img
#   make PRODUCT=netdemo PLATFORM=qemu-aarch64 run    - boot in QEMU

# Validate platform
ifneq ($(PLATFORM),qemu-aarch64)
$(error Product 'netdemo' only supports platform 'qemu-aarch64', not '$(PLATFORM)')
endif

# Product info
PRODUCT_NAME := Virtio Network Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-network

# Primary protection domain (built by the generic rust.mk rule)
PD_NAME := network_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
NETCLIENT_ELF := $(BUILD_DIR)/netclient_pd.elf

# Enable the virtio-net driver for this product's cargo build.
# CARGO_BUILD_STD is simply-expanded, so append via a target-specific
# variable that the generic $(PD_ELF) recipe in include/rust.mk picks up.
$(PD_ELF): CARGO_BUILD_STD += --features net-virtio

# System descriptor
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/netdemo.system

# Source files for dependency tracking
PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-network-protocol/src/*.rs)

# Output files
SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

# Build the netclient PD (rust.mk builds only --bin $(PD_NAME), so the
# client binary needs its own cargo invocation; same features for cache
# reuse of the shared dependencies)
$(NETCLIENT_ELF): $(PRODUCT_SOURCES) $(PD_ELF) | $(BUILD_DIR)
	@echo "=== Building netclient_pd Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(PRODUCT_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		--bin netclient_pd \
		--features net-virtio \
		$(CARGO_BUILD_STD)
	cp $(PRODUCT_SRC_DIR)/target/$(CARGO_TARGET)/release/netclient_pd.elf $@
	@echo "Built: $@"

# System image needs both PDs in the search path
$(SYSTEM_IMAGE): $(NETCLIENT_ELF)
$(LOADER_ELF): $(NETCLIENT_ELF)

# Attach a virtio-net device backed by QEMU user networking (slirp) so the
# ARP probe to 10.0.2.2 gets answered
QEMU_EXTRA_ARGS += -device virtio-net-device,netdev=net0 -netdev user,id=net0
