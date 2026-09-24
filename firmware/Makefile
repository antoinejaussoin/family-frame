CARGO ?= cargo
TARGET := thumbv8m.main-none-eabihf
HOST_TARGET ?= $(shell rustc -vV | awk '/^host:/{print $$2}')
BIN := family-frame
TARGET_DIR := $(or $(CARGO_TARGET_DIR),target)
ELF := $(TARGET_DIR)/$(TARGET)/release/$(BIN)
UF2 := $(BIN).uf2
OLED_BIN := family-frame-oled
OLED_ELF := $(TARGET_DIR)/$(TARGET)/release/$(OLED_BIN)
OLED_UF2 := $(OLED_BIN).uf2
FLASH := ./flash-pico.sh
HERE := $(CURDIR)

.PHONY: all build uf2 flash build-oled uf2-oled flash-oled test clean help

# Default: compile, write a Pico UF2, and copy it onto the bootloader drive.
all: flash

build:
	$(CARGO) build --release --target $(TARGET) --bin $(BIN)
	@echo
	@echo "Firmware ELF: $(ELF)"

uf2: build
	UF2="$(HERE)/$(UF2)" $(FLASH) --uf2-only $(ELF)

flash: build
	UF2="$(HERE)/$(UF2)" $(FLASH) $(ELF)

build-oled:
	$(CARGO) build --release --target $(TARGET) --features oled-debug --bin $(OLED_BIN)
	@echo
	@echo "OLED debug ELF: $(OLED_ELF)"

uf2-oled: build-oled
	UF2="$(HERE)/$(OLED_UF2)" $(FLASH) --uf2-only $(OLED_ELF)

oled: build-oled
	UF2="$(HERE)/$(OLED_UF2)" $(FLASH) $(OLED_ELF)

test:
	$(CARGO) test --target $(HOST_TARGET) --lib

clean:
	$(CARGO) clean
	rm -f $(UF2) $(OLED_UF2)

help:
	@echo "make            build, convert to UF2, and flash a Pico LiPo 2 XL W in BOOTSEL mode"
	@echo "make build      compile the production binary only"
	@echo "make uf2        compile and write $(UF2) without flashing"
	@echo "make flash-oled same as make, but the OLED debug binary ($(OLED_UF2))"
	@echo "make uf2-oled   compile and write $(OLED_UF2) without flashing"
	@echo "make test       host-side URL, settings, HTTP, and panel-map tests"
	@echo "make clean      remove build output"
