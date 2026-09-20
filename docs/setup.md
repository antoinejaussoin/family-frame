# Setup & development

Product overview and screenshots: [`README.md`](../README.md).
Hardware to buy: [`shopping.md`](../shopping.md).

A **Pimoroni Pico LiPo 2 XL W** is a dumb client: it POSTs battery
diagnostics, paints a 960 000-byte Spectra 6 frame, and sleeps. A **Rust
server** on the LAN fetches household datasources, renders HTML/CSS,
screenshots the panel with Chromium, and dithers it for the glass.

## What it shows

The 1600×1200 dashboard is a fixed grid, not a widget toolkit.

| Block | Source | Notes |
|---|---|---|
| Mast (weekday, day, month, saint) | `saints` | French civil calendar |
| Today / Next events | `calendar` (ICS) + `bins` + `birthdays` + Pronote hours | Weather icons sit in the headings |
| To do / joke | `todoist` / `jokes` | Same row; each a quarter of the 1600px panel |
| On this day / school week | `history` / `pronote` | Wikipedia facts above the Mon–Fri week grid (next week on Sat/Sun) |
| Transit | `tfl` | Default: Northern, Circle, District, Victoria |
| House | `meross` | Demo rooms if no credentials |
| Battery / next wake | Pico POST | Hidden until the Pico has reported |

Homework and grades have markup but stay off the glass unless
`sources.pronote.show_sections = true`. School **hours** still appear as
calendar rows (`School: Léa (finishes at 16:30)`).

## Two modes

The same device shows the family dashboard or a rotating photo.

Switch from the family UI (trusted LAN — no auth):

- **Dashboard** vs **picture** mode
- Poll interval or **per-weekday** wake-up times, stored **per mode**
- Upload landscape photos (under `pictures/` next to the config)
- Choose the rotation and preview the dithered Spectra 6 look

The family UI **Setup** page (`/config`) writes household settings into
`config.toml`: frame name, timezone, battery size, calendar links,
Wandsworth bin UPRN, birthdays, Todoist, and BBC weather. Pronote,
Meross, and Tube lines still live in the file. Trusted LAN — no auth.

## Repository map

| Piece | Where |
|---|---|
| Hardware shopping list | [`shopping.md`](../shopping.md) |
| Wiring / stack | [`wiring.svg`](../wiring.svg), [`connections.svg`](../connections.svg) |
| Pico firmware (LiPo 2 XL W) | [`firmware/`](../firmware/) |
| Rust server + family UI + layout simulator | [`server/`](../server/) |
| Family SPA (Svelte) | [`server/ui/`](../server/ui/) |
| Pico client simulator | [`pico-sim/`](../pico-sim/) |
| How to add a datasource | [`server/src/sources/README.md`](../server/src/sources/README.md) |

## How the pieces talk

```
config.toml
  → sources::load_dashboard()     registry of DataSource impls
  → Dashboard + fit_to_panel()
  → Minijinja dashboard.html
  → Chromium 1600×1200 screenshot
  → Floyd–Steinberg → Spectra 6 .bin
  → FrameCache (10s memory + disk checksum skip)
  → POST /api/frame.bin
```

The Pico never sees calendars, passwords, or HTML. It POSTs to
`/api/frame.bin`, paints on **200**, skips the glass on **204**, and
sleeps for `X-Sleep-Seconds`. Protocol: [`firmware/PROTOCOL.md`](../firmware/PROTOCOL.md).
Hole-by-hole stack: [`wiring.svg`](../wiring.svg).

## Quick start (no hardware)

Chrome or Chromium is required only for dashboard `/api/frame.bin` /
`/api/frame.png`. Picture mode and `/preview` work without it.

```bash
cd server
cp config.example.toml config.toml   # optional; demo data is the default
cargo run                            # or: make serve
make fake                            # screenshot mode (see below)
```

The family SPA is optional for the API. Without `ui/dist`, `/` explains
how to start Vite; `/dashboard` and `/api/*` still work. `make` (no
target) builds the SPA first, then runs the server.

### Iterate with hot reload

```bash
# terminal 1 — Rust API (rebuilds on src / templates / CSS)
cd server && cargo run --features watch -- --watch   # or: make watch

# terminal 2 — family UI
cd server/ui && npm ci && npm run dev   # or: make ui-dev
```

`--watch` is local only and needs the `watch` Cargo feature
(`make watch` passes it). Docker `CMD` is the binary with no flags.
Do not watch `config.toml` (the family UI edits that live) or `ui/`
(use Vite).

