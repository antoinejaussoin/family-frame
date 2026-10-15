# Support

Helping households set up, keeping frames running, and replacing what
breaks.

Plan stages: founder-run in **3**, a support process in **5**. See
[`plan.md`](plan.md).

## Where we are

One household, supported by its builder. Useful tools exist already:

- `/stats`: battery graph, every poll, next wake, firmware drift
  ([`firmware/PROTOCOL.md`](../firmware/PROTOCOL.md)).
- `show` over USB serial: Wi-Fi status, last result, battery.
- Cold-boot colour fills: yellow, red, blue
  ([`firmware/README.md`](../firmware/README.md)).
- `GET /health` returns the server version.

## Expected support topics

From the current build and the failure table in
[`experience.md`](experience.md):

| Topic | Self-help | Needs us |
|---|---|---|
| Wi-Fi is 5 GHz only | Help article | Rarely |
| Frame not updating | On-glass “not updated since” stamp; help article | If the module or cloud is down |
| Battery life shorter than claimed | UI estimate shows the schedule’s cost | Check sleep current; replace the cell if faulty |
| Calendar not showing | UI names the failing link | If the provider changed format |
| Cracked panel in transit | — | Replacement from spares |
| New router | Re-enter setup with a button hold | Rarely |
| Moving house or selling the frame | Factory reset and release | Rarely |
| Security issue reported | — | Vulnerability process ([`certification.md`](certification.md)) |

## Stage 3: founder-run

- One support inbox and a log of every contact: date, serial number,
  topic, time spent, outcome. This log is a gate item in
  [`plan.md`](plan.md).
- Remote diagnostics only with the household’s permission
  ([`privacy.md`](privacy.md)).
- Ten to fifteen help articles covering the table above.
- A replacement policy within the warranty
  ([`company.md`](company.md)).
- A vulnerability-report page and contact (`security.txt`).

## Stage 5: support process

- Help centre and contact form; target response times.
- A support view showing a frame’s last contact, battery trend, firmware
  version, and failed wakes ([`platform.md`](platform.md)).
- Returns and repairs process with spares on the shelf
  ([`supply.md`](supply.md)).
- Firmware and module release notes; staged roll-outs so a bad update
  reaches few homes.
- Published security-update period, honoured.
- Support handled by someone other than the founder, at least for first
  line.

## End of life

- Home frame keeps working without Family Frame servers.
- Cloud frame: if the service ever closes, households get notice, a data
  export, and a way to move to a Home frame or a self-hosted server.

## Open questions

1. Email only, or chat as well?
2. Which hours and languages (English and French)?
3. Paid setup help for less technical buyers?
4. How long do we keep replacement panels and cells for older frames?

## Work by stage

- **Stage 3** — Support inbox, contact log, first help articles,
  vulnerability page.
- **Stage 5** — Help centre, support view, returns process, staged
  updates, first-line support person.
