# Pico frame protocol

The Pico is a dumb client. It does not compose calendars or HTML. Once an
hour it:

1. Wakes, joins 2.4 GHz Wi-Fi.
2. `GET /frame.bin?checksum=<last>` (also send `If-None-Match: <last>`).
3. **304** → leave the panel alone, go back to sleep.
4. **200** → write the 960 000-byte body to the Inky, store
   `X-Frame-Checksum` / `ETag`, sleep.

The implementation lives in this directory. See [`README.md`](README.md).

## Endpoints

| URL | Body |
|---|---|
| `GET /frame.bin` | Spectra 6 packed frame, `application/octet-stream` |
| `GET /frame.png` | Chromium screenshot (debug) |
| `GET /frame-dither.png` | Same pixels after palette quantise |
| `GET /frame.json` | `{ checksum, bytes, content_hash, source_note }` |
| `GET /dashboard` | 1600×1200 HTML the server screenshots |
| `GET /preview` | Layout workbench |

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
cargo run --quiet -- --url http://127.0.0.1:8765 --interval-secs 5
```

New frames are saved as timestamped PNGs in `pico-sim/out/`.

## Power

- LiPo → Pico LiPo 2 XL W **JST-PH** (onboard MCP73831; 3.0–4.2 V).
- USB-C flashes and charges. Do not feed **VBUS**.
- Inky 3.3 V and SPI ride the 40-pin header.
- The first Rust port polls in Embassy PowerSave (`sleep` seconds, default
  3600). Shut the CYW43439 and POWMAN-sleep once that path is verified,
  or you will not get weeks.
- 2.4 GHz only. Reserved DHCP or a static IP keeps the wake under ~45 s.
