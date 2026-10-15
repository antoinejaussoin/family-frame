# Offer

What a household buys, what is in the box, and what we promise.

Plan stages: defined in **1**, tested with pilots in **3**, both products
on sale in **5**. See [`plan.md`](plan.md).

## Where we are

- One product exists: a self-built frame plus a LAN server you run
  yourself ([`README.md`](../README.md), [`shopping.md`](../shopping.md)).
- Hero claim on the README: **six months on a single charge**, on a
  schedule of a few wakes per day with unchanged frames skipped.
- Two modes on the same panel: family dashboard and rotating photo album
  ([`docs/setup.md`](../docs/setup.md)).
- Panel: 13.3″ Spectra 6, 1600×1200, six colours, image stays with the
  power off. A full refresh takes about 35 seconds.

## Decided

- **2026-10-10** Two products, one frame:
  - **Cloud frame** — the frame alone. Rendering and settings live on
    Family Frame servers. Easiest setup. Household data leaves the house.
  - **Home frame** — the frame plus a **home module** plugged into the
    router. The module renders everything locally. Household data stays
    on the LAN.
- **2026-10-10** The board becomes a widget board; households choose
  what appears ([`widgets.md`](widgets.md)).

## What is in each box

| Item | Cloud frame | Home frame |
|---|---|---|
| Framed 13.3″ colour e-ink panel, battery fitted | Yes | Yes |
| USB-C cable for charging | Yes | Yes |
| Wall fixing (hook, screws, or a stand) | Yes | Yes |
| Quick-start card with a pairing code or QR | Yes | Yes |
| Home module | — | Yes |
| Module power supply and Ethernet cable | — | Yes |
| Account required | Yes | Optional (needed only for remote access or backups, if offered) |

Not in either box: a USB charger (to decide — see open questions).

## What the buyer never does

- Solder headers, fit a HAT, or check battery polarity.
- Install Rust, Node, Docker, or Chromium.
- Open a serial terminal or hold BOOTSEL to flash firmware.
- Edit `config.toml` or HTML.
- Find the server’s IP address.

Each item above is something the current build requires. Removing it is
tracked in [`experience.md`](experience.md), [`platform.md`](platform.md),
and [`hardware.md`](hardware.md).

## Promises we can make

| Promise | Status | Evidence needed |
|---|---|---|
| Months between charges on a typical schedule | Plausible from one frame | Logged battery curves from pilot homes, the schedule they ran, and the cell fitted. State the schedule next to the claim. |
| Picture stays with the power off | True of the panel | — |
| No cable on the wall | True | — |
| Home frame: household data stays in the house | True for rendering and storage | List what still goes out: calls to calendar, weather, and transit services; update checks. See [`privacy.md`](privacy.md). |
| Works without our servers (Home frame) | Target | Module must run fully offline from Family Frame infrastructure apart from optional updates. |
| Updates for N years | To decide | Must be published at sale for UK and EU connected-product rules ([`certification.md`](certification.md)). |

The battery claim matters most. A claim on the box or the shop page is
an advertising claim and must hold for an ordinary household on the
stated schedule. Publish it as “up to N months with M wakes a day” once
pilot data exists.

## Open questions

1. Is the photo album a headline feature or a second mode? It changes the
   shop page and who the product is for.
2. One size only, or a smaller panel later? The 13.3″ panel drives price.
3. Frame finish: the current build uses an IKEA RÖDALM 21×30. A sold
   product needs its own frame or a supplied frame with a licence to use
   it. See [`hardware.md`](hardware.md).
4. Include a USB charger or not? Leaving it out is cheaper and common,
   but every buyer needs a way to charge twice a year.
5. Should the Home frame module also offer an optional cloud connection
   (remote access from a phone, backups)? It blurs the privacy promise if
   on by default.
6. Can a Home frame owner move to the Cloud frame later, and the reverse?
   The frame hardware allows it; the account and data move needs design.
7. Subscription on the Cloud frame? See [`pricing.md`](pricing.md).

## Work by stage

- **Stage 1** — Agree the box contents table and the promises table.
- **Stage 3** — Pilot units match the Home frame box. Collect battery
  data for the claim.
- **Stage 5** — Final box contents for both products; claims backed by
  data; support period published.
