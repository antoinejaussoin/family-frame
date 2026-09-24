<script>
  import { onMount } from 'svelte'
  import FrameMark from '../lib/FrameMark.svelte'
  import PageNav from '../lib/PageNav.svelte'
  import { formatDuration, intervalToDuration } from 'date-fns'
  import { deleteDebug, getDebug } from '../lib/api.js'

  const builtVersion = import.meta.env.APP_VERSION
  let page = $state(null)
  let pollPage = $state(1)
  let error = $state('')
  let loading = $state(true)
  let clearing = $state(false)
  let fetchGen = 0
  let driftTip = $state(null)

  async function refresh() {
    const gen = ++fetchGen
    const requested = pollPage
    try {
      const next = await getDebug(requested)
      if (gen !== fetchGen) return
      page = next
      if (next?.page) pollPage = next.page
      error = ''
    } catch (e) {
      if (gen !== fetchGen) return
      error = e.message || String(e)
    } finally {
      if (gen === fetchGen) loading = false
    }
  }

  async function goPage(n) {
    pollPage = n
    await refresh()
    document.getElementById('pico-requests')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  async function clearHistory() {
    if (
      !confirm(
        'Are you sure you want to delete all Pico poll history and stored frames? This cannot be undone.',
      )
    ) {
      return
    }
    clearing = true
    error = ''
    try {
      await deleteDebug()
      pollPage = 1
      await refresh()
    } catch (e) {
      error = e.message || String(e)
    } finally {
      clearing = false
    }
  }

  onMount(() => {
    refresh()
    const id = setInterval(refresh, 10_000)
    return () => clearInterval(id)
  })

  const etaClass = {
    ok: 'text-sage',
    usb: 'text-muted',
    wait: 'text-muted',
    stable: 'text-muted',
    dead: 'text-terracotta-dark',
    empty: 'text-muted',
  }

  const confClass = {
    low: 'debug-pill-low',
    ok: 'debug-pill-ok',
    high: 'debug-pill-high',
  }

  const WEEK = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
  // XL W white power LED on 3V3 (hardwired). Floor is POWMAN + regulator leftover.
  const POWER_LED_MA = 1.05
  const IDLE_WITHOUT_LED_FLOOR_MA = 0.35
  let simWakes = $state(null)
  let simLed = $state(true)

  const bat = $derived(page?.battery)
  const scheduleWakes = $derived(bat?.wakes_per_day_avg ?? 12)
  const sliderWakes = $derived(simWakes ?? scheduleWakes)
  const sliderMax = $derived(Math.max(48, Math.ceil(scheduleWakes)))
  const weekPeak = $derived(Math.max(1, ...(bat?.wakes_by_day ?? [1])))
  const soc = $derived(page?.last_pct ?? bat?.soc_pct ?? 0)
  const socLow = $derived(soc < 25)
  const simLedShare = $derived(bat ? ledShareMa(bat.idle_ma) : 0)
  const simIdleMa = $derived(bat ? idleForSim(bat.idle_ma, simLed) : 0)
  const simDailyMah = $derived(bat ? simIdleMa * 24 + sliderWakes * bat.cycle_mah : 0)

  function fmtInt(n) {
    if (n == null || Number.isNaN(n)) return '—'
    return Math.round(n).toLocaleString('en-GB')
  }

  function fmtMah(n) {
    if (n == null || Number.isNaN(n)) return '—'
    if (Math.abs(n) >= 100) return Math.round(n).toLocaleString('en-GB')
    return n.toFixed(1)
  }

  function fmtWakes(n) {
    if (n == null) return '—'
    return Number.isInteger(n) || Math.abs(n - Math.round(n)) < 0.05
      ? String(Math.round(n))
      : n.toFixed(1)
  }

  function fmtEtaShort(secs) {
    if (!Number.isFinite(secs) || secs <= 0) return '—'
    return (
      formatDuration(intervalToDuration({ start: 0, end: Math.round(secs) * 1000 }), {
        format: ['years', 'months', 'days'],
      }) || '—'
    )
  }

  function lifeLabel(kind, secs) {
    if (kind === 'usb') return 'On USB'
    if (kind === 'dead') return 'Empty'
    if (kind === 'stable') return 'Holding'
    if (kind === 'empty') return '—'
    return fmtEtaShort(secs)
  }

  function lifeHint(kind, fallback) {
    if (kind === 'usb') return 'discharge estimate paused'
    if (kind === 'dead') return 'battery looks empty'
    if (kind === 'stable') return 'not draining in recent samples'
    return fallback
  }

  function ledShareMa(idleMa) {
    if (!Number.isFinite(idleMa)) return 0
    return Math.min(POWER_LED_MA, Math.max(0, idleMa - IDLE_WITHOUT_LED_FLOOR_MA))
  }

  function idleForSim(idleMa, ledOn) {
    return ledOn ? idleMa : idleMa - ledShareMa(idleMa)
  }

  function simLifeSecs(mah, daily) {
    if (!Number.isFinite(mah) || !Number.isFinite(daily) || mah <= 1 || daily <= 0.05) return 0
    return Math.round((mah / daily) * 86400)
  }

  function fmtSleep(s) {
    if (!s) return ''
    if (s < 90) return `${s}s`
    if (s < 3600) return `${Math.round(s / 60)} min`
    const h = s / 3600
    if (Math.abs(h - Math.round(h)) < 0.05) return `${Math.round(h)} h`
    return `${h.toFixed(1)} h`
  }

  const DRIFT_SPAN_PCT = 5

  function driftPct(p) {
    if (p && typeof p.pico_drift === 'number' && Number.isFinite(p.pico_drift)) {
      return p.pico_drift * 100
    }
    const label = p?.pico_drift_label || ''
    const m = label.match(/([\d.]+)%\s+(slow|fast)/i)
    if (!m) return 0
    const n = Number(m[1])
    return m[2].toLowerCase() === 'slow' ? n : -n
  }

  function onDriftPointer(event) {
    const hit = event.target?.closest?.('[data-drift]')
    if (!hit) {
      driftTip = null
      return
    }
    const box = event.currentTarget.getBoundingClientRect()
    driftTip = {
      text: hit.dataset.drift,
      x: event.clientX - box.left,
      y: event.clientY - box.top,
    }
  }

  function driftWord(pct) {
    if (Math.abs(pct) < 0.05) return 'on time'
    return pct > 0 ? 'slow' : 'fast'
  }

  function driftNeedle(pct) {
    const t = Math.max(-1, Math.min(1, pct / DRIFT_SPAN_PCT))
    // Slow (positive) on the left, fast (negative) on the right.
    const theta = ((t + 1) / 2) * Math.PI
    const cx = 60
    const cy = 58
    const r = 40
    return {
      t,
      x2: cx + r * Math.cos(theta),
      y2: cy - r * Math.sin(theta),
    }
  }

  const drift = $derived(driftPct(page))
  const driftHint = $derived(driftWord(drift))
  const needle = $derived(driftNeedle(drift))
  const driftTone = $derived(
    Math.abs(drift) < 0.05 ? 'text-muted' : drift > 0 ? 'text-sage' : 'text-terracotta-dark',
  )
</script>

{#snippet pager()}
  {#if page?.page_count > 1}
    <nav class="flex items-center justify-between gap-3" aria-label="Pico request pages">
      <button
        type="button"
        class="btn btn-ghost"
        disabled={page.page <= 1}
        onclick={() => goPage(page.page - 1)}
      >
        Previous
      </button>
      <p class="text-sm font-semibold text-muted">
        Page {page.page} of {page.page_count}
      </p>
      <button
        type="button"
        class="btn btn-ghost"
        disabled={page.page >= page.page_count}
        onclick={() => goPage(page.page + 1)}
      >
        Next
      </button>
    </nav>
  {/if}
{/snippet}

<main class="mx-auto max-w-3xl px-4 py-8 sm:px-6 lg:py-12">
  <header class="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
    <div class="flex items-center gap-4">
      <FrameMark size={52} />
      <div>
        <p
          class="font-display text-base text-terracotta italic"
          style="font-variation-settings: 'opsz' 72"
        >
          Family Frame
        </p>
        <h1
          class="font-display text-3xl font-semibold tracking-tight text-ink sm:text-4xl"
          style="font-variation-settings: 'opsz' 96"
        >
          Stats
        </h1>
        {#if page?.version || builtVersion}
          <p class="mt-1 text-sm font-semibold text-muted">v{page?.version || builtVersion}</p>
        {/if}
      </div>
    </div>
    <PageNav />
  </header>

  {#if error}
    <div
      class="mb-6 rounded-2xl border border-terracotta/30 bg-[#fff1ea] px-4 py-3 text-sm font-semibold text-terracotta-dark"
      role="alert"
    >
      {error}
    </div>
  {/if}

  {#if loading && !page}
    <p class="font-semibold text-muted">Loading Pico history…</p>
  {:else if page?.has_polls}
    <section class="card mb-5 p-5 sm:p-6" aria-label="Battery">
      <div class="mb-4 flex items-start justify-between gap-3">
        <h2 class="text-xs font-extrabold tracking-wide text-muted uppercase">Battery</h2>
        {#if bat?.confidence}
          <span class="debug-pill {confClass[bat.confidence] || 'debug-pill-ok'}">
            {bat.confidence} confidence
          </span>
        {/if}
      </div>

      <p class="font-display text-7xl leading-none tracking-tight text-ink">
        {soc}<span class="text-3xl text-muted">%</span>
      </p>
      <div
        class="debug-meter mt-4 {socLow ? 'low' : ''}"
        role="meter"
        aria-label="Battery charge"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={soc}
      >
        <i style="width: {soc}%"></i>
      </div>
      <p class="mt-3 text-lg font-semibold tabular-nums text-ink">
        {fmtMah(bat?.remaining_mah)} mAh
        <span class="text-muted">of {fmtInt(bat?.capacity_mah)} mAh</span>
      </p>
      <p class="mt-0.5 text-sm font-semibold text-muted">
        {fmtInt(page.last_mv)} mV
        {#if bat}
          <span> · Pico linear {bat.linear_pct}%</span>
        {/if}
      </p>

      {#if bat}
        <div class="debug-stats mt-5">
          <div class="debug-stat">
            <p class="debug-stat-label">From now</p>
            <p class="debug-stat-value {etaClass[bat.eta_kind] || 'text-ink'}">
              {lifeLabel(bat.eta_kind, bat.eta_seconds)}
            </p>
            <p class="debug-stat-hint">{lifeHint(bat.eta_kind, 'at this week’s schedule')}</p>
          </div>
          <div class="debug-stat">
            <p class="debug-stat-label">From a full charge</p>
            <p class="debug-stat-value {etaClass[bat.eta_kind] || 'text-ink'}">
              {lifeLabel(bat.eta_kind, bat.full_eta_seconds)}
            </p>
            <p class="debug-stat-hint">{lifeHint(bat.eta_kind, 'if the pack were at 100%')}</p>
          </div>
        </div>

        <div class="debug-stats debug-stats-3 mt-3">
          <div class="debug-stat">
            <p class="debug-stat-label">Idle</p>
            <p class="debug-stat-value">{bat.idle_ma.toFixed(2)} mA</p>
            <p class="debug-stat-hint">{fmtMah(bat.idle_mah_per_day)} mAh/day doing nothing</p>
          </div>
          <div class="debug-stat">
            <p class="debug-stat-label">Per wake</p>
            <p class="debug-stat-value">{fmtMah(bat.cycle_mah)} mAh</p>
            <p class="debug-stat-hint">
              {#if bat.split_refresh}
                {fmtMah(bat.wake_mah)} radio + {fmtMah(bat.refresh_mah)} when it paints
              {:else}
                radio and panel together
              {/if}
            </p>
          </div>
          <div class="debug-stat">
            <p class="debug-stat-label">Daily use</p>
            <p class="debug-stat-value">{fmtMah(bat.schedule_mah_per_day)} mAh</p>
            <p class="debug-stat-hint">{fmtWakes(scheduleWakes)} wake-ups/day on the configured schedule</p>
          </div>
        </div>

        {#if bat.wakes_by_day?.length}
          <div class="week-bars mt-5" aria-label="Wake-ups by weekday">
            {#each bat.wakes_by_day as n, i}
              <div class="week-bar">
                <div class="week-bar-col">
                  <i style="height: {(n / weekPeak) * 100}%"></i>
                </div>
                <span class="week-bar-n">{fmtWakes(n)}</span>
                <span class="week-bar-d">{WEEK[i]}</span>
              </div>
            {/each}
          </div>
        {/if}

        <p class="mt-4 text-sm font-semibold text-muted">{bat.model_note}</p>
      {/if}
    </section>

    {#if bat && !bat.on_usb}
      <section class="card mb-5 p-5 sm:p-6" aria-label="Simulate wake-ups">
        <h2 class="text-xs font-extrabold tracking-wide text-muted uppercase">Simulate</h2>
        <label class="mt-3 block" for="wake-sim">
          <span class="text-xs font-extrabold tracking-wide text-muted uppercase">
            Wake-ups per day
          </span>
        </label>
        <p class="mt-0.5 font-display text-3xl font-semibold tabular-nums tracking-tight">
          {fmtWakes(sliderWakes)}
        </p>
        {#if Math.abs(sliderWakes - scheduleWakes) > 0.05}
          <p class="text-sm font-semibold text-muted">
            Schedule is {fmtWakes(scheduleWakes)}
          </p>
        {/if}
        <input
          id="wake-sim"
          class="wake-sim"
          type="range"
          min="0.5"
          max={sliderMax}
          step="0.5"
          value={sliderWakes}
          oninput={(e) => {
            simWakes = Number(e.currentTarget.value)
          }}
        />
        <p class="mt-5 text-xs font-extrabold tracking-wide text-muted uppercase">Power LED</p>
        <div class="seg mt-2" role="group" aria-label="Power LED">
          <button
            type="button"
            class={simLed ? 'on' : ''}
            aria-pressed={simLed}
            onclick={() => (simLed = true)}
          >
            On
          </button>
          <button
            type="button"
            class={!simLed ? 'on' : ''}
            aria-pressed={!simLed}
            onclick={() => (simLed = false)}
          >
            Off
          </button>
        </div>
        <p class="mt-2 text-sm font-semibold text-muted">
          {#if simLed}
            As fitted — {simIdleMa.toFixed(2)} mA idle includes the white LED
          {:else if simLedShare > 0.02}
            Cut-trace estimate — {simIdleMa.toFixed(2)} mA idle (−{simLedShare.toFixed(2)} mA)
          {:else}
            Idle is already at the no-LED floor
          {/if}
        </p>
        <div class="debug-stats debug-stats-3 mt-4">
          <div class="debug-stat">
            <p class="debug-stat-label">From now</p>
            <p class="debug-stat-value">{fmtEtaShort(simLifeSecs(bat.remaining_mah, simDailyMah))}</p>
            <p class="debug-stat-hint">at {fmtWakes(sliderWakes)} wake-ups/day</p>
          </div>
          <div class="debug-stat">
            <p class="debug-stat-label">From a full charge</p>
            <p class="debug-stat-value">{fmtEtaShort(simLifeSecs(bat.capacity_mah, simDailyMah))}</p>
            <p class="debug-stat-hint">{fmtMah(simDailyMah)} mAh/day in this simulation</p>
          </div>
          <div class="debug-stat">
            <p class="debug-stat-label">Idle</p>
            <p class="debug-stat-value">{simIdleMa.toFixed(2)} mA</p>
            <p class="debug-stat-hint">{simLed ? 'LED still wired' : 'LED trace cut'}</p>
          </div>
        </div>
      </section>
    {/if}

    <section class="card mb-5 p-5 sm:p-6" aria-label="Pico">
      <h2 class="mb-4 text-xs font-extrabold tracking-wide text-muted uppercase">Pico</h2>
      <div class="debug-stats">
        <div class="debug-stat">
          <p class="debug-stat-label">Last seen</p>
          <p class="debug-stat-value">{page.last_seen_rel}</p>
          <p class="debug-stat-hint">{page.last_seen}</p>
        </div>
        {#if page.has_next_refresh}
          <div class="debug-stat">
            <p class="debug-stat-label">Next refresh</p>
            <p class="debug-stat-value">{page.next_refresh_rel}</p>
            <p class="debug-stat-hint">{page.next_refresh}</p>
          </div>
        {/if}
      </div>
      <div class="debug-stat mt-3" aria-label="Pico timer drift">
        <div class="flex flex-wrap items-end justify-between gap-3">
          <div>
            <p class="debug-stat-label">Pico timer</p>
            <p class="debug-stat-value {driftTone}">
              {Math.abs(drift) < 0.05 ? '0%' : `${Math.abs(drift).toFixed(1)}%`}
            </p>
            <p class="debug-stat-hint">
              {driftHint}{#if (page.pico_overhead_secs ?? 0) >= 1}
                · {Math.round(page.pico_overhead_secs)}s wake{/if}
            </p>
          </div>
          <div class="drift-gauge-wrap">
            <svg
              class="drift-gauge"
              viewBox="0 0 120 72"
              role="meter"
              aria-label="Pico timer drift"
              aria-valuemin={-DRIFT_SPAN_PCT}
              aria-valuemax={DRIFT_SPAN_PCT}
              aria-valuenow={Number(drift.toFixed(2))}
              aria-valuetext={Math.abs(drift) < 0.05 ? 'on time' : `${Math.abs(drift).toFixed(1)} percent ${driftHint}`}
            >
              <defs>
                <linearGradient id="debug-drift-grad" x1="0" y1="0" x2="1" y2="0">
                  <stop offset="0" stop-color="#4f8f68" />
                  <stop offset="0.5" stop-color="#c4b8a8" />
                  <stop offset="1" stop-color="#d4654a" />
                </linearGradient>
              </defs>
              <path
                d="M 16 58 A 44 44 0 0 1 104 58"
                fill="none"
                stroke="rgba(58, 42, 36, 0.08)"
                stroke-width="10"
                stroke-linecap="round"
              />
              <path
                d="M 16 58 A 44 44 0 0 1 104 58"
                fill="none"
                stroke="url(#debug-drift-grad)"
                stroke-width="7"
                stroke-linecap="round"
              />
              <line x1="60" y1="58" x2="60" y2="16" stroke="rgba(58, 42, 36, 0.18)" stroke-width="1.5" />
              <line
                x1="60"
                y1="58"
                x2={needle.x2}
                y2={needle.y2}
                class={needle.t > 0.02 ? 'drift-needle-pos' : needle.t < -0.02 ? 'drift-needle-neg' : 'drift-needle-zero'}
                stroke-width="2.5"
                stroke-linecap="round"
              />
              <circle cx="60" cy="58" r="3.6" fill="var(--color-ink)" />
            </svg>
            <div class="drift-gauge-scale">
              <span class="pos">slow</span>
              <span>0</span>
              <span class="neg">fast</span>
            </div>
          </div>
        </div>
      </div>
      <div class="debug-stats debug-stats-3 mt-3">
        <div class="debug-stat">
          <p class="debug-stat-label">Power</p>
          <p class="debug-stat-value">{page.power_label}</p>
          <p class="debug-stat-hint">
            <span class="debug-pill {page.last_usb ? 'debug-pill-usb' : 'debug-pill-batt'}">
              {page.last_usb ? 'charging' : 'on pack'}
            </span>
          </p>
        </div>
        <div class="debug-stat">
          <p class="debug-stat-label">Last poll</p>
          <p class="debug-stat-value">{page.last_status}</p>
          <p class="debug-stat-hint">
            {page.last_status_label} · {page.last_wake}{#if page.last_sleep_s}
              {' · '}sleep {fmtSleep(page.last_sleep_s)}{/if}
          </p>
        </div>
        <div class="debug-stat">
          <p class="debug-stat-label">Storage</p>
          <p class="debug-stat-value">{page.debug_dir_label}</p>
          <p class="debug-stat-hint">debug log on disk</p>
        </div>
      </div>
      <button
        type="button"
        class="btn btn-ghost mt-5 text-terracotta-dark"
        disabled={clearing}
        onclick={clearHistory}
      >
        {clearing ? 'Deleting…' : 'Delete history'}
      </button>
    </section>

    {#if page.graph_svg}
      <section class="card mb-5 p-5 sm:p-6" aria-label="Battery over time">
        <h2 class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">
          Battery over time
        </h2>
        <div class="battery-graph">
          {@html page.graph_svg}
        </div>
        <p class="mt-3 text-sm font-semibold text-muted">
          Filled dots are on battery. Hollow dots are USB. Dashed line is a drain estimate.
        </p>
      </section>
    {/if}

    {#if page.drift_graph_svg}
      <section class="card mb-5 p-5 sm:p-6" aria-label="Wake error over time">
        <h2 class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">
          Wake error over time
        </h2>
        <div
          class="drift-graph"
          role="group"
          aria-label="Wake error chart"
          onpointermove={onDriftPointer}
          onpointerleave={() => (driftTip = null)}
        >
          {@html page.drift_graph_svg}
          {#if driftTip}
            <p class="drift-tip" style="left: {driftTip.x}px; top: {driftTip.y}px">{driftTip.text}</p>
          {/if}
        </div>
        <p class="mt-3 text-sm font-semibold text-muted">
          Each timer wake, early or late versus its scheduled slot, as a percent of the interval.
          Positive is late. This should settle toward zero as sleep compensation catches the clock.
        </p>
      </section>
    {/if}

    <section id="pico-requests" class="card p-5 sm:p-6" aria-label="Pico requests">
      <h2 class="mb-1 text-xs font-extrabold tracking-wide text-muted uppercase">Pico requests</h2>
      <p class="mb-3 text-sm font-semibold text-muted">
        {page.poll_count} poll{page.poll_count === 1 ? '' : 's'}
      </p>
      {@render pager()}
      <ol class="m-0 list-none p-0 {page.page_count > 1 ? 'mt-3' : ''}">
        {#each page.polls as p}
          <li class="border-t border-ink/10 py-4 first:border-t-0 first:pt-0">
            <div class="mb-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm font-semibold text-muted">
              <time class="font-extrabold text-ink">{p.when}</time>
              <span
                class="debug-pill {p.status === 200
                  ? 'debug-pill-high'
                  : p.status === 204 || p.status === 304
                    ? 'debug-pill-ok'
                    : 'debug-pill-low'}"
              >
                {p.status_label}
              </span>
              <span class="tabular-nums">
                {p.pct}% · {fmtInt(p.mv)} mV · {p.usb ? 'USB' : 'battery'} · {p.wake}{#if p.sleep_s}
                  {' · '}sleep {fmtSleep(p.sleep_s)}{/if}
              </span>
              {#if p.checksum_short}
                <code class="text-xs">{p.checksum_short}</code>
              {/if}
            </div>
            {#if p.has_image}
              <img
                src={p.image_url}
                alt="Frame served at {p.when}"
                width="400"
                class="w-full rounded-xl border border-ink/10 bg-white"
              />
            {:else}
              <p class="text-sm font-semibold text-muted">No stored image for this checksum.</p>
            {/if}
          </li>
        {/each}
      </ol>
      {#if page.page_count > 1}
        <div class="mt-3">
          {@render pager()}
        </div>
      {/if}
    </section>
  {:else}
    <section class="card p-5 sm:p-6">
      <p class="font-display text-xl text-ink">No Pico has checked in yet.</p>
      <p class="mt-2 text-sm font-semibold text-muted">
        The frame <code class="text-ink">POST</code>s <code class="text-ink">/api/frame.bin</code>
        with battery millivolts, percent, USB, and wake reason. Browser downloads of the packed
        file are not logged.
      </p>
      <p class="mt-2 text-sm font-semibold text-muted">
        Use <code class="text-ink">pico-sim</code> to generate sample polls without hardware.
      </p>
      {#if page}
        <dl class="mt-4 text-sm">
          <div>
            <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Storage</dt>
            <dd class="mt-0.5 font-semibold">{page.debug_dir_label}</dd>
          </div>
        </dl>
        {#if page.debug_dir_bytes > 0}
          <button
            type="button"
            class="btn btn-ghost mt-5 text-terracotta-dark"
            disabled={clearing}
            onclick={clearHistory}
          >
            {clearing ? 'Deleting…' : 'Delete history'}
          </button>
        {/if}
      {/if}
    </section>
  {/if}
</main>
