# Supply, assembly, and fulfilment

Getting parts in, building frames and modules that work every time, and
getting them to the customer.

Plan stages: hand build in **3**, repeatable build in **5**, contract
manufacture in **6**. See [`plan.md`](plan.md).

## Where we are

One frame, assembled by hand from retail orders at The Pi Hut, Pimoroni,
Amazon, and IKEA ([`shopping.md`](../shopping.md)). No stock, no test
procedure beyond “it paints”.

## Stage 3: hand build (tens)

### Parts

- Order panels in batches; they are the long-lead, high-cost part.
- One battery supplier with a test summary and fixed polarity
  ([`hardware.md`](hardware.md)).
- Keep 10% spare of every part for failures and replacements.

### Assembly procedure

Write it as a checklist with photos, one per unit:

1. Fit headers (or pre-soldered parts, if available).
2. Cut the power-LED trace.
3. Flash the production firmware.
4. Fit the cell; confirm polarity on a meter.
5. Stack panel, HAT, board, and cell into the frame.
6. Record the serial number and device identity.

### End-of-line test

1. Joins the test Wi-Fi and pairs with a test module.
2. Paints a test pattern with all six colours.
3. Buttons A and B wake the frame.
4. Battery voltage reported, and charging over USB-C works.
5. Sleep current measured against a limit; the six-month claim depends
   on it.
6. Factory reset, so the customer starts clean.

Log the results per serial number.

### Home module

Flash the module image, run its self-test, record its serial number,
factory reset.

### Packaging

Protects the glass (the panel is the most fragile and expensive part);
holds the card, cable, and module; carries battery and WEEE marks and
the safety leaflet ([`certification.md`](certification.md)).

### Shipping

- Lithium-ion in equipment: check each carrier’s rules and labels.
- Insured, tracked delivery; panels can crack in transit.
- Keep the box and the inner packaging used for returns.

## Stage 5: repeatable build (low hundreds a year)

- A small adapter board replacing the HAT and headers, or pre-assembled
  sub-assemblies from a local assembler.
- A test jig that runs the end-of-line test without manual steps.
- Custom frame from a framer, in agreed finishes.
- Packaging designed and printed.
- A third-party logistics partner for storage and dispatch, or
  dispatched from a small unit.
- Stock planning: panel lead times set how much cash sits in inventory.

## Stage 6: contract manufacture

- Custom board and enclosure built by a contract manufacturer.
- Design for manufacture review, pilot run, first article inspection.
- Incoming inspection for panels and cells.

## Open questions

1. Does Pimoroni sell the Pico LiPo 2 XL W with headers fitted, or is
   there a variant that removes soldering?
2. Who assembles in Stage 5: us, or a local assembler?
3. Ship ourselves or use a logistics partner from the start?
4. How many spare panels to hold for breakages and warranty?

## Work by stage

- **Stage 3** — Assembly checklist, end-of-line test, packaging that
  survives a courier, carrier chosen for lithium shipments.
- **Stage 5** — Test jig, custom frame, logistics partner, stock plan.
- **Stage 6** — Contract manufacturer.
