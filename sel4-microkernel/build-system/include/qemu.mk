# QEMU run rules

# Only include if IS_QEMU is true
ifeq ($(IS_QEMU),true)

.PHONY: run run-debug

# Run in QEMU
run: $(SYSTEM_IMAGE)
	@echo "=== Booting seL4 Microkit in QEMU ($(PLATFORM)) ==="
	@echo "Press Ctrl-A X to exit"
	@echo ""
ifeq ($(PLATFORM_ARCH),aarch64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m $(QEMU_MEMORY) \
		-nographic \
		$(QEMU_EXTRA_ARGS) \
		-device loader,file=$<,addr=$(QEMU_LOADER_ADDR),cpu-num=0
else ifeq ($(PLATFORM_ARCH),riscv64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m $(QEMU_MEMORY) \
		-nographic \
		$(QEMU_EXTRA_ARGS) \
		-kernel $<
endif

# Run with GDB server
run-debug: $(SYSTEM_IMAGE)
	@echo "=== Booting seL4 Microkit in QEMU with GDB ($(PLATFORM)) ==="
	@echo "Connect GDB with: target remote localhost:1234"
	@echo "Press Ctrl-A X to exit"
	@echo ""
ifeq ($(PLATFORM_ARCH),aarch64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m $(QEMU_MEMORY) \
		-nographic \
		-s -S \
		$(QEMU_EXTRA_ARGS) \
		-device loader,file=$<,addr=$(QEMU_LOADER_ADDR),cpu-num=0
else ifeq ($(PLATFORM_ARCH),riscv64)
	$(QEMU) \
		-machine $(QEMU_MACHINE) \
		-cpu $(QEMU_CPU) \
		-m $(QEMU_MEMORY) \
		-nographic \
		-s -S \
		$(QEMU_EXTRA_ARGS) \
		-kernel $<
endif

endif # IS_QEMU
