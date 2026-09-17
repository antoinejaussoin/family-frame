# Family e-ink frame

An e-ink, battery-powered frame for the family.

A 13.3″ Spectra 6 panel in a picture frame. A **Pimoroni Pico LiPo 2 XL W**
wakes, downloads a packed image, and sleeps for however long the server
says. A **Rust server** on the LAN builds that image from HTML/CSS plus
the family calendar, to-dos, house temperatures, Tube status, and BBC
weather in the section headers.

Hardware to buy is in [`shopping.md`](shopping.md).

## THIS IS A WORK IN PROGRESS

This is being worked on, not working yet.

## What you get

| Piece | Where |
|---|---|
| Hardware shopping list | [`shopping.md`](shopping.md) (list A: LiPo 2 XL W, or list B: Plus 2 W) |
| Wiring / stack | [`wiring.svg`](wiring.svg), [`connections.svg`](connections.svg) |
| Pico firmware (LiPo 2 XL W) | [`firmware/`](firmware/) |
| Rust server + family UI + layout simulator | [`server/`](server/) |
| Family SPA (Svelte) | [`server/ui/`](server/ui/) |
| Pico client simulator | [`pico-sim/`](pico-sim/) |

## Family UI

Open the family app on a phone or laptop (trusted LAN — no auth). From there you can:

- Switch between **dashboard** and **picture** mode
- Edit the poll interval or wake-up times **per mode** (both are kept; the UI stores which one is selected)
- Upload landscape photos (stored under `pictures/` next to the config)
- Choose which photos to rotate each wake, and preview the dithered Spectra 6 look

### Iterate with hot reload

Run the API and the Svelte app as two processes. Vite proxies `/api` and
`/dashboard` to the server so you get HMR without `npm run build`. `/preview`
and `/debug` are pages in the SPA.

```bash
# terminal 1 — Rust API
cd server && cargo run -- --watch   # or: make watch

# terminal 2 — family UI
cd server/ui && npm ci && npm run dev   # or: make ui-dev
```

Open <http://127.0.0.1:5173/>. If the server is not on `:8765`, set `EINK_API`
(for example `EINK_API=http://127.0.0.1:9000 npm run dev`).

### Serve the built SPA from the API

Docker does this automatically. Locally:

```bash
cd server/ui && npm ci && npm run build
```

Then <http://127.0.0.1:8765/> is the family UI.

## Layout workflow

1. Edit [`server/templates/dashboard.html`](server/templates/dashboard.html) and
   [`server/static/dashboard.css`](server/static/dashboard.css).
2. Open `/preview` in a browser. The iframe is the real 1600×1200 panel.
3. On the LAN, Chromium screenshots `/dashboard`, the server dithers to
   Spectra 6, and the Pico POSTs `/api/frame.bin` with battery diagnostics.
4. If the family data has not changed, the checksum matches and the Pico
   does **not** refresh the glass. Open `/debug` on a phone to see battery
   history and every Pico poll.

The dashboard HTML must not include a ticking clock. A changing “updated at”
would make every hour look like a new image.

## Run the server

Chrome or Chromium is required only for dashboard `/api/frame.bin` /
`/api/frame.png`. Picture mode and the layout simulator (`/preview`) work without it.

```bash
cd server
cp config.example.toml config.toml   # optional; demo data is the default
cargo run                            # or: make serve
```

The family SPA is optional for the API. Without `ui/dist`, `/` explains how to
start Vite; `/dashboard` and `/api/*` still work. `make` (no target)
builds the SPA first, then runs the server.

While iterating on Rust, templates, or dashboard CSS, `--watch` rebuilds and
restarts (not `config.toml` — the family UI edits that live; not `ui/` — use
Vite for that). Do not use `--watch` in production (Docker `CMD` is the binary
with no flags).

```bash
cargo run -- --watch
# or: make watch
```

Then open <http://127.0.0.1:5173/> (Vite) or <http://127.0.0.1:8765/> (built
SPA), the layout simulator at <http://127.0.0.1:8765/preview>, or the debug
page at <http://127.0.0.1:8765/debug>.

### Docker

The image includes Google Chrome (amd64) or Chromium (arm64) and the built
family UI so `/api/frame.bin` and `/` work. Deploy only needs `config.toml`
(and optional photos under `data/pictures/`).

On the Linux box, copy [`docker-compose.yml`](docker-compose.yml) and a `data/config.toml` (from [`server/config.example.toml`](server/config.example.toml)):

```bash
mkdir -p data
# edit data/config.toml
docker compose up -d
```

Then <http://<host>:8765/>, <http://<host>:8765/preview>, or
<http://<host>:8765/debug>. Meross login, BBC weather caches, uploaded
photos, and Pico poll history stay in `data/` next to the config.

