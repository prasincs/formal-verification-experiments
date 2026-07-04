# Product: ipdemo (smoltcp DHCP + ICMP demo for QEMU)

ifneq ($(PLATFORM),qemu-aarch64)
$(error Product 'ipdemo' only supports platform 'qemu-aarch64', not '$(PLATFORM)')
endif

PRODUCT_NAME := smoltcp IP Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-network
PD_NAME := ipdemo_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/ipdemo.system

$(PD_ELF): CARGO_BUILD_STD += --features "net-virtio net-stack-smoltcp qemu-time-fallback"

PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(wildcard $(ROOT_DIR)/rpi4-network-protocol/src/*.rs)

SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

QEMU_EXTRA_ARGS += -device virtio-net-device,netdev=net0 -netdev user,id=net0
