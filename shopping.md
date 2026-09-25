# Shopping list: 13.3″ family e-ink frame

One board: the **Pico LiPo 2 XL W**. Charge it over USB-C — you do not
need a separate LiPo charger.

The Inky PCB is A4 (**297 × 210 mm**). A pouch cell hides behind it.
Electronics cost is roughly **£180–220** before the frame.

Do **not** buy the non-XL [Pico LiPo 2](https://shop.pimoroni.com/products/pimoroni-pico-lipo-2)
(no Wi-Fi).

Qty | Item | Cost | Exact product and shop | Purpose |
|---:|---|---|:---:|---|
| 1 | Colour e-ink panel | £ 230 | [Pimoroni Inky Impression 13.3″ (2025 Edition, PIM774), The Pi Hut](https://thepihut.com/products/inky-impression-13-3-2025-edition) - [Alternative Supplier](https://shop.pimoroni.com/products/inky-impression?variant=55186435277179) | 1600×1200 Spectra 6 glass. Image stays with the power off. |
| 1 | Pimoroni Pico LiPo 2 XL W (PIM776) (Wi-Fi + PSRAM + LiPo charger) | £ 21 |[Pimoroni Pico LiPo 2 XL W (PIM776), The Pi Hut](https://thepihut.com/products/pimoroni-pico-lipo-2-xl-w) | RP2350B, 8 MB PSRAM, 2.4 GHz Wi-Fi, JST-PH. Charges from USB-C. Headers are **not** in the box. [Pimoroni](https://shop.pimoroni.com/products/pimoroni-pico-lipo-2-xl-w?variant=55447911006587) if Pi Hut is sold out. |
| 1 | Male headers | £1 | [Male Header Set for Raspberry Pi Pico, The Pi Hut](https://thepihut.com/products/male-headers-for-raspberry-pi-pico) | Two 1×20 male strips (2.54 mm). Solder them on the **USB-end** holes only. |
| 1 | Pico-to-Pi adapter | £ 10 | [Hard Stuff Pico to Pi HAT **H** (soldered female headers), The Pi Hut](https://thepihut.com/products/pico-to-pi-hat) | Must be the **H** version, not **X**. Seat the XL W toward the USB end. |
| 1 | Flat LiPo | £ 15 | [Any 3.7 V 10,000 mAh pouch, Amazon](https://www.amazon.co.uk/dp/B0F63419NS?ref=ppx_yo2ov_dt_b_fed_asin_title&th=1) | Plugs into the XL W JST-PH. **‼️ Check polarity before you plug it in. This one MUST be swapped ‼️** |
| 1 | USB-C data cable | | Any USB-C data cable | Flash **and** charge. Charge-only cables will not flash. |
| 1 | Frame | £5 | [IKEA RÖDALM 21 × 30](https://www.ikea.com/gb/en/p/roedalm-frame-black-00548882/) | The one I use. See **Frame** below. |
| 1 | PIR (AS312) | £ 5 | [M5Stack PIR Unit (AS312), The Pi Hut](https://thepihut.com/products/pir-module) | Motion in front of the glass. About 60 µA, so it can stay powered in sleep. Grove cable is in the box. |
| 1 | Warm-white LED bar | £ 4 | [5 V COB LED strip, warm white, The Pi Hut](https://thepihut.com/products/5v-cob-led-strip-light-warm-white) | 60 mm bar for the top inner lip. Run it from the **cell**, not from 5 V — at 5 V it is 3 W. |
| 1 | N-MOSFET | £ 2 | IRLB8721PBF, TO-220 (Pi Hut or RS) | Low-side switch. Fully on from a 3.3 V GPIO. Strip current returns through this, not through the Pico. |
| 1 | 100 kΩ resistor | | Any ¼ W | Gate to ground. Keeps the strip **off** while GP33 floats in reset and in sleep. |

Stack drawings: [`wiring.svg`](wiring.svg), [`connections.svg`](connections.svg).

## ‼️ Battery polarity — check before you plug in ‼️

**JST-PH packs are not standardised.** The pouch I bought had the
opposite polarity to the XL W. A reversed plug can kill the board.

Confirm red = `+` on both the pouch **and** the board silkscreen. If they
disagree, swap the wires in the connector before connecting. Never force
a reversed plug.

## Headers

The XL W has **two rows of 30 holes**. The HAT is a standard Pico
**2×20**. Populate only the 20 holes at the **USB-C end**.

1. Solder the two 20-pin strips into the USB-end holes. Plastic collar on
   the **button / JST** face. Bare pins exit the **back** so they plug
   down into the HAT.
2. Leave the extra ten holes (antenna end) empty **except** the four used by the light: antenna-end `3V3`, a `GND` next to it, `GP32`, `GP33`. Solder wires in those holes. Do not fit a header there — the HAT does not use them.
3. Ignore the 3-pin debug header in the pack.

Do **not** buy stacking, short-plug, or “long” Pico headers, and not a
2×20 Pi GPIO header.

## Frame

**Buy [IKEA RÖDALM 21 × 30](https://www.ikea.com/gb/en/p/roedalm-frame-black-00548882/).**
This is the frame I actually use. The panel fits, the 3 cm depth is
enough for this stack, and it sits flush on the wall. Recommend this one.

Do not buy a standard A4 photo frame or an “A4 3D box” — those inner
wells are usually ~200 mm. The PCB is 210 mm.

## After the parts arrive

1. **Polarity first.** See the warning above.
2. Solder the USB-end headers. Seat the HAT on those 20 pins. The extra
   24 mm of board hangs off the antenna end.
3. Stack: frame glass → Inky → Pico-to-Pi HAT H → XL W (USB end) → pouch
   in the JST.
4. Charge over the Pico’s USB-C. No extra charger.
5. For long sleep, cut the rear **power-LED** trace (LED symbol, USB-C
   end).
6. Run the server simulator in [`server/`](server/) and lock the HTML
   layout **before** flashing [`firmware/`](firmware/).
7. Light, after the stack is in the frame. No firmware for this yet —
   the resistor holds the strip off until GP33 is driven. See
   **Presence light** below.

## Also needed

- a **2.4 GHz** Wi-Fi network (the Pico cannot join 5 GHz-only);
- a computer that can run the Rust server and Chromium;
- a soldering iron (the XL W ships without pins).

## Presence light

Someone walking up to the glass turns on a short warm-white bar. The
sensor stays on in sleep (microamps). The bar is on the cell, switched
off the rest of the day. Drawings: [`wiring.svg`](wiring.svg),
[`connections.svg`](connections.svg).

The HAT covers the USB-end 20 pins. These four holes are on the
**antenna end**, past that header. USB-C at the top, left side, just
past `GP15`:

| Wire | XL W hole | Other end |
|---|---|---|
| PIR red (Grove VCC) | antenna-end `3V3` | PIR stays powered while the Pico sleeps |
| PIR black | `GND` next to `GP31` | Common ground |
| PIR white (OUT) | `GP32` | High for about 2 s after motion. Yellow Grove wire is unused — leave it open |
| MOSFET gate | `GP33` | Also a 100 kΩ from this hole to `GND` |

IRLB8721, writing towards you, leads down: **gate, drain, source**.

- Source → `GND`.
- Drain → LED **−** (black).
- LED **+** (red) → battery **+** on the JST-PH. That net is `VSYS`. Do not use `VBUS`, and do not use `3V3` — the bar is the cell’s load, not the regulator’s.
- Cut the USB plug off the bar and solder to the pads. Stick it on the top inner lip, shining onto the glass. The heatsink in the bag is optional at cell voltage; the bar is only meant to be on for a glance.

`GP30` is the BOOT button, `GP43` is the battery sense, `GP47` is PSRAM. Leave those alone.

## Do not buy

- A separate LiPo charger — USB-C on the XL W is enough.
- Standoff screws — not needed.
- Pico 2 W (no Plus / no PSRAM) — the packed frame is 960000 bytes.
- Pimoroni Pico LiPo 2 (**not** XL W) — charging, no Wi-Fi.
- A USB power bank — many shut off at Pico sleep current.
- Raspberry Pi Zero / Zero 2 W — days of battery, not months.
- A 24 GHz presence module (HLK-LD2410 and the like) — about 80 mA all day, a few days on the 10 Ah pouch.
- A 12 V LED strip — the cell is 3.0–4.2 V. No always-on boost.
- Addressable LEDs (WS2812 and the like) — each chip still draws current with the colour set to black.
