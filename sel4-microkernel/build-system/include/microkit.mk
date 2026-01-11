# Microkit tool rules

# SDK paths
SDK_BOARD := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)
SDK_LIB := $(SDK_BOARD)/lib
SDK_INCLUDE := $(SDK_BOARD)/include

# Build system image (binary format, used for direct boot)
$(SYSTEM_IMAGE): $(PD_ELF) $(SYSTEM_DESC) | check-sdk
	@echo "=== Building Microkit System Image ==="
	$(MICROKIT_TOOL) \
		$(SYSTEM_DESC) \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		-o $@ \
		-r $(REPORT)
	@echo ""
	@echo "Build complete!"
	@echo "  System image: $@"
	@echo "  Report: $(REPORT)"

# Build loader ELF (for bootelf command)
$(LOADER_ELF): $(PD_ELF) $(SYSTEM_DESC) | check-sdk
	@echo "=== Building Microkit Loader ELF ==="
	$(MICROKIT_TOOL) \
		$(SYSTEM_DESC) \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		--image-type elf \
		-o $@
	@echo "Built: $@"

# Check SDK prerequisites
.PHONY: check-sdk
check-sdk:
	@if [ ! -d "$(MICROKIT_SDK)" ]; then \
		echo "Error: Microkit SDK not found at $(MICROKIT_SDK)"; \
		echo "Run: make setup-sdk"; \
		exit 1; \
	fi
	@if [ ! -d "$(SDK_BOARD)" ]; then \
		echo "Error: Board $(MICROKIT_BOARD) not found in SDK"; \
		echo "Available boards:"; \
		ls $(MICROKIT_SDK)/board/ 2>/dev/null || echo "  (none)"; \
		exit 1; \
	fi

# Download and setup Microkit SDK
.PHONY: setup-sdk
setup-sdk:
	@echo "=== Setting up Microkit SDK $(MICROKIT_VERSION) ==="
	$(SCRIPTS_DIR)/download-sdk.sh \
		--version $(MICROKIT_VERSION) \
		--sha256 $(MICROKIT_SDK_SHA256) \
		--output $(MICROKIT_SDK)
