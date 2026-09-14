# Family e-ink frame

An e-ink, battery-powered frame for the family.

A 13.3″ Spectra 6 panel in a picture frame. A **Pimoroni Pico LiPo 2 XL W**
wakes once an hour, downloads a packed image, and sleeps. A **Rust server**
on the LAN builds that image from HTML/CSS plus the family calendar,
to-dos, house temperatures, Tube status, and BBC weather in the section headers.

Hardware to buy is in [`shopping.md`](shopping.md).

## THIS IS A WORK IN PROGRESS

This is being worked on, not working yet.

## What you get

| Piece | Where |
|---|---|
| Hardware shopping list | [`shopping.md`](shopping.md) (list A: LiPo 2 XL W, or list B: Plus 2 W) |
| Wiring / stack | [`wiring.svg`](wiring.svg), [`connections.svg`](connections.svg) |
| Pico firmware (LiPo 2 XL W) | [`firmware/`](firmware/) |
| Rust server + layout simulator | [`server/`](server/) |
| Pico client simulator | [`pico-sim/`](pico-sim/) |

## Layout workflow

1. Edit [`server/templates/dashboard.html`](server/templates/dashboard.html) and
   [`server/static/dashboard.css`](server/static/dashboard.css).
2. Open `/preview` in a browser. The iframe is the real 1600×1200 panel.
3. On the LAN, Chromium screenshots `/dashboard`, the server dithers to
   Spectra 6, and the Pico GETs `/frame.bin`.
4. If the family data has not changed, the checksum matches and the Pico
   does **not** refresh the glass.

The dashboard HTML must not include a ticking clock. A changing “updated at”
would make every hour look like a new image.

## Run the server

Chrome or Chromium is required only for `/frame.bin` / `/frame.png`. The
HTML simulator works without it.

```bash
cd server
cp config.example.toml config.toml   # optional; demo data is the default
cargo run
```

While iterating locally, `--watch` rebuilds and restarts on source, template,
static, fixture, or config changes. Do not use it in production (Docker `CMD`
is the binary with no flags).

```bash
cargo run -- --watch
# or: make watch
```

Then open <http://127.0.0.1:8765/preview>.

### Docker

The image includes Google Chrome (amd64) or Chromium (arm64) so `/frame.bin` works.
Dashboard HTML/CSS/JS is compiled into the binary — deploy only needs `config.toml`.

On the Linux box, copy [`docker-compose.yml`](docker-compose.yml) and a `data/config.toml` (from [`server/config.example.toml`](server/config.example.toml)):

```bash
mkdir -p data
# edit data/config.toml
docker compose up -d
```

Then <http://<host>:8765/preview>. Meross login and BBC weather caches stay in `data/` next to the config.

Local one-off: `cd server && make docker-build && make docker-run`. Pushes to Docker Hub (`antoinejaussoin/family-frame-server`) happen from GitHub Actions on `main` (repo secrets `DOCKER_USERNAME` and `DOCKER_PASSWORD`, same as compta).

## Pretend to be the Pico

A separate crate polls `/frame.bin` the way the LiPo 2 XL W will: keep the last
checksum, skip a refresh on 304, and unpack a new frame to PNG on 200.

```bash
cd pico-sim
cargo run -- --url http://127.0.0.1:8765 --interval-secs 5
# or: make run
```

Each new frame is written as a timestamped PNG under `pico-sim/out/` (gitignored).

## Family calendar

Apple does not offer a public “Family Sharing API”. What works:

1. Create an **app-specific password** at [account.apple.com](https://account.apple.com).
2. Put the Apple ID and that password in `config.toml`.
3. Set `calendars = ["Family"]` (or whatever the shared calendar is called
   in Calendar.app). Family Sharing calendars show up over CalDAV.

Or publish a read-only webcal URL in `sources.ics_urls`.

## Family to-dos

To-dos come from a shared [Todoist](https://todoist.com) project:

1. Create a project (for example `Family`) and invite the household.
2. Copy a **personal API token** from Todoist → Settings → Integrations → Developer.
3. Put the token and project name in `config.toml` under `[todoist]`.

Leave `todoist.token` empty to show the built-in demo list.

The shopping column is now **house temperatures**. Meross MS100
thermometer/hygrometers have no Wi-Fi of their own: they talk through the
Meross hub. Local HTTP to that hub is signed with the account key, so put
the Meross app email and password in `config.toml` (`[meross]`). The
server logs in once, caches `meross-creds.json`, and reads
`Appliance.Hub.Sensor.All` over MQTT (or LAN if you set `hub_hosts`).
Temperatures are rounded to the nearest degree and humidity to the
nearest 5% so the panel does not twitch every hour.

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
nodes), then `GET /frame.bin` and paint on 200. `make build` in
`firmware/` and drop `family-frame.uf2` on the `RP2350` drive. Without
the board, [`pico-sim`](pico-sim/) speaks the same loop.