Open <http://127.0.0.1:5173/>. If the server is not on `:8765`, set
`EINK_API` (for example `EINK_API=http://127.0.0.1:9000 npm run dev`).

Then <http://127.0.0.1:5173/> (Vite) or <http://127.0.0.1:8765/> (built
SPA), `/preview`, `/stats`, `/config`, `/dashboard`.

### Screenshot mode

`make fake` (or `cargo run -- --fake`, or `FAMILY_FRAME_FAKE=1`) starts
the server with a real `config.toml` but **does not fetch household
data**. Calendar / ICS, Todoist, Pronote, Meross, and birthdays use the
built-in demo payloads. Weather, Tube, jokes, history, and saints stay
live. Open `/dashboard` or `/preview` and shoot.

### Serve the built SPA from the API

```bash
cd server/ui && npm ci && npm run build
```

Then <http://127.0.0.1:8765/> is the family UI.

## Configure sources

On the LAN, open `/config` and save. That writes `[sources.*]` in
`config.toml`. You can still edit the file by hand.

Copy [`server/config.example.toml`](../server/config.example.toml). Each
datasource is a `[sources.<id>]` table. Legacy `[todoist]` / `[weather]`
/ `[meross]` / `[pronote]`, top-level `birthdays`, and `[sources].ics_urls`
still load for one release. A leftover `[icloud]` table is ignored.

### Calendar (ICS)

Public ICS URLs only. There is no iCloud / CalDAV client. The Setup
page walks through publishing an iCloud Family calendar.

1. In Calendar.app (or Google Calendar, Fastmail, …) publish the family
   calendar as a **read-only** webcal / ICS link.
2. Paste that URL on `/config`, or put it in `sources.calendar.ics_urls`.
   `webcal://` becomes `https://`.

**To remove this source:** leave `ics_urls` empty. If nothing else
contributes events (no birthdays, no Pronote hours), the built-in demo
calendar is shown.

### Birthdays

```toml
[sources.birthdays]
people = ["Maya,2018-03-15", "Sam,2015-11-02"]
```

Anyone whose next birthday is today or within two weeks is merged as
“Name turns N”, with a present icon. Leap-day birthdays show on 28
February in non-leap years.

**To remove this source:** set `people = []`.

### Todoist

1. Create a project (for example `Family`) and invite the household.
2. Copy a **personal API token** from Todoist → Settings → Integrations → Developer.
3. Put the token and project name under `[sources.todoist]`.

**To remove this source:** leave `token` empty (demo to-dos on a stock
board / in-process default).

### Meross (house temperatures)

MS100 thermometer/hygrometers talk through the hub; MTS200 wall
thermostats are Wi-Fi devices on the same account.

1. Put the Meross app email and password under `[sources.meross]`.
2. The server logs in once, caches `meross-creds.json`, and reads
   sensors over MQTT (or LAN if you set `hub_hosts`).
3. A device named “Kitchen Thermostat” shows as Kitchen (suffix strip).
   Unlabeled model names fall back to **Thermostat**. Override with
   `[sources.meross.labels]`.

Temperatures are shown to one decimal place (`21.4°`) and humidity to
the nearest 1% so the painted values match the code.

**To remove this source:** leave email/password empty.

### Weather

