# Product: llmdemo (deterministic local inference with signed receipts)

# android-avf builds the identical qemu_virt_aarch64 board image and adds
# adb/crosvm/Termux deployment targets (docs/android-agent-os.md).
ifeq ($(filter qemu-aarch64 android-avf,$(PLATFORM)),)
$(error Product 'llmdemo' only supports platforms 'qemu-aarch64' and 'android-avf', not '$(PLATFORM)')
endif

PRODUCT_NAME := Verified Local Inference Demo
PRODUCT_SRC_DIR := $(ROOT_DIR)/rpi4-llm
PD_NAME := llmdemo_pd
PD_ELF := $(BUILD_DIR)/$(PD_NAME).elf
SYSTEM_DESC := $(PRODUCT_SRC_DIR)/llmdemo.system

$(PD_ELF): CARGO_BUILD_STD += --features pd

PRODUCT_SOURCES := $(wildcard $(PRODUCT_SRC_DIR)/src/*.rs) \
                   $(wildcard $(PRODUCT_SRC_DIR)/src/**/*.rs) \
                   $(PRODUCT_SRC_DIR)/Cargo.toml \
                   $(PRODUCT_SRC_DIR)/fixtures/tinystories-260k-f32.gguf \
                   $(wildcard $(ROOT_DIR)/rpi4-llm-loader/src/*.rs) \
                   $(ROOT_DIR)/rpi4-llm-loader/Cargo.toml

SYSTEM_IMAGE := $(BUILD_DIR)/loader.img
LOADER_ELF := $(BUILD_DIR)/loader.elf
REPORT := $(BUILD_DIR)/report.txt
