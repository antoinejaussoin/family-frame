# Commercial

How Family Frame goes from a board you build yourself to a product a
household buys, unboxes, and hangs in the kitchen.

Start with [`plan.md`](plan.md). It is the only high-level plan: six
stages, the gate that closes each one, and which brief owns the work.
Every other file here is a working brief for one step.

Developer setup stays in [`docs/`](../docs/). These briefs describe the
work. They do not implement it.

> **Visibility.** This repository is public on GitHub. Anything in this
> folder is public too until the licence and visibility decision in
> [`licensing.md`](licensing.md) is made. Keep supplier quotes, prices
> you have negotiated, and customer data out of git.

## How to use a brief

Each brief has the same shape:

1. **Where we are** — facts from this repository today.
2. **Decided** — choices already made, with the date.
3. **Open questions** — what blocks the next stage.
4. **Work by stage** — what this brief must deliver at each gate in
   [`plan.md`](plan.md).

When a question is answered, move it to **Decided**, then add a line to
the decision log below.

## The two products

| | Cloud frame | Home frame |
|---|---|---|
| In the box | Frame | Frame + home module |
| Where rendering runs | Family Frame servers | The module, on the household router |
| Household data | Leaves the house | Stays on the LAN |
| Account | Required | Optional |
| Setup | Wi-Fi on the frame | Plug the module into the router, then Wi-Fi on the frame |

Both use the same frame and the same protocol: the frame is a dumb
client that asks for a picture and goes back to sleep
([`firmware/PROTOCOL.md`](../firmware/PROTOCOL.md)).

## Documents

| Brief | Covers | Status |
|---|---|---|
| [`plan.md`](plan.md) | Stages, gates, sequence | Draft |
| [`offer.md`](offer.md) | What each box contains and promises | Draft |
| [`experience.md`](experience.md) | Buy, unbox, connect, choose, hang, charge | Draft |
| [`widgets.md`](widgets.md) | From a fixed grid to a layout people compose | Draft |
| [`platform.md`](platform.md) | Server, home image, cloud, updates, identity | Draft |
| [`hardware.md`](hardware.md) | Frame and module bill of materials, design for assembly | Draft |
| [`privacy.md`](privacy.md) | Household and children’s data, both products | Draft |
| [`licensing.md`](licensing.md) | GPL / AGPL facts, open decision | Draft — decision parked |
| [`company.md`](company.md) | Legal entity, consumer law, warranty, insurance | Draft |
| [`certification.md`](certification.md) | Radio, battery, safety marks, WEEE | Draft |
| [`supply.md`](supply.md) | Parts, assembly, packaging, fulfilment | Draft |
| [`pricing.md`](pricing.md) | Cost stack and pricing model | Draft |
| [`go-to-market.md`](go-to-market.md) | First buyers, then international | Draft |
| [`support.md`](support.md) | Setup help, faults, updates, replacements | Draft |

## Decision log

| Date | Decision | Brief |
|---|---|---|
| 2026-10-10 | Two products on one frame: a **Cloud frame** and a **Home frame** with a module on the router. | [`offer.md`](offer.md) |
| 2026-10-10 | Both products are on the roadmap; sequence is home module first, cloud designed in from the start. | [`plan.md`](plan.md) |
| 2026-10-10 | The board becomes a widget board. Households pick widgets and layout; country-specific sources are optional packs. The goal is international. | [`widgets.md`](widgets.md) |
| 2026-10-10 | Scale grows in gated stages: tens of units, then low hundreds a year as a small company, then a product company. | [`plan.md`](plan.md) |
| 2026-10-10 | Licence and repository visibility are **parked**. Making the repo private and changing the licence is an option to take to a lawyer. | [`licensing.md`](licensing.md) |
