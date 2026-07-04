# Product: supdemo (hierarchical child restart demo for QEMU)

ifneq ($(PLATFORM),qemu-aarch64)
$(error Product 'supdemo' only supports platform 'qemu-aarch64', not '$(PLATFORM)')
endif

PRODUCT_NAME := Hierarchical Supervisor Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-supervisor

PD_NAME := supervisor_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
WORKER_ELF := $(BUILD_DIR)/worker_pd.elf
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/supdemo.system

PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml

SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

$(WORKER_ELF): $(PRODUCT_SOURCES) $(PD_ELF) | $(BUILD_DIR)
	@echo "=== Building worker_pd Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(PRODUCT_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		--bin worker_pd \
		$(CARGO_BUILD_STD)
	cp $(PRODUCT_SRC_DIR)/target/$(CARGO_TARGET)/release/worker_pd.elf $@
	@echo "Built: $@"

$(SYSTEM_IMAGE): $(WORKER_ELF)
$(LOADER_ELF): $(WORKER_ELF)
