# Family Frame UI

Svelte 5 + Tailwind SPA: family home (mode, schedule, photos), layout
simulator (`/preview`), and Pico stats (`/stats`).

## Hot reload (recommended while iterating)

Run the Rust API and Vite as two processes. The SPA talks to the API through
Vite’s proxy, so `fetch('/api/…')` stays same-origin and HMR keeps working.

```bash
# terminal 1 — API + /dashboard (the 1600×1200 panel HTML)
cd server && cargo run --features watch -- --watch   # or: make watch

# terminal 2 — family UI (/, /preview, /stats)
cd server/ui && npm ci && npm run dev   # or: make ui-dev
```

Open <http://127.0.0.1:5173/>. Vite proxies `/api`, `/dashboard`, and `/static`
to `http://127.0.0.1:8765`. Override the target with `EINK_API` if the server
is bound elsewhere.

Vite listens on the LAN as well (`host: true`) so a phone can hit the Network
URL it prints.

## Production build

The Rust server serves `dist/` at `/` (Docker builds this automatically):

```bash
npm ci
npm run build   # → dist/
```
