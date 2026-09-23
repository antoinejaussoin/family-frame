CARGO ?= cargo
TARGET := thumbv8m.main-none-eabihf
BIN := firmware-test
TARGET_DIR := $(or $(CARGO_TARGET_DIR),target)
ELF := $(TARGET_DIR)/$(TARGET)/release/$(BIN)
UF2 := $(BIN).uf2
FLASH := ../../firmware/flash-pico.sh
HERE := $(CURDIR)

.PHONY: all build flash clean

# Default: compile, write a Pico UF2, and copy it onto the bootloader drive.
all: flash

build:
	$(CARGO) build --release --target $(TARGET) --bin $(BIN)

flash: build
	UF2="$(HERE)/$(UF2)" $(FLASH) $(ELF)

clean:
	$(CARGO) clean
	rm -f $(UF2)
