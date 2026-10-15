# Experience

The buyer journey from the shop page to a frame on the wall, and what
happens when it goes wrong.

Plan stages: written in **2**, proven by pilots in **3**, both products
in **5**. See [`plan.md`](plan.md).

## Where we are

Today the builder:

1. Buys parts from several shops and solders headers
   ([`shopping.md`](../shopping.md)).
2. Checks and possibly swaps battery polarity.
3. Runs the Rust server with Chromium on a computer on the LAN, or via
   Docker ([`docs/setup.md`](../docs/setup.md)).
4. Flashes the Pico with a UF2 file while holding BOOTSEL.
5. Types `wifi`, `psk`, `server <ip:port>`, `save` into a USB serial
   terminal ([`firmware/README.md`](../firmware/README.md)).
6. Opens `/config` on the LAN to paste calendar links and tokens.
7. Hangs the frame. Charges it over USB-C.

Cold-boot colour fills are the only feedback on the glass: yellow (no
Wi-Fi saved), red (no PSRAM or Wi-Fi failed), blue (HTTP failed, no
previous frame).

## Target journey: Home frame

1. **Buy.** Shop page shows both products and a privacy comparison.
2. **Unbox.** Frame (charged to storage level), module, power supply,
   Ethernet cable, quick-start card.
3. **Plug in the module.** Ethernet to the router, power on. A status
   light shows it is ready.
4. **Open the setup page.** Scan the QR on the card. It opens the module
   on the LAN by a local name or discovery. No IP address typing.
5. **Create the household owner.** Set a password. No default
   credentials.
6. **Wake the frame.** Press a button on the back. The glass shows a
   setup screen with the next step written on it.
7. **Give the frame Wi-Fi.** The phone sends the Wi-Fi name and password
   to the frame. Candidate methods in [`platform.md`](platform.md):
   Bluetooth LE from the setup page, or a temporary frame hotspot.
8. **Pair.** The frame finds the module and they exchange keys. The
   setup page confirms “Frame connected”.
9. **Choose widgets.** Calendar link, weather location, to-do list,
   photos. Preview on the phone shows the real 1600×1200 layout.
10. **First paint.** The frame shows the board within a minute.
11. **Choose a schedule.** School-run wake, evening wake, weekends off.
    The UI shows the battery estimate that exists today on `/stats`.
12. **Hang.** Wall fixing in the box.
13. **Charge in six months.** The board shows a low-battery notice weeks
    ahead; the owner also gets one in the family UI.

## Target journey: Cloud frame

Same as above without steps 3–5. Instead:

- Scan the QR on the card and create an account, or sign in.
- The pairing code on the card claims the frame for that household.
- Steps 6 onward are the same; the frame talks to the cloud.

## What breaks, and what the buyer sees

| Problem | Today | Target |
|---|---|---|
| 5 GHz-only home Wi-Fi | Frame cannot join; red fill | Setup screen names 2.4 GHz as the cause; help page explains how to enable a 2.4 GHz band |
| Wrong Wi-Fi password | Red fill | Setup page reports the failure while the phone is still connected |
| Module unplugged or dead | Blue fill only if no earlier frame; otherwise old picture stays forever | Frame keeps the last picture and paints a small “not updated since …” stamp after a set number of failed wakes |
| Cloud outage | — | Same as module down; status page link in the family UI |
| Battery low | Visible on `/stats` only | On-glass notice and family UI alert, weeks ahead |
| Router replaced (new Wi-Fi name) | Re-flash over USB serial | Button-hold on the frame re-enters setup |
| Frame moved to another house | Same | Factory reset button sequence; ownership released in the family UI |
| Calendar link broken | Demo data or a missing block | Widget shows “calendar unavailable” and the UI says which link failed |
| Firmware update fails mid-way | Re-flash by hand | Two firmware slots; the frame falls back to the last good one |

## Open questions

1. Bluetooth LE or a temporary hotspot for Wi-Fi setup? BLE is smoother
   on phones; a hotspot needs no app. The radio on the Pico LiPo 2 XL W
   (CYW43439) can do both. Decision in [`platform.md`](platform.md).
2. Native phone app or web only? Web is cheaper; BLE setup from a web
   page works on Android Chrome and not in iOS Safari.
3. Does the module have its own display or just a light?
4. Who receives the low-battery alert, and how (email, push, only the
   UI)?
5. Should the frame wake on its own for urgent changes (school snow day)?
   The current design wakes only on schedule or on a button press.

## Work by stage

- **Stage 2** — This journey reviewed step by step against what the
  firmware and server can do; every gap filed in
  [`platform.md`](platform.md) or [`hardware.md`](hardware.md).
- **Stage 3** — Pilot households set up from the card alone. Record
  where each one got stuck.
- **Stage 5** — Cloud frame journey live; failure table fully handled.
