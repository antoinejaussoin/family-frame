CARGO ?= cargo
TARGET := thumbv8m.main-none-eabihf
HOST_TARGET ?= $(shell rustc -vV | awk '/^host:/{print $$2}')
BIN := family-frame
ELF := target/$(TARGET)/release/$(BIN)
UF2 := $(BIN).uf2
FLASH := ./flash-pico.sh

.PHONY: all build uf2 flash test clean help

# Default: compile, write a Pico UF2, and copy it onto the bootloader drive.
all: flash

build:
	$(CARGO) build --release --target $(TARGET)
	@echo
	@echo "Firmware ELF: $(ELF)"

uf2: build
	$(FLASH) --uf2-only $(ELF)

flash: build
	$(FLASH) $(ELF)

test:
	$(CARGO) test --target $(HOST_TARGET) --lib

clean:
	$(CARGO) clean
	rm -f $(UF2)

help:
	@echo "make        build, convert to UF2, and flash a Pico LiPo 2 XL W in BOOTSEL mode"
	@echo "make build  compile only"
	@echo "make uf2    compile and write $(UF2) without flashing"
	@echo "make test   host-side URL, settings, HTTP, and panel-map tests"
	@echo "make clean  remove build output"
