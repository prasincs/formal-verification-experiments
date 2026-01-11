# SD card image creation rules (RPi4 only)

# Only include for rpi4 platform
ifeq ($(PLATFORM),rpi4)

.PHONY: sdcard sdcard-uboot firmware bootfiles write-sdcard

# Download firmware
$(FIRMWARE_DIR):
	@echo "=== Downloading Raspberry Pi 4 Firmware ($(RPI_FIRMWARE_TAG)) ==="
	mkdir -p $@
	curl -L -o $@/start4.elf $(FIRMWARE_BASE_URL)/start4.elf
	curl -L -o $@/fixup4.dat $(FIRMWARE_BASE_URL)/fixup4.dat
	curl -L -o $@/bcm2711-rpi-4-b.dtb $(FIRMWARE_BASE_URL)/bcm2711-rpi-4-b.dtb
	@echo "Firmware downloaded to $@"

firmware: $(FIRMWARE_DIR)

# Create config.txt
$(BUILD_DIR)/config.txt: | $(BUILD_DIR)
	@echo "=== Creating config.txt ==="
	@echo "# seL4 Microkit on Raspberry Pi 4" > $@
	@echo "arm_64bit=1" >> $@
	@echo "kernel=loader.img" >> $@
	@echo "kernel_address=0x10000000" >> $@
	@echo "" >> $@
	@echo "# Display settings" >> $@
	@echo "hdmi_force_hotplug=1" >> $@
	@echo "hdmi_group=2" >> $@
	@echo "hdmi_mode=82" >> $@
	@echo "disable_overscan=1" >> $@
	@echo "" >> $@
	@echo "# GPU memory (minimum for framebuffer)" >> $@
	@echo "gpu_mem=64" >> $@
	@echo "" >> $@
	@echo "# Enable UART for debug output" >> $@
	@echo "enable_uart=1" >> $@
	@echo "uart_2ndstage=1" >> $@
	@echo "Created: $@"

# Create SD card image using mtools (no root required)
$(SDCARD_IMG): $(SYSTEM_IMAGE) $(FIRMWARE_DIR) $(BUILD_DIR)/config.txt
	@echo "=== Creating SD Card Image ==="
	$(SCRIPTS_DIR)/create-sdcard.sh \
		--loader $(SYSTEM_IMAGE) \
		--firmware $(FIRMWARE_DIR) \
		--config $(BUILD_DIR)/config.txt \
		--output $@ \
		--size $(SDCARD_SIZE_MB)
	@echo ""
	@echo "=== SD Card Image Ready ==="
	@echo "Image: $@"
	@echo ""
	@echo "Flash to SD card with:"
	@echo "  sudo dd if=$@ of=/dev/sdX bs=4M status=progress conv=fsync"

sdcard: $(SDCARD_IMG)
	@echo ""
	@echo "=== SD Card Image Ready ==="
	@echo "Product:  $(PRODUCT_NAME)"
	@echo "Platform: $(PLATFORM)"
	@echo "Image:    $(SDCARD_IMG)"
	@echo "Size:     $$(du -h $(SDCARD_IMG) | cut -f1)"
	@echo ""
	@echo "Flash with:"
	@echo "  sudo dd if=$(SDCARD_IMG) of=/dev/sdX bs=4M status=progress conv=fsync"
	@echo ""
	@echo "Or use: make PRODUCT=$(PRODUCT) PLATFORM=$(PLATFORM) write-sdcard DEVICE=/dev/sdX"

# Build U-Boot
$(UBOOT_BIN): | $(BUILD_DIR)
	@echo "=== Building U-Boot $(UBOOT_VERSION) for Raspberry Pi 4 ==="
	$(SCRIPTS_DIR)/build-uboot.sh \
		--source $(UBOOT_DIR) \
		--output $@ \
		--cross-compile $(CROSS_COMPILE) \
		--version $(UBOOT_VERSION)

.PHONY: uboot
uboot: $(UBOOT_BIN)

# Create SD card with U-Boot + bootelf support
sdcard-uboot: $(SYSTEM_IMAGE) $(LOADER_ELF) $(UBOOT_BIN) $(FIRMWARE_DIR) $(BUILD_DIR)/config.txt
	@echo "=== Creating SD Card Image with U-Boot ==="
	$(SCRIPTS_DIR)/create-sdcard.sh \
		--loader $(SYSTEM_IMAGE) \
		--loader-elf $(LOADER_ELF) \
		--firmware $(FIRMWARE_DIR) \
		--config $(BUILD_DIR)/config.txt \
		--uboot $(UBOOT_BIN) \
		--output $(SDCARD_IMG) \
		--size $(SDCARD_SIZE_MB)

# Create boot files directory (no SD card image)
bootfiles: $(SYSTEM_IMAGE) $(FIRMWARE_DIR) $(BUILD_DIR)/config.txt
	@echo "=== Creating Boot Files Directory ==="
	mkdir -p $(BUILD_DIR)/boot
	cp $(FIRMWARE_DIR)/start4.elf $(BUILD_DIR)/boot/
	cp $(FIRMWARE_DIR)/fixup4.dat $(BUILD_DIR)/boot/
	cp $(FIRMWARE_DIR)/bcm2711-rpi-4-b.dtb $(BUILD_DIR)/boot/
	cp $(BUILD_DIR)/config.txt $(BUILD_DIR)/boot/
	cp $(SYSTEM_IMAGE) $(BUILD_DIR)/boot/loader.img
	@echo ""
	@echo "Boot files created in: $(BUILD_DIR)/boot/"

# Write SD card image to device
# Usage: make PRODUCT=graphics PLATFORM=rpi4 write-sdcard DEVICE=/dev/sdb
write-sdcard: $(SDCARD_IMG)
ifndef DEVICE
	$(error DEVICE is required. Usage: make ... write-sdcard DEVICE=/dev/sdX)
endif
	@echo "=== Writing SD Card Image to $(DEVICE) ==="
	@echo "Image: $(SDCARD_IMG)"
	@echo ""
	@# Safety checks
	@if [ ! -b "$(DEVICE)" ]; then \
		echo "Error: $(DEVICE) is not a block device"; \
		exit 1; \
	fi
	@if mount | grep -q "$(DEVICE)"; then \
		echo "Error: $(DEVICE) or a partition is mounted. Unmount first."; \
		exit 1; \
	fi
	@echo "WARNING: This will ERASE all data on $(DEVICE)"
	@echo "Press Ctrl-C within 5 seconds to cancel..."
	@sleep 5
	sudo dd if=$(SDCARD_IMG) of=$(DEVICE) bs=4M status=progress conv=fsync
	@echo ""
	@echo "=== Write Complete ==="
	@echo "You can now insert the SD card into your Raspberry Pi 4."

endif # PLATFORM == rpi4