Forecast lives in the **Today** and **Coming next** headings: morning /
afternoon / evening icons and temps, plus sunrise, sunset, and pollen
from [BBC Weather](https://www.bbc.co.uk/weather). Set
`sources.weather.location_id` to the number in the location’s BBC URL
(`https://www.bbc.co.uk/weather/2643743` is London). A missing table or
empty id does **not** fetch London; the example config keeps London so
documented demos still work. Past slots from earlier in the day are
kept in `weather-cache.json`.

**To remove this source:** `location_id = ""`.

### Tube

[TfL](https://api.tfl.gov.uk) status. No API key. Lines and colours are
config; the default is Northern / Circle / District / Victoria.

**To remove this source:** `sources.tfl.enabled = false`.

### Jokes / history / saints

Jokes, Wikipedia “On this day”, and saints stay on by default. Facts sit
above the school week; leftover height decides how many fit.

**To remove a joke / history / saint:** `sources.jokes.enabled = false`
(same for `history` and `saints`). An empty slot collapses the same way
`no-joke` / `no-history` already do.

### School (Pronote)

Unofficial session protocol (the flow documented by
[pronotepy](https://github.com/bain3/pronotepy)). ENT / EduConnect is
not supported.

1. Open the **direct** Pronote space (`eleve.html` or `parent.html`).
2. Put URL, username, and password under `[sources.pronote]`. Set
   `student` to the child’s first name. For a parent account set
   `account = "parent"` and optionally `child = "Firstname"`.
3. If Pronote asks for a PIN, set `pin`.
4. `show_sections = false` keeps homework/grades off the glass; hours
   still merge when Pronote is live. The Monday–Friday subject grid
   always paints when the timetable is available (next week on
   Saturday and Sunday).

**To remove this source:** leave `url` empty on a real `config.toml`.
That does **not** invent Léa’s school day. The in-process default (no
config file) still uses the demo school profile so `cargo run` without
a file looks populated.

## Layout workflow

1. Edit [`server/templates/dashboard.html`](../server/templates/dashboard.html)
   and [`server/static/dashboard.css`](../server/static/dashboard.css).
2. Open `/preview`. The iframe is the real 1600×1200 panel.
3. On the LAN, Chromium screenshots `/dashboard`, the server dithers to
   Spectra 6, and the Pico POSTs `/api/frame.bin`.
4. If the new bitmap matches the last checksum, the Pico does **not**
   refresh the glass. Open `/stats` for battery history and every poll.

## Docker / versioning

The image includes Google Chrome (amd64) or Chromium (arm64) and the
built family UI. Deploy only needs `config.toml` (and optional photos
under `data/pictures/`).

```bash
mkdir -p data
# copy server/config.example.toml → data/config.toml and edit
docker compose up -d
```

Then <http://\<host\>:8765/>, `/preview`, `/stats`, `/config`. Meross login, BBC
weather caches, uploaded photos, and Pico poll history stay in `data/`.

Local one-off: `cd server && make docker-build && make docker-run`.
Pushes to Docker Hub (`antoinejaussoin/family-frame-server`) happen from
GitHub Actions on `main` (repo secrets `DOCKER_USERNAME` and
`DOCKER_PASSWORD`). Images are tagged `latest` and with [`VERSION`](../VERSION).

The version is a single line in `VERSION`. That is the only file to edit
when you cut a release — Cargo.toml, package.json, and Docker labels are
filled in at **build time**.

1. Change `VERSION` (for example `0.1.0` → `0.2.0`) in a PR.
2. Merge to `main`.
3. CI builds `:0.2.0` and `:latest`, and creates git tag `v0.2.0` if it
   does not already exist.

Locally, `eink-frame --version`, `GET /health`, and Stats all read the
same value.

## Pico / shopping / flashing

Buy list: [`shopping.md`](../shopping.md). Firmware:
[`firmware/README.md`](../firmware/README.md). USB-serial `wifi` / `psk` /
`server` / `save`, then `POST /api/frame.bin` and paint on 200. Sleep
length comes back on `X-Sleep-Seconds` from that mode’s
`poll_interval_secs` or `wake-up` (shortened by measured `pico_drift` and
`pico_overhead_secs`).
`make build` in `firmware/` and drop `family-frame.uf2` on the `RP2350`
drive.

Without the board, [`pico-sim`](../pico-sim/) speaks the same loop:

```bash
cd pico-sim
cargo run -- --url http://127.0.0.1:8765 --drain
```

Sleep length comes from `X-Sleep-Seconds`. Each new frame is written as
a timestamped PNG under `pico-sim/out/` (gitignored).

## Forking this into your own house

1. Copy `server/config.example.toml` → `config.toml`. Do not commit it.
2. Publish a family ICS URL; drop iCloud leftovers.
3. Turn off sources you do not use (`enabled = false` or empty secrets).
4. Delete or ignore firmware pieces you are not flashing.
5. To **add** a source: implement `DataSource`, register it in
   `all_sources()`, add `[sources.you]`, and (if you need a new slot) a
   section in `dashboard.html`. Four steps:
   [`server/src/sources/README.md`](../server/src/sources/README.md).
6. Optional Cargo features: default builds include `pronote` and `meross`.
   `watch` is off unless you pass `--features watch`. Use
   `--no-default-features` to compile without Pronote crypto or Meross MQTT.

## Security note

Trusted LAN only. There is **no auth** on the family UI or the Pico
endpoint. Never commit `config.toml` or `meross-creds.json` — they are
gitignored.
