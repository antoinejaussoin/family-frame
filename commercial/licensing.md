# Licensing

The licences on this code today, what they allow when selling a product,
and the decision still to make.

Plan stages: parked in **1**, decided before **5**. See
[`plan.md`](plan.md).

> This brief records facts and questions for a lawyer. It is not legal
> advice.

## Where we are

| Part | Licence | Source |
|---|---|---|
| Server, family UI, pico-sim, docs | GPL-3.0 | [`LICENSE`](../LICENSE) |
| Firmware | AGPL-3.0-or-later | [`firmware/NOTICES.md`](../firmware/NOTICES.md) |
| Panel driver inside the firmware | Rust port of AGPL-3.0-or-later C code (el133-pico-driver, tesserae-device-pico-bin) | [`firmware/NOTICES.md`](../firmware/NOTICES.md) |
| CYW43439 radio firmware blobs | Infineon permissive binary licence | [`firmware/cyw43-firmware/`](../firmware/cyw43-firmware/) |
| Fonts | SIL Open Font License 1.1 | [`server/static/fonts/TRMNL-README.md`](../server/static/fonts/TRMNL-README.md) |
| Rust and npm dependencies | Mostly MIT / Apache-2.0; not yet audited | `Cargo.lock`, `package-lock.json` |
| Chromium inside the Docker image | BSD-style plus many bundled licences | Docker image |

Authorship: the git history shows one human author (Antoine Jaussoin) plus
commits made by a coding agent on his behalf. No outside contributors so
far. Confirm before relying on it.

The repository is public on GitHub.

## Decided

- **2026-10-10** Decision parked. Making the repository private and
  changing the licence is an option.

## What the current licences allow

- **Selling is allowed.** GPL and AGPL do not forbid charging for
  hardware, software, setup, or support.
- **Distributing binaries brings duties.** The frame firmware and the
  module software ship inside devices we sell. Each buyer must get the
  corresponding source, or a written offer for it, plus the licence text.
- **Consumer devices and GPL v3.** For a “User Product” GPL v3 requires
  the installation information a buyer needs to run modified software on
  their device. Secure boot that only accepts our signed firmware
  conflicts with that unless buyers can install their own keys or images.
  This affects the update design in [`platform.md`](platform.md).
- **Cloud and AGPL.** Running GPL code on our own servers is not
  distribution, so the GPL server code in the cloud carries no source
  duty. AGPL code offered to users over a network does. The firmware is
  AGPL but runs on the frame, not in the cloud.

## What going private does and does not do

| Action | Effect |
|---|---|
| Make the GitHub repo private | Stops new copies from GitHub. Copies people already have keep their GPL/AGPL rights. |
| Relicense code Antoine wrote | Possible as sole copyright holder, for future versions. Past public versions stay GPL for whoever has them. |
| Relicense the panel driver port | **Not possible alone.** It derives from third-party AGPL code. Needs a licence from those authors or a clean replacement. |
| Ship a private fork of the firmware with the AGPL driver | Still bound by AGPL for that driver and whatever is combined with it. |

Replacing the driver: the init register values match Pimoroni’s
`inky_el133uf1.py`. A rewrite from the panel datasheet and permissively
licensed sources, by someone who documents the process, could free the
firmware. Check Pimoroni’s licence for that file first.

## Options to evaluate

| Option | Shape | Notes |
|---|---|---|
| A. Stay open | Keep GPL/AGPL; sell hardware, the cloud service, and support | Community and trust; competitors may build from the code; fits the Home frame privacy story |
| B. Open frame, separate cloud | Firmware and module open; cloud service a separate codebase not derived from the GPL server | Cloud must be written fresh or relicensed by the copyright holder |
| C. Relicense and go private | Stop publishing; replace the AGPL driver; new proprietary licence for new versions | Most control; loses the open-source story; driver rewrite first |
| D. Source-available | Publish under a licence that forbids competing commercial use for a time | Middle ground; not open source in the usual sense |

## Questions for a lawyer

1. With one human author and agent-made commits, is relicensing future
   versions straightforward?
2. Exactly which parts of the firmware are derived from the AGPL driver,
   and does replacing them free the rest?
3. How do we meet GPL v3 installation-information duties while signing
   updates and protecting buyers from malicious firmware?
4. What notices and source offers must ship in the box and on the
   module?
5. Can the fonts and radio blobs be redistributed in a commercial device
   as we do now?
6. Trade mark: is “Family Frame” available to register in the UK and EU?

## Work by stage

- **Stage 1** — This brief written; decision parked with a revisit date
  before Stage 5.
- **Stage 2** — Dependency licence audit (for example `cargo deny`,
  npm licence check). Decide whether these commercial briefs stay public.
- **Stage 3** — Pilot devices ship with licence notices and a source
  offer.
- **Stage 5** — Decision made with legal advice and applied; trade mark
  filed.
