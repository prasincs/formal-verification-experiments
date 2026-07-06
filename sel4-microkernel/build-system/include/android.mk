# Android deployment rules (AVF / crosvm and Termux / QEMU)
#
# Only included when the platform sets IS_ANDROID (android-avf).
# Host QEMU parity for the same image comes from include/qemu.mk,
# which android-avf also enables via IS_QEMU.

ifeq ($(IS_ANDROID),true)

ANDROID_SCRIPTS := $(SCRIPTS_DIR)/android

# adb invocation, honoring an explicit device serial
ADB_CMD := $(ADB)
ifneq ($(ANDROID_SERIAL),)
ADB_CMD += -s $(ANDROID_SERIAL)
endif

.PHONY: deploy-avf run-avf termux-bundle

# Stage the system image and on-device launcher over adb
deploy-avf: $(SYSTEM_IMAGE)
	@echo "=== Deploying $(PRODUCT) to Android device (AVF) ==="
	ADB="$(ADB)" ANDROID_SERIAL="$(ANDROID_SERIAL)" \
	CROSVM_BIN="$(CROSVM_BIN)" \
	$(ANDROID_SCRIPTS)/deploy-avf.sh \
		--image $(SYSTEM_IMAGE) \
		--stage-dir $(ANDROID_STAGE_DIR)

# Deploy, then boot under crosvm with serial routed to this terminal.
# Requires a rooted or userdebug device; see docs/android-agent-os.md
# for the current crosvm bring-up status before expecting guest output.
run-avf: deploy-avf
	@echo "=== Booting under crosvm (serial -> this terminal) ==="
	$(ADB_CMD) shell MEM_MB=$(AVF_MEMORY_MB) CPUS=$(AVF_CPUS) \
		CROSVM="$(CROSVM_BIN)" \
		sh $(ANDROID_STAGE_DIR)/run-crosvm.sh

# Tarball with the image and a Termux QEMU launcher, for the
# software-emulation loop on the device (no root required):
#   adb push $(TERMUX_BUNDLE) /sdcard/Download/
#   (in Termux) tar xf ... && sh run-termux-qemu.sh
TERMUX_BUNDLE := $(BUILD_DIR)/termux-$(PRODUCT).tar.gz
termux-bundle: $(TERMUX_BUNDLE)

$(TERMUX_BUNDLE): $(SYSTEM_IMAGE) $(ANDROID_SCRIPTS)/run-termux-qemu.sh
	@echo "=== Creating Termux bundle ==="
	tar -czf $@ \
		-C $(BUILD_DIR) $(notdir $(SYSTEM_IMAGE)) \
		-C $(ANDROID_SCRIPTS) run-termux-qemu.sh
	@echo ""
	@echo "Bundle: $@"
	@echo "  adb push $@ /sdcard/Download/"
	@echo "  (Termux) pkg install qemu-system-aarch64-headless"
	@echo "  (Termux) tar xzf /sdcard/Download/$(notdir $@) && sh run-termux-qemu.sh"

endif # IS_ANDROID
