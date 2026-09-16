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

Browsers still `GET /api/frame.bin` (or the legacy `/frame.bin` alias) to
download the packed file; those hits are not logged. Only POSTs from the
Pico (or [`pico-sim`](../pico-sim/)) show up on `/debug`.

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
| `wake` | `timer` after POWMAN sleep, `cold` on power-on |

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
`poll_interval_secs` or, when that mode’s `wake-up` has at least one `HH:MM`
time, the number of seconds until the next of those times in `timezone`.
Dashboard and Pictures can each have their own schedule.

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
| `GET /` | Family SPA (mode, schedule, photos) |
| `GET /dashboard` | 1600×1200 HTML the server screenshots |
| `GET /preview` | Layout workbench |
| `GET /debug` | Battery graph and Pico request log |

Legacy aliases: `/frame.bin`, `/frame.png`, `/frame-dither.png`, `/frame.json`,
`/health` still work for older firmware and bookmarks.

## Packed `.bin` (must match Tesserae / el133-pico-driver)

- 1600 × 1200 landscape, **no header**
- length **exactly 960000**
- two pixels per byte: high nibble = even column, low nibble = odd
- nibbles: `0` black, `1` white, `2` yellow, `3` red, `5` blue, `6` green

The Pico LiPo 2 XL W firmware streams this buffer with
`el133::show_frame()` after a 90° rotate/split (the panel’s two
controllers are portrait halves).
