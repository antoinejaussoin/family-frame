# Adding a dashboard source

Four steps. Pixel math, HTML, and Chromium stay out of this folder.

## 1. Implement `DataSource`

Add `server/src/sources/you.rs` (or extend an existing module):

```rust
pub struct YouSource;

#[async_trait::async_trait]
impl DataSource for YouSource {
    fn id(&self) -> &'static str { "you" }

    fn enabled(&self, cfg: &Config) -> bool {
        cfg.sources.you.enabled /* or non-empty secrets */
    }

    fn when_disabled(&self, _cfg: &Config) -> DisabledBehaviour {
        DisabledBehaviour::Skip // or Demo
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> Result<SourceOutcome> {
        // HTTP/MQTT/parse only. Return a Contribution. Never touch Dashboard.
        Ok(SourceOutcome::live("you", Contribution::None))
    }

    fn demo(&self, ctx: &SourceContext<'_>) -> Option<Contribution> {
        let _ = ctx;
        None
    }
}
```

`SourceContext` already has timezone, today, a shared `reqwest` client,
and `config_dir` (for `weather-cache.json`, `meross-creds.json`).

Use `cache::TtlCache` and `filter::is_family_friendly` instead of a new
`static Mutex`.

## 2. Register it

In `all_sources()` (`mod.rs`), `Box::new(you::YouSource)` in the order
you want notes and calendar rows applied. Calendar contributors
**extend**; nothing may clear another source’s events. Demo calendar
runs once at the end if the merged list is empty.

## 3. Add `[sources.you]`

Declare the table in `config.rs` (`SourcesConfig`) and document it in
`config.example.toml`. Credential sources are enabled iff the secrets
are non-empty. Always-on sources (`tfl`, `jokes`, `history`, `saints`)
take `enabled = true` by default.

Legacy aliases (`[todoist]`, top-level `birthdays`, `[sources].ics_urls`)
exist for one release. Do not add new top-level keys.

## 4. Optional HTML slot

The template is a fixed grid. Reuse an existing slot (`sidebar.transit`
is already generic status lines) or add a section to
`templates/dashboard.html` + `static/dashboard.css`. Event flags
`birthday` / `school` / `recurring` / `all_day` are CSS classes, not
plugin renderers.

`fit_to_panel()` still owns the pixel budget. Disabling a source must
go through that function so leftover height refills to-dos and history.

## What not to do

- Do not fetch from `http.rs`, `frame.rs`, or the family SPA.
- Do not put secrets in the Svelte UI.
- Config `enabled = false` (or empty secrets) hides a slot. Cargo
  features (`pronote`, `meross`, `watch`) only shrink the binary.
