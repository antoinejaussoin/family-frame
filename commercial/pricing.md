# Pricing

What each unit costs us, what the cloud costs per household, and how to
price the two products.

Plan stages: first numbers for **3**, full model for **5**. See
[`plan.md`](plan.md).

No retail price is set here. Fill the blanks with real quotes and keep
negotiated prices out of this public repo.

## Where we are

Frame parts at retail prices come to about **£282** including the IKEA
frame ([`hardware.md`](hardware.md)). The panel is £230 of that. Retail
prices are the worst case; trade prices come later.

## Unit cost worksheet

### Frame

| Line | Stage 3 | Stage 5 |
|---|---|---|
| Panel (Inky Impression 13.3″) | £230 (retail) | Trade price: ___ |
| Board, adapter, headers | £32 (retail) | ___ |
| Battery with protection and test summary | ___ | ___ |
| Frame and back | £5 (IKEA) | Custom: ___ |
| Cable, card, wall fixing | ___ | ___ |
| Packaging | ___ | ___ |
| Assembly and test time × hourly rate | ___ | ___ |
| Failure and scrap allowance (percentage) | ___ | ___ |
| **Frame landed cost** | ___ | ___ |

### Home module (Home frame only)

| Line | Stage 3 | Stage 5 |
|---|---|---|
| Board (Pi 5 / CM5 / mini PC) | ___ | ___ |
| Storage (eMMC or SSD) | ___ | ___ |
| Enclosure | ___ | ___ |
| Power supply and Ethernet cable | ___ | ___ |
| Flash and test time | ___ | ___ |
| **Module landed cost** | ___ | ___ |

### Per order

| Line | Value |
|---|---|
| Shipping out (insured, lithium) | ___ |
| Payment fees (percentage of price) | ___ |
| Returns allowance (percentage of orders × cost of a return) | ___ |
| Warranty reserve (percentage of price) | ___ |
| Certification and registration costs ÷ units expected | ___ |
| VAT (20% in the UK, inside a consumer price) | — |

## Cloud running cost per household

Render volume is small: a few wakes a day per frame
([`platform.md`](platform.md)). Costs to estimate:

| Line | Per household per year |
|---|---|
| Rendering (Chromium) and fetching | ___ |
| Photo storage | ___ |
| Database, backups, monitoring | ___ |
| Licensed data (weather and similar) | ___ |
| Email and notifications | ___ |
| Support time | ___ |

Multiply by the number of years of service promised at sale. A frame
sold once with “cloud included forever” is a liability that grows each
year.

## Pricing models to compare

| Model | Cloud frame | Home frame |
|---|---|---|
| One-off price | Includes a stated number of years of service | Simple; the module has no running cost to us |
| Hardware + subscription | Lower upfront, recurring revenue that matches recurring cost | Optional paid extras only (backups, remote access) |
| Hardware + free basic tier + paid tier | Free tier covers core widgets; paid tier adds premium widgets and photo storage | Same paid extras available |

A common rule of thumb for direct-to-consumer hardware is a retail price
of roughly 2.5 to 4 times landed cost, so that overheads, returns, and
development are paid for. Treat it as a sanity check, not a target.

## Open questions

1. Subscription on the Cloud frame, or one-off with a fixed service
   period?
2. Is the Home frame priced higher (extra module) or the same as a
   Cloud frame plus a few years of subscription?
3. Pilot price: at cost, discounted, or full price?
4. Which costs fall when moving from retail Inky Impressions to bare
   panels and a custom board, and by how much?

## Work by stage

- **Stage 3** — Worksheet filled with real Stage 3 costs; pilot price
  set.
- **Stage 5** — Stage 5 column, cloud cost per household, chosen model,
  launch prices for both products.
