# Pico frame protocol

The Pico is a dumb client. It does not compose calendars or HTML. Once an
hour it:

1. Wakes, joins 2.4 GHz Wi-Fi.
2. `POST /frame.bin` with battery diagnostics in the body and
   `If-None-Match: <last checksum>` when it has one.
3. **204** (or **304**) → leave the panel alone, then sleep (or stay
   awake if USB-C is plugged into a host).
4. **200** → write the 960 000-byte body to the Inky, store
   `X-Frame-Checksum` / `ETag`, then sleep unless USB is plugged.

Browsers still `GET /frame.bin` to download the packed file; those hits
are not logged. Only POSTs from the Pico (or [`pico-sim`](../pico-sim/))
show up on `/debug`.

The implementation lives in this directory. See [`README.md`](README.md).

## Pico POST

```http
POST /frame.bin HTTP/1.1
Host: 192.168.0.251:8765
Connection: close
If-None-Match: <sha256>
Content-Type: application/x-www-form-urlencoded
Content-Length: 31

mv=3850&pct=72&usb=0&wake=timer
```

| Field | Meaning |
|---|---|
| `mv` | VSYS millivolts |
| `pct` | 0–100 estimate (3.3–4.2 V linear map) |
| `usb` | `1` if a USB host is sending SOFs, else `0` |
| `wake` | `timer` after POWMAN sleep, `cold` on power-on |

Checksum is **not** in the URL. Unchanged frames return **204 No Content**
(the honest POST equivalent of 304). Firmware still accepts 304.

## Endpoints

| URL | Body |
|---|---|
| `POST /frame.bin` | Pico poll: form telemetry in, Spectra 6 frame or 204 out |
| `GET /frame.bin` | Same packed frame, for browsers (not logged) |
| `GET /frame.png` | Chromium screenshot (debug) |
| `GET /frame-dither.png` | Same pixels after palette quantise |
| `GET /frame.json` | `{ checksum, bytes, content_hash, source_note }` |
| `GET /dashboard` | 1600×1200 HTML the server screenshots |
| `GET /preview` | Layout workbench |
| `GET /debug` | Battery graph and Pico request log |

## Packed `.bin` (must match Tesserae / el133-pico-driver)

- 1600 × 1200 landscape, **no header**
- length **exactly 960000**
- two pixels per byte: high nibble = even column, low nibble = odd
- nibbles: `0` black, `1` white, `2` yellow, `3` red, `5` blue, `6` green

The Pico LiPo 2 XL W firmware streams this buffer with
`el133::show_frame()` after a 90° rotate/split (the panel’s two
controllers are portrait halves).

The Rust [`pico-sim`](../pico-sim/) crate exercises the checksum loop
without hardware:

```bash
cd pico-sim
cargo run --quiet -- --url http://127.0.0.1:8765 --interval-secs 5 --drain
```

New frames are saved as timestamped PNGs in `pico-sim/out/`. `--drain`
lowers the fake battery each poll so `/debug` can plot a slope.

## Power

- LiPo → Pico LiPo 2 XL W **JST-PH** (onboard MCP73831; 3.0–4.2 V).
- USB-C flashes and charges. Do not feed **VBUS**.
- User LED (RM2 `WL_GPIO0`) is on only while a USB host is sending SOFs.
  The white power LED is hardwired to 3V3; cut the rear LED trace to kill it.
- Inky 3.3 V and SPI ride the 40-pin header.
- Between polls the switched-core is powered down (AON timer wake,
  CYW43439 `WL_REG_ON` held low) unless a USB host is sending SOFs.
  A plugged USB-C data cable keeps the CDC CLI enumerated. Default
  interval is 3600 s.
- 2.4 GHz only. Reserved DHCP or a static IP keeps the wake under ~45 s.
