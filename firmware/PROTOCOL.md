# Pico frame protocol

The Pico is a dumb client. It does not compose calendars or HTML, and it
does not choose its own sleep interval. On every boot (including a timer
wake) it:

1. Joins 2.4 GHz Wi-Fi.
2. `POST /api/frame.bin` with battery diagnostics in the body and
   `If-None-Match: <last checksum>` when it has one.
3. **204** (or **304**) → leave the panel alone.
4. **200** → write the 960 000-byte body to the Inky, store
   `X-Frame-Checksum` / `ETag`.
5. Sleep for `X-Sleep-Seconds` from that response (or stay awake if USB-C
   is plugged into a host). A failed poll retries after 120 s.

Inky **A** or **B** (the two buttons on the 13.3″ PCB) are POWMAN GPIO
wake sources. A press powers the switched-core back up, the Pico POSTs
with `wake=button`, and the server skips its dashboard TTL cache so the
next image is built from live calendar / to-do / weather data. Unchanged
pixels still return **204** so the glass is not refreshed for nothing.
Buttons **C** and **D** share GP25 / GP24 with the RM2 radio and cannot
be used.

Browsers still `GET /api/frame.bin` to download the packed file; those
hits are not logged. Only POSTs from the Pico (or [`pico-sim`](../pico-sim/))
show up on `/debug`.

The implementation lives in this directory. See [`README.md`](README.md).

## Pico POST

```http
POST /api/frame.bin HTTP/1.1
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
| `wake` | `timer` after POWMAN sleep, `cold` on power-on, `button` if Inky A or B woke the chip (or was pressed while USB kept it awake) |

Checksum is **not** in the URL. Unchanged frames return **204 No Content**
(the honest POST equivalent of 304). Firmware still accepts 304.

The response always includes how long the Pico should stay down:

```http
HTTP/1.1 204 No Content
ETag: <sha256>
X-Frame-Checksum: <sha256>
X-Sleep-Seconds: 3600
```

`X-Sleep-Seconds` is computed on the server from the **current mode’s**
selected schedule: `poll_interval_secs` when `schedule_kind` is `interval`,
or seconds until the next `wake-up` `HH:MM` in `timezone` when it is `times`.
If `schedule_kind` is omitted, a non-empty `wake-up` list selects times.
Dashboard and Pictures each keep both values so the family UI can switch
without losing the other setting.

The Pico’s POWMAN timer (LPOSC) typically runs a few percent slow, so a
commanded hour can land a couple of minutes late. After two consecutive
`wake=timer` polls (not buttons, and not while USB is holding the chip
awake) the server compares wall-clock elapsed time to the previous
`X-Sleep-Seconds`, stores that fraction as `pico_drift` in `config.toml`
(capped at ±5% — larger gaps are ignored), and shortens later sleeps so
the panel still refreshes on the intended wall-clock cadence.

## Endpoints

| URL | Body |
|---|---|
| `POST /api/frame.bin` | Pico poll: form telemetry in, Spectra 6 frame or 204 out, plus `X-Sleep-Seconds` |
| `GET /api/frame.bin` | Same packed frame, for browsers (not logged) |
| `GET /api/frame.png` | Chromium screenshot or current picture (debug) |
| `GET /api/frame-dither.png` | Same pixels after palette quantise |
| `GET /api/frame.json` | `{ checksum, bytes, content_hash, source_note }` |
| `GET /api/settings` | Public schedule / mode (family UI) |
| `GET /api/pictures` | Photo library |
| `GET /api/debug` | Pico poll history, battery graph SVG, next-refresh copy |
| `GET /api/debug/frames/{checksum}.png` | Dithered frame stored for that checksum |
| `GET /` | Family SPA (mode, schedule, photos) |
| `GET /preview` | Layout workbench (same SPA) |
| `GET /debug` | Battery graph and Pico request log (same SPA) |
| `GET /dashboard` | 1600×1200 HTML the server screenshots |
| `GET /health` | `{ "ok": true }` (also at `/api/health`) |

## Packed `.bin` (must match Tesserae / el133-pico-driver)

- 1600 × 1200 landscape, **no header**
- length **exactly 960000**
- two pixels per byte: high nibble = even column, low nibble = odd
- nibbles: `0` black, `1` white, `2` yellow, `3` red, `5` blue, `6` green

The Pico LiPo 2 XL W firmware streams this buffer with
`el133::show_frame()` after a 90° rotate/split (the panel’s two
controllers are portrait halves).
