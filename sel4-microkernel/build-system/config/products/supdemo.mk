# Product: supdemo (hierarchical child restart demo for QEMU)

ifneq ($(PLATFORM),qemu-aarch64)
$(error Product 'supdemo' only supports platform 'qemu-aarch64', not '$(PLATFORM)')
endif

PRODUCT_NAME := Hierarchical Supervisor Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-supervisor

# Build the worker through the generic Rust rule first. Its linked restart
# trampoline is then extracted by the trusted build and injected into the
# supervisor compilation; the child never supplies the restart PC at runtime.
PD_NAME := worker_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
SUPERVISOR_ELF := $(BUILD_DIR)/supervisor_pd.elf
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/supdemo.system

PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(PRODUCT_SRC_DIR)/build.rs

SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt

$(SUPERVISOR_ELF): $(PD_ELF) $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building supervisor_pd with trusted worker restart entry ==="
	@restart_entry=`aarch64-linux-gnu-nm -n $(PD_ELF) | awk '$$3 == "worker_restart_entry" { print "0x" $$1; exit }'`; \
		test -n "$$restart_entry"; \
		echo "Worker restart entry: $$restart_entry"; \
		cd $(PRODUCT_SRC_DIR) && WORKER_RESTART_ENTRY=$$restart_entry $(CARGO) build \
			--release \
			--target $(TARGET_SPEC) \
			--bin supervisor_pd \
			$(CARGO_BUILD_STD)
	cp $(PRODUCT_SRC_DIR)/target/$(CARGO_TARGET)/release/supervisor_pd.elf $@
	@echo "Built: $@"

$(SYSTEM_IMAGE): $(SUPERVISOR_ELF)
$(LOADER_ELF): $(SUPERVISOR_ELF)
