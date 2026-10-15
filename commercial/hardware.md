# Hardware

The frame and the home module as products a stranger can unbox safely.

Plan stages: module chosen in **2**, hand-built pilots in **3**,
repeatable build in **5**, custom board in **6**. See [`plan.md`](plan.md).

## Where we are

### Frame (from [`shopping.md`](../shopping.md))

| Part | Retail price | Notes |
|---:|---|---|
| Pimoroni Inky Impression 13.3″ (Spectra 6, 1600×1200) | £230 | Most of the cost. One supplier |
| Pimoroni Pico LiPo 2 XL W (RP2350B, 8 MB PSRAM, CYW43439 radio, charger) | £21 | Headers not fitted |
| Male headers | £1 | Hand-soldered |
| Hard Stuff Pico-to-Pi HAT H | £10 | Swaps SCLK and MOSI against its datasheet |
| 3.7 V 10 000 mAh pouch | £15 | Amazon listing; polarity reversed against the board |
| IKEA RÖDALM 21×30 frame | £5 | 3 cm depth fits the stack |
| **Total** | **about £282** | |

[`shopping.md`](../shopping.md) also says “electronics cost is roughly
£180–220 before the frame”, which does not match its own table (about
£277 before the frame). Fix one of the two.

Build steps that a buyer must never do: solder headers, check and swap
battery polarity, cut the power-LED trace for long sleep, seat the HAT
toward the USB end.

### Home module

Does not exist yet. Today the server runs on any computer or Docker host
with Chromium.

## Decided

- **2026-10-10** A Home frame ships with a separate module that plugs
  into the router.

## Frame: path from parts to product

| Stage | Frame electronics | Who assembles |
|---|---|---|
| 3 Pilots | Current retail stack, assembled and tested by us; known-good cell with protection circuit and correct connector; LED trace cut | Us, by hand |
| 5 Small company | Same stack or a small adapter board that replaces the HAT and headers; custom frame | Us or a local assembler, with a test jig |
| 6 Product company | Custom board: RP2350 + Raspberry Pi RM2 radio module + panel connector + charger + fuel gauge | Contract manufacturer |

Keeping a pre-certified radio module (the RM2 used on the Pico boards)
on the custom board avoids certifying a radio design from scratch. See
[`certification.md`](certification.md).

### Battery

- 10 000 mAh at 3.7 V is about 37 Wh: under the 100 Wh limit for
  shipping lithium-ion cells inside equipment, but still regulated
  (UN 38.3 test summary, labelling, carrier rules).
- Source cells from a maker that supplies a UN 38.3 test summary and a
  datasheet, with a built-in protection circuit. Retail marketplace
  cells are not acceptable in a sold product.
- Fix the connector polarity once, at the supplier.
- The EU Battery Regulation requires portable batteries in appliances to
  be removable and replaceable by the user from February 2027. Design
  the frame back so the cell can be swapped with common tools. Check the
  exact text before Stage 5.
- A fuel gauge would make the battery estimate more accurate than the
  current voltage-based `pct` (3.3–4.2 V linear map,
  [`firmware/PROTOCOL.md`](../firmware/PROTOCOL.md)).

### Enclosure

The IKEA frame is fine for one house. For a product: a frame sourced from
a framer or made to order, with a back that holds the board and cell,
a slot for the USB-C port, access to wake buttons A and B, and a wall
fixing. Buttons C and D are not usable (they share pins with the radio).

### Panel supply

The 13.3″ Spectra 6 panel (EL133UF1) is the critical part. Pimoroni’s
Inky Impression bundles it with a driver board at retail price.

- Stage 3: buy Inky Impressions retail.
- Stage 5: ask Pimoroni for a trade price and supply commitment.
- Stage 6: buy panels from the panel maker or a distributor and drive
  them from the custom board.

## Home module

Must run the renderer with Chromium ([`platform.md`](platform.md)),
plug into the router by Ethernet, and run unattended for years.

| Option | Approximate cost | For | Against |
|---|---|---|---|
| Raspberry Pi 5 (4 GB) + case + PSU + storage | Board around £50–60 plus extras; price at quote time | Known platform; arm64 Docker image exists | SD cards wear; check stock and pricing |
| Raspberry Pi Compute Module 5 with eMMC on a small carrier | Higher than a Pi 5 | Industrial lifetime, eMMC storage, pre-certified radio variant | Carrier board and enclosure to design |
| Small x86 mini PC | Varies | Fast, amd64 image exists | Larger, more power, more varied supply |

Prices above are placeholders to verify with real quotes in Stage 2;
keep quotes out of this public repo.

Requirements: Ethernet; 2 GB RAM minimum for Chromium, 4 GB preferred;
eMMC or SSD instead of an SD card for a sold product; a status light; a
reset button; an enclosure that does not need a fan.

## Open questions

1. Raspberry Pi 5, CM5, or mini PC for the module?
2. Does the frame keep the 10 000 mAh cell, or does a smaller cell plus
   a better sleep current give the same six months?
3. Frame design: who designs it, and in which finishes?
4. Will Pimoroni sell Inky Impressions or bare panels at trade price, and
   with what notice on end-of-life?
5. When does a custom frame board pay for itself?

## Work by stage

- **Stage 2** — Module chosen and costed. Cell supplier chosen.
  Enclosure concept.
- **Stage 3** — Pilot frames assembled and tested by us; pilot modules
  built on the chosen board.
- **Stage 5** — Repeatable assembly, test jig, custom frame, trade supply
  for the panel.
- **Stage 6** — Custom board, contract manufacture.
