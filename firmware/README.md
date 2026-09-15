# Pico firmware (dumb client)

Embassy / Rust client for a Pimoroni **Pico LiPo 2 XL W** and Inky
Impression 13.3″. Family data and HTML stay on the LAN server. This
binary:

1. Joins 2.4 GHz Wi-Fi (SSID and password from USB, not compiled in).
2. `POST /frame.bin` with battery diagnostics and `If-None-Match`.
3. **204** → leave the glass alone.
4. **200** → `show_frame()` the 960 000-byte body, store the checksum.
5. Powers the switched-core down for `sleep` seconds (default 3600) and repeats.

Wi-Fi / server provisioning is the same USB CDC CLI as the laser-tag
temperature-display and IR-capture nodes: type `wifi`, `psk`, `server`,
`save`. Settings live in the last 4 KiB flash sector.

The panel driver is a Rust port of
[el133-pico-driver](https://github.com/dmellok/el133-pico-driver) /
[tesserae-device-pico-bin](https://github.com/dmellok/tesserae-device-pico-bin).
See [`PROTOCOL.md`](PROTOCOL.md) and [`NOTICES.md`](NOTICES.md).

## Configure (USB serial)

Flash the UF2, then open the CDC port (no baud rate matters):

```bash
screen /dev/cu.usbmodem* 115200
```

```
wifi MySsid
psk MyPassword
server 192.168.0.251:8765
sleep 3600
save
```

| Command | Meaning |
|---|---|
| `wifi <ssid>` | 2.4 GHz SSID (max 32) |
| `psk <password>` | WPA2 PSK, or empty for an open network |
| `server <host:port>` | Family-frame HTTP origin. `POST /frame.bin` is appended |
| `sleep <seconds>` | Interval between polls. `0` = poll every 60 s. POWMAN-dormant between polls |
| `save` | Write the last flash sector and join Wi-Fi |
| `show` | SSID, URL, sleep, checksum, Wi-Fi / frame status, battery, wake reason |
| `forget` | Drop the last checksum so the next poll paints |
| `clear` | Erase saved settings |
| `help` | Command list |

Nothing is compiled in. `save` is required after `wifi` / `psk` /
`server` / `sleep`. Until the node is ready, `show` prints
`USB: wifi/save` and the panel paints yellow once.

## Build

Rust stable with the `thumbv8m.main-none-eabihf` target (see
`rust-toolchain.toml`). Same Embassy crate versions as the laser-tag
Pico 2 W nodes, with `embassy-rp` on **`rp235xb`** (48 GPIO, GP47 PSRAM).

```bash
cd firmware
make test    # host check of /frame.bin URL shaping
make build
make uf2     # writes family-frame.uf2
make uf2-oled  # writes family-frame-oled.uf2 (SSD1306/SH1106 status)
```

## Flash

Hold **BOOTSEL** (or the board’s BOOT button) while plugging USB-C, or
double-tap reset. The drive is `RP2350`.

```bash
make flash
# or: cp family-frame.uf2 /Volumes/RP2350/
```

No e-ink ribbon yet: flash the OLED debug variant instead (`make flash-oled`). Same Wi-Fi / HTTP / PSRAM / e-ink code, plus a 0.96″ status panel. See **OLED debug** below.

## What you should see

After `save`, `show` reports `wifi ok` then `frame 200` or `frame 204`.
A later poll with an unchanged dashboard skips the 35 s refresh.

Cold-boot diagnostics (one full refresh):

| Colour | Meaning |
|---|---|
| Yellow | No Wi-Fi / server saved yet |
| Red | No PSRAM, or Wi-Fi failed |
| Blue | HTTP failed and no previous frame |

## Pins

Hard Stuff Pico-to-Pi HAT **H**, USB-end 20 pins of the XL W. The HAT
swaps SCLK and MOSI — meter it, do not trust the vendor PDF.

| Signal | GP |
|---|---|
| SCLK | 10 |
| MOSI | 11 |
| DC | 22 |
| RST | 27 |
| BUSY | 17 (active low) |
| CS_M (left, cols 0–599) | 26 |
| CS_S (right, cols 600–1199) | 16 |
| OLED SDA (`family-frame-oled` only) | 18 |
| OLED SCL (`family-frame-oled` only) | 19 |

The **user LED** (next to USB-C, RM2 `WL_GPIO0`) is on only while a USB
host is actually talking (SOF frames). Unplugging the cable turns it off
within ~100 ms even if the board was previously enumerated. The white
**power LED** is hardwired to 3V3 — firmware cannot switch it. Cut the
rear LED-symbol trace for weeks of sleep. Solder **`+1A Mode`** only if a
refresh browns out and the cell can deliver it.

## OLED debug

A second binary, `family-frame-oled`, is the same client (Wi-Fi, PSRAM,
`POST /frame.bin`, Inky driver still compiled in) plus a 0.96″ I²C status
panel. Use it until the e-ink ribbon arrives.

The laser-tag temperature-display node used **GP16 / GP17**. Do **not**
do that here: those pins are Inky `CS_S` and `BUSY`. Use I²C1 on **GP18 /
GP19** on the **right** side (USB-end headers you already soldered).

| OLED pin | XL W | Physical pin (USB-C at the top) |
|---|---|---:|
| `VCC` | `3V3` | 36 (fifth down the **right** side) |
| `GND` | `GND` | 23 (right side, immediately below GP18) |
| `SDA` | **GP18** | 24 |
| `SCL` | **GP19** | 25 |

Power the OLED from **3.3 V only**. Do not use `VBUS` (pin 40).

Hold BOOTSEL, plug USB-C, then:

```bash
cd firmware
make flash-oled
```

USB CLI is unchanged (`wifi` / `psk` / `server` / `save`). For a fast
poll while you watch the OLED, `sleep 0` then `save` (wakes every 60 s).
The glass and radio go dark between polls; the CDC port drops until the
next wake, so type `show` during the fetch window or tap reset.

The OLED shows PSRAM bring-up, VSYS battery, on-die chip temperature,
Wi-Fi, and the last `/frame.bin` result. Cold-boot e-ink colour fills
run once per power-on, not after a timer wake. With no panel they just
waste a few seconds of SPI.

This module is the Pi Hut 0.96″ 128×64. The listing says SSD1306; the
laser-tag firmware talks to it as SH1106. If the yellow band is readable
and the blue area is noise, in `src/oled.rs` switch
`OledConfig::sh1106_128x64()` to `OledConfig::ssd1306_128x64()` and flash
again. Address `0x3C` (try `0x3D` if the glass stays black).

Between polls both binaries power-down the switched-core (AON LPOSC
alarm wake, `WL_REG_ON` held low, OLED `display_off`). Until Wi-Fi and
server are saved, the node stays awake for the USB CLI.

Without hardware, [`pico-sim`](../pico-sim/) speaks the same HTTP loop.
