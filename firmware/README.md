# Pico firmware (dumb client)

Embassy / Rust client for a Pimoroni **Pico LiPo 2 XL W** and Inky
Impression 13.3″. Family data and HTML stay on the LAN server. This
binary:

1. Joins 2.4 GHz Wi-Fi (SSID and password from USB, not compiled in).
2. `GET /frame.bin?checksum=<last>` (and `If-None-Match`).
3. **304** → leave the glass alone.
4. **200** → `show_frame()` the 960 000-byte body, store the checksum.
5. Waits `sleep` seconds (default 3600) and repeats.

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
| `server <host:port>` | Family-frame HTTP origin. `GET /frame.bin` is appended |
| `sleep <seconds>` | Interval between polls. `0` = stay awake, poll every 60 s |
| `save` | Write the last flash sector and join Wi-Fi |
| `show` | SSID, URL, sleep, checksum, Wi-Fi / frame status, battery |
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
```

## Flash

Hold **BOOTSEL** (or the board’s BOOT button) while plugging USB-C, or
double-tap reset. The drive is `RP2350`.

```bash
make flash
# or: cp family-frame.uf2 /Volumes/RP2350/
```

## What you should see

After `save`, `show` reports `wifi ok` then `frame 200` or `frame 304`.
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

Cut the rear **power-LED** trace for weeks of sleep. Solder **`+1A Mode`**
only if a refresh browns out and the cell can deliver it.

The first Rust port stays in Embassy and polls (`sleep` seconds, radio in
PowerSave). POWMAN power-gating can follow once the panel path is
verified on hardware.

Without hardware, [`pico-sim`](../pico-sim/) speaks the same HTTP loop.
