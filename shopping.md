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
2. Leave the extra ten holes (antenna end) empty.
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

## Also needed

- a **2.4 GHz** Wi-Fi network (the Pico cannot join 5 GHz-only);
- a computer that can run the Rust server and Chromium;
- a soldering iron (the XL W ships without pins).

## Do not buy

- A separate LiPo charger — USB-C on the XL W is enough.
- Standoff screws — not needed.
- Pico 2 W (no Plus / no PSRAM) — the packed frame is 960000 bytes.
- Pimoroni Pico LiPo 2 (**not** XL W) — charging, no Wi-Fi.
- A USB power bank — many shut off at Pico sleep current.
- Raspberry Pi Zero / Zero 2 W — days of battery, not months.
