# Privacy

What household data the product touches, where it goes in each product,
and what we promise.

Plan stages: Home frame promises in **2**, Cloud frame obligations in
**5**. See [`plan.md`](plan.md).

## Why two products

The Home frame exists so a household can have the board without its
calendars, children’s school timetables, and photos living on someone
else’s server. The Cloud frame exists for households who prefer no extra
box. Both choices are legitimate; the shop page must make the difference
plain.

## Data the product touches

| Data | Source today | Sensitivity |
|---|---|---|
| Family calendar events (names, places, medical appointments) | ICS links | High |
| Children’s names and birthdays | Birthdays setup | High (children) |
| School timetable, homework, grades | Pronote | High (children) |
| Family photos | Picture mode uploads | High |
| Room temperatures and humidity | Meross | Medium; reveals occupancy |
| To-do items | Todoist | Medium |
| Wi-Fi name and password | Stored in the frame’s flash | High |
| Logins and tokens for third parties | `config.toml`, `meross-creds.json` | High |
| Battery, wake times, poll history | Frame telemetry, `/stats` | Low to medium; reveals household routine |
| Location for weather, bins, transit | Setup | Medium |

## Home frame

All of the above stays on the module and the frame. What still leaves
the house:

- The module fetches from the household’s own sources (calendar host,
  Todoist, weather, TfL, and so on). Those services see the household’s
  IP address and the request, as they would from any app.
- Update checks to Family Frame servers (module version, frame version).
- Anything the owner opts into later: crash reports, remote support
  access, backups.

Promise to publish: **Family Frame does not receive your calendars,
photos, school data, or household settings.** List the exceptions above
next to it.

Our legal position as a company is lighter here, since the household
processes its own data on its own device. Update checks and any opt-in
telemetry still make us responsible for that slice.

## Cloud frame

Everything in the table is stored and processed by us. Expect to need:

- A privacy notice in plain language; a lawful basis per use (mostly
  performance of the contract).
- A record of processing, retention periods, and a deletion path that
  removes photos and calendars, including from backups within a stated
  time.
- Export of a household’s data.
- Contracts with every hosting and email provider that touches the data.
- A breach response process and the regulator notification route.
- Registration and fee with the UK Information Commissioner’s Office.
- A data protection impact assessment, given children’s data and photos.
- If selling to EU households from a UK company: an EU representative.
- Encryption at rest for third-party logins and tokens; staff access
  logged and minimal.
- If children use the family UI themselves, the UK Children’s Code
  applies to that service.

Get these reviewed by someone qualified before Stage 5. This brief is a
checklist, not legal advice.

## Both products

- Frame factory reset wipes Wi-Fi, keys, and ownership.
- The glass is visible to every visitor. A “guest mode” or per-widget
  hide option is a product feature worth considering (for example hiding
  grades, which already stay off by default with
  `sources.pronote.show_sections = false`).
- Third-party logins move to OAuth where possible so we never hold
  passwords ([`platform.md`](platform.md)).

## Open questions

1. Will the Home frame send any telemetry by default, or only on opt-in?
2. Does the Cloud frame store photos at full resolution, or only the
   1600×1200 dithered version?
3. Which hosting region(s) for the cloud?
4. Is Pronote data acceptable to hold in the cloud at all? (See
   [`widgets.md`](widgets.md) on whether Pronote stays.)

## Work by stage

- **Stage 2** — Home frame promise written and checked against the
  module’s actual network traffic.
- **Stage 3** — Pilot agreement covers what we collect from pilot units.
- **Stage 5** — Cloud frame obligations above in place before the first
  cloud sale.
