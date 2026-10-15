# Certification and compliance

What a connected device with a lithium battery must meet before anyone
outside the household receives one.

Plan stages: listed in **1**, minimum file for pilots in **3**, full
certification in **5**. See [`plan.md`](plan.md).

> Rules and dates below are a starting checklist as understood in
> October 2026. Confirm each one with a test lab or compliance adviser
> before relying on it.

## The key fact

There is no small-batch exemption. Supplying one unit to a consumer in
the course of a business, paid or free, places it on the market. What
changes with volume is the cost per unit, not which rules apply. Pilot
units in Stage 3 need a real compliance file, kept proportionate.

## What applies

### Radio and electrical safety

| Market | Rule | Mark |
|---|---|---|
| Great Britain | Radio Equipment Regulations 2017 | UKCA (CE currently accepted; check) |
| EU and Northern Ireland | Radio Equipment Directive 2014/53/EU | CE |

Typical standards for a 2.4 GHz Wi-Fi / Bluetooth device: EN 300 328
(radio), EN 301 489-1 and -17 (EMC), EN 62368-1 (safety), EN 62311 or
EN 62479 (RF exposure).

The CYW43439 sits on a pre-certified module (Raspberry Pi RM2 on the
Pico boards). That reduces radio testing but does not replace a
declaration of conformity for the finished frame. The home module, if a
Raspberry Pi board or Compute Module, can lean on Raspberry Pi’s own
reports in the same way.

### Cybersecurity for connected products

| Market | Rule | What it means here |
|---|---|---|
| UK | Product Security and Telecommunications Infrastructure (PSTI) regime, in force since April 2024 | No universal default passwords; a published way to report vulnerabilities; a published minimum security-update period; a statement of compliance in the box |
| EU | RED delegated act on cybersecurity (EN 18031 series), applying since August 2025 | Network protection, personal-data protection, secure updates, authentication |
| EU | Cyber Resilience Act | Vulnerability reporting duties from September 2026; full requirements from December 2027 |

Today’s family UI has no authentication and the frame protocol has no
identity ([`platform.md`](platform.md)). Both must change before the
first pilot unit leaves the house.

### General product safety

- GB: General Product Safety Regulations 2005, as amended under the
  Product Regulation and Metrology Act 2025.
- EU: General Product Safety Regulation (EU) 2023/988. Requires an
  economic operator established in the EU, traceability, safety
  information, and product details on online listings.
- A written risk assessment covering the battery, the glass, wall
  mounting, and charging.

### Substances

RoHS and REACH declarations from every part supplier.

### Battery

- UN 38.3 test summary for the cell (needed to ship it at all).
- Carrier rules for lithium-ion in equipment (for example IATA PI 967
  for air).
- EU Battery Regulation (EU) 2023/1542: labelling, producer duties, and
  user-removable batteries in appliances from February 2027
  ([`hardware.md`](hardware.md)).
- UK battery producer registration.

### End of life

- UK WEEE producer registration (small-producer route below 5 tonnes a
  year) and packaging producer duties.
- France: registration with an approved eco-organisation for electrical
  equipment and batteries, a unique identifier (UIN), and the Triman sort
  label.
- Equivalent registrations in each EU country we sell to, or via an
  authorised representative.

## What changes with volume

| Item | Stage 3: pilots (tens) | Stage 5: small company |
|---|---|---|
| Radio, EMC, safety | Pre-compliance scan at a lab; module certificates; self-declared conformity with a technical file | Full test reports for the final design |
| Security | PSTI statement, vulnerability contact, update period, no default passwords | Plus EN 18031 assessment; Cyber Resilience Act process ready |
| Battery | Supplier UN 38.3 summary; safe packaging | Same, plus EU Battery Regulation labelling and removability |
| Product safety | Risk assessment, instructions, warnings, traceable serial numbers | EU responsible person; online-listing details |
| WEEE / batteries / packaging | UK registration | Each sales country |
| Cost | Hundreds to low thousands of pounds | Thousands to tens of thousands of pounds |

Cost ranges are rough orders of magnitude to replace with quotes.

## Open questions

1. GB only for pilots, or France as well? France adds EU registrations
   and an EU economic operator.
2. Which test lab, and can it pre-scan both the frame and the module in
   one visit?
3. Is a USB charger in the box? It brings its own safety and efficiency
   rules.
4. Pilot as a loan or a sale: does it change our obligations? (Assume
   not until advised.)

## Work by stage

- **Stage 1** — This list reviewed; pilot countries chosen.
- **Stage 3** — Technical file, risk assessment, declaration of
  conformity, PSTI statement, vulnerability-report page, registrations
  for the pilot country.
- **Stage 5** — Full test reports and registrations for every sales
  country.
- **Stage 6** — Re-test for the custom board.
