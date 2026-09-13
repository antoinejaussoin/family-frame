# Notices

The panel driver (init sequence, 90° rotate/split, Spectra 6 D/C setup)
is a Rust port of C from:

- [el133-pico-driver](https://github.com/dmellok/el133-pico-driver) (AGPL-3.0-or-later)
- [tesserae-device-pico-bin](https://github.com/dmellok/tesserae-device-pico-bin) (AGPL-3.0-or-later)

Init register values for the EL133UF1 match Pimoroni’s
`inky_el133uf1.py`. On-chip PSRAM bring-up uses `embassy-rp::psram`
(APS6404L on QMI CS1 / GP47).

USB CDC provisioning (`wifi` / `psk` / `server` / `save`) matches the
laser-tag temperature-display and IR-capture Embassy nodes.

CYW43439 blobs in [`cyw43-firmware/`](cyw43-firmware/) come from
[embassy-rs/embassy](https://github.com/embassy-rs/embassy/tree/main/cyw43-firmware)
and are covered by the Infineon permissive binary license in that
directory.

This firmware directory is therefore **AGPL-3.0-or-later**. The rest of
the family-frame repository stays GPL-3.0 as in the root `LICENSE`.
