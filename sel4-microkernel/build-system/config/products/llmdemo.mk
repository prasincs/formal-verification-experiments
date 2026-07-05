# Product: llmdemo (deterministic local inference with signed receipts)

ifneq ($(PLATFORM),qemu-aarch64)
$(error Product 'llmdemo' only supports platform 'qemu-aarch64', not '$(PLATFORM)')
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
