# Rust/Cargo build rules

# Environment for cargo
export SEL4_INCLUDE_DIRS := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)/include

# Rust target directory (uses the project's target directory by default)
CARGO_TARGET_DIR := $(PRODUCT_SRC_DIR)/target

# Build protection domain ELF
# PRODUCT_SRC_DIR and PD_NAME must be set by product config
$(PD_ELF): $(PRODUCT_SOURCES) | $(BUILD_DIR)
	@echo "=== Building $(PD_NAME) Protection Domain ($(PLATFORM_ARCH)) ==="
	cd $(PRODUCT_SRC_DIR) && $(CARGO) build \
		--release \
		--target $(TARGET_SPEC) \
		$(CARGO_BUILD_STD)
	@mkdir -p $(BUILD_DIR)
	cp $(CARGO_TARGET_DIR)/$(CARGO_TARGET)/release/$(PD_NAME).elf $@
	@echo "Built: $@"

# Create build directory
$(BUILD_DIR):
	mkdir -p $@

# Clean Rust build artifacts
.PHONY: clean-rust
clean-rust:
	cd $(PRODUCT_SRC_DIR) && cargo clean