Local one-off: `cd server && make docker-build && make docker-run`. Pushes to Docker Hub (`antoinejaussoin/family-frame-server`) happen from GitHub Actions on `main` (repo secrets `DOCKER_USERNAME` and `DOCKER_PASSWORD`, same as compta). Images are tagged `latest` and with the contents of [`VERSION`](VERSION).

## Versioning

The version is a single line in [`VERSION`](VERSION). That is the only file to edit when you cut a release — Cargo.toml, package.json, and Docker labels are filled in at **build time**, so there is no extra commit that rewrites manifests.

1. Change `VERSION` (for example `0.1.0` → `0.2.0`) in a PR.
2. Merge to `main`.
3. CI builds `antoinejaussoin/family-frame-server:0.2.0` and `:latest`, and creates git tag `v0.2.0` if it does not already exist.

Locally, `eink-frame --version`, `GET /health`, and the Debug page all read the same value (`make docker-build` passes it as a Docker build-arg).

```bash
# edit VERSION, then:
cd server && cargo run -- --version
```

## Pretend to be the Pico

A separate crate polls `/api/frame.bin` the way the LiPo 2 XL W will: POST
battery diagnostics, keep the last checksum, skip a refresh on 204, and
unpack a new frame to PNG on 200.

```bash
cd pico-sim
cargo run -- --url http://127.0.0.1:8765 --drain
# or: make run
```

Sleep length comes from `X-Sleep-Seconds`, the same way the Pico does.
Each new frame is written as a timestamped PNG under `pico-sim/out/` (gitignored).

## Family calendar

Apple does not offer a public “Family Sharing API”. What works:

1. Create an **app-specific password** at [account.apple.com](https://account.apple.com).
2. Put the Apple ID and that password in `config.toml`.
3. Set `calendars = ["Family"]` (or whatever the shared calendar is called
   in Calendar.app). Family Sharing calendars show up over CalDAV.

Or publish a read-only webcal URL in `sources.ics_urls`.

## Birthdays

Birthdays are not read from a calendar. In `config.toml`:

```toml
birthdays = ["Maya,2018-03-15", "Sam,2015-11-02"]
```

Anyone whose next birthday is today or within two weeks is merged into
**Today** / **Coming next** as “Name turns N”, with a present icon.
Leap-day birthdays show on 28 February in non-leap years.

## Family to-dos

To-dos come from a shared [Todoist](https://todoist.com) project:

1. Create a project (for example `Family`) and invite the household.
2. Copy a **personal API token** from Todoist → Settings → Integrations → Developer.
3. Put the token and project name in `config.toml` under `[todoist]`.

Leave `todoist.token` empty to show the built-in demo list.

The shopping column is now **house temperatures**. Meross MS100
thermometer/hygrometers talk through the hub; MTS200 wall thermostats are
Wi-Fi devices on the same account. Local HTTP is signed with the account
key, so put the Meross app email and password in `config.toml` (`[meross]`).
The server logs in once, caches `meross-creds.json`, and reads
`Appliance.Hub.Sensor.All` (sensors), `Appliance.Control.Thermostat.Mode`
(MTS200), and `Appliance.Hub.Mts100.All` (hub TRVs) over MQTT — or LAN if
you set `hub_hosts`. A device named “Kitchen Thermostat” shows as Kitchen
(no humidity). Temperatures are rounded to the nearest degree and humidity
to the nearest 5% so the panel does not twitch every hour.

Never commit `config.toml` or `meross-creds.json` — they are gitignored.

## Weather

Forecast lives in the **Today** and **Coming next** section headers:
morning / afternoon / evening icons and temps, plus sunrise, sunset, and
pollen from [BBC Weather](https://www.bbc.co.uk/weather). Set
`weather.location_id` to the number in the location’s BBC URL
(`https://www.bbc.co.uk/weather/2643743` is London). Leave it empty for
demo icons. Past slots from earlier in the day are kept in `weather-cache.json`.
If the server starts after BBC has dropped those hours, morning uses
the day’s low and afternoon the high.

## Tube

The sidebar shows [TfL](https://api.tfl.gov.uk) status for Northern,
Circle, District, and Victoria. No API key is required. On fetch failure
the demo statuses are shown.

## Pico side

The [firmware](firmware/) is the Pico LiPo 2 XL W Embassy / Rust client:
USB-serial `wifi` / `psk` / `server` / `save` (same as the laser-tag
nodes), then `POST /api/frame.bin` and paint on 200. Sleep length comes back
on `X-Sleep-Seconds` from that mode’s `poll_interval_secs` or `wake-up` in
`config.toml` (shortened by a measured `pico_drift` so the low-power
oscillator still hits the intended wall-clock time). A timer poll within
10 minutes before that planned wake is treated as the wake itself, so the
Pico is not sent back for a few seconds or minutes. `make build` in
`firmware/` and drop `family-frame.uf2`
on the `RP2350` drive. Without the board, [`pico-sim`](pico-sim/) speaks
the same loop.
