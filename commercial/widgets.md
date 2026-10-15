# Widgets

From a fixed grid built for one kitchen to a board any household
composes.

Plan stages: direction in **1**, data sources checked in **2**, built in
**4**, more country packs in **6**. See [`plan.md`](plan.md).

## Where we are

The 1600×1200 dashboard is a fixed grid in
[`server/templates/dashboard.html`](../server/templates/dashboard.html)
and [`server/static/dashboard.css`](../server/static/dashboard.css).
Data comes from a registry of `DataSource` implementations
([`server/src/sources/README.md`](../server/src/sources/README.md)).
Sources can be turned off; empty blocks collapse. Households cannot move,
resize, or add blocks without editing HTML.

### Current sources

| Source | Data from | Country | Commercial terms |
|---|---|---|---|
| `calendar` | Published ICS links | Any | Household’s own data |
| `birthdays` | Typed in the UI | Any | Household’s own data |
| `todoist` | Todoist API with a personal token | Any | Needs a registered app and OAuth for customers |
| `weather` | BBC Weather broker endpoint | UK-centric | **No agreement.** Undocumented endpoint. Replace before selling |
| `tfl` | TfL Unified API | London | Open data; registration and attribution required |
| `bins` | ukbinday.co.uk | UK | Check terms for commercial use |
| `pronote` | Unofficial Pronote session protocol | France | **No agreement.** Reverse-engineered; may break or breach terms |
| `meross` | Meross cloud login, MQTT | Any | **No agreement.** Uses account credentials |
| `history` | Wikipedia “On this day” | Any (English) | CC BY-SA; attribution; API etiquette |
| `jokes` | icanhazdadjoke | English | Check terms for commercial use |
| `saints` | Built-in French civil calendar | France | Own data |
| `minecraft` | Typed in the UI | Any | Own data |
| Picture mode | Household photo uploads | Any | Household’s own data |

## Decided

- **2026-10-10** Households choose widgets and their layout. The product
  is country-agnostic; country-specific widgets are optional packs.
- **2026-10-10** The goal is international.

## Direction

### Core widgets (every country)

Calendar (ICS and later direct Google / Microsoft / Apple), weather,
to-dos, birthdays and countdowns, a pinned family note, photos,
date and day, battery and next wake.

### Country packs

| Pack | Widgets |
|---|---|
| UK | Bin day, TfL, National Rail, school term dates, bank holidays |
| France | Saints, Pronote (if a lawful route exists), jours fériés, vacances scolaires |
| Later | Other countries’ transit, waste, school systems |

### Optional and fun

Jokes, on this day, Minecraft allowance (generalised to “screen-time
allowance”), Meross and other smart-home readings (through Home
Assistant rather than vendor clouds).

### Ideas already listed

[`ideas.md`](../ideas.md) ranks exception banners, kit flags, meals,
canteen menu, and countdowns highly. Those fit as core widgets.

## Layout model

The panel is fixed: 1600×1200, six colours, slow refresh. Free-form
drag-and-drop on a canvas is the wrong tool for that. A small set of
grid templates with slots, where each widget declares the sizes it can
render at, keeps every layout legible on e-ink.

Constraints every widget must respect:

- Legible from across a kitchen in six colours after dithering.
- Changes rarely between wakes, so the checksum skip saves the battery
  (see [`firmware/PROTOCOL.md`](../firmware/PROTOCOL.md)).
- Collapses cleanly when there is no data.
- Translatable text and locale-aware dates.

## Work to do

1. Define a widget interface: settings schema, sizes it supports, data
   fetch, template, empty state.
2. Move each current block onto that interface.
3. Store layouts per household (needed for the cloud; see
   [`platform.md`](platform.md)).
4. Build the layout editor in the family UI on top of today’s `/preview`
   simulator.
5. Translate UI and widget text; ship English and French.
6. Replace every source marked **No agreement** with a licensed one or
   drop it from the sold product. Weather is the urgent one.

## Open questions

1. Does Pronote stay in a sold product? Options: drop it, keep it as a
   self-hosted community add-on outside the product, or seek an agreement
   with the publisher.
2. Third-party widgets: open to outside developers later, or in-house
   only? It affects licensing and the cloud security model.
3. Which weather provider? Candidates to evaluate on commercial terms,
   coverage, and cost per household: Met Office DataHub, Open-Meteo
   (commercial plan), MET Norway, a paid global API.
4. Is Home Assistant the smart-home route for both products, or only for
   the Home frame?

## Work by stage

- **Stage 1** — Direction above agreed.
- **Stage 2** — Commercial terms checked for every source; replacements
  chosen.
- **Stage 3** — Pilots run the fixed dashboard with unlicensed sources
  removed.
- **Stage 4** — Widget interface, layout editor, UK and France packs,
  English and French.
- **Stage 6** — More packs.
