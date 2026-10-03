<script>
  import { onMount } from 'svelte'
  import FrameMark from '../lib/FrameMark.svelte'
  import PageNav from '../lib/PageNav.svelte'
  import { adjustMinecraft, getMinecraft } from '../lib/api.js'

  const STEPS = [5, 10, 15, 20, 25, 30]
  const LOG_PREVIEW = 15

  let report = $state(null)
  let loading = $state(true)
  let busy = $state(false)
  let error = $state('')
  let flash = $state(null) // { text, tone }
  let selected = $state(null) // weekend date for the build-up chart
  let showAllLog = $state(false)
  let fetchGen = 0

  async function refresh() {
    const gen = ++fetchGen
    try {
      const next = await getMinecraft()
      if (gen !== fetchGen) return
      report = next
      error = ''
    } catch (e) {
      if (gen !== fetchGen) return
      error = e.message || String(e)
    } finally {
      if (gen === fetchGen) loading = false
    }
  }

  async function adjust(minutes) {
    if (busy) return
    busy = true
    error = ''
    try {
      fetchGen++
      const out = await adjustMinecraft(minutes)
      report = out.report
      flash = {
        text: out.summary,
        tone: out.applied > 0 ? 'add' : out.applied < 0 ? 'remove' : 'none',
      }
    } catch (e) {
      error = e.message || String(e)
    } finally {
      busy = false
    }
  }

  onMount(() => {
    refresh()
    const id = setInterval(refresh, 30_000)
    return () => clearInterval(id)
  })

  const isWeekend = $derived(report?.phase === 'weekend')
  const targetLabel = $derived(
    report?.weekends.find((w) => w.weekend === report.target)?.label || report?.target,
  )
  const chartWeekends = $derived(report ? [...report.weekends].reverse().slice(-12) : [])
  const buildWeekend = $derived(selected || report?.target || null)
  const log = $derived(
    report ? (showAllLog ? report.entries : report.entries.slice(0, LOG_PREVIEW)) : [],
  )

  function fmt(minutes) {
    const m = Math.max(0, minutes | 0)
    const h = Math.floor(m / 60)
    const mm = m % 60
    if (!h) return `${mm}m`
    if (!mm) return `${h}h`
    return `${h}h ${String(mm).padStart(2, '0')}`
  }

  function signed(minutes) {
    return minutes > 0 ? `+${minutes}` : `${minutes}`
  }

  const statusLabel = {
    past: 'Played',
    now: 'This weekend',
    coming: 'Coming',
    next: 'Next',
  }

  const statusClass = {
    past: 'debug-pill-batt',
    now: 'debug-pill-ok',
    coming: 'debug-pill-high',
    next: 'debug-pill-high',
  }

  // Weekend bars: earned above the axis, lost below, total on top.
  const BAR_W = 640
  const BAR_H = 220
  const BAR_PAD = { top: 26, bottom: 28, left: 8, right: 8 }
  const bars = $derived.by(() => {
    const list = chartWeekends
    if (!list.length) return null
    const maxA = Math.max(30, ...list.map((w) => w.added))
    const maxR = Math.max(0, ...list.map((w) => w.removed))
    const inner = BAR_H - BAR_PAD.top - BAR_PAD.bottom
    const scale = inner / (maxA + maxR || 1)
    const axis = BAR_PAD.top + maxA * scale
    const slot = (BAR_W - BAR_PAD.left - BAR_PAD.right) / list.length
    const w = Math.min(44, slot * 0.62)
    return {
      axis,
      items: list.map((wk, i) => {
        const cx = BAR_PAD.left + slot * i + slot / 2
        return {
          ...wk,
          x: cx - w / 2,
          cx,
          w,
          addTop: axis - wk.added * scale,
          addH: wk.added * scale,
          remH: wk.removed * scale,
          day: wk.label.replace(/^\w+ /, ''),
        }
      }),
    }
  })

  // Build-up: running total for one weekend, from the previous Friday noon
  // (when banking for it can start) to its own Friday noon.
  const LINE_W = 640
  const LINE_H = 200
  const LINE_PAD = { top: 16, bottom: 28, left: 40, right: 12 }
  const buildUp = $derived.by(() => {
    if (!report || !buildWeekend) return null
    const end = new Date(`${buildWeekend}T12:00`)
    const start = new Date(end.getTime() - 7 * 86_400_000)
    const entries = [...report.entries]
      .reverse()
      .filter((e) => e.weekend === buildWeekend)
      .map((e) => ({ t: new Date(e.local_iso), minutes: e.minutes }))
      .sort((a, b) => a.t - b.t)
    const now = Date.now()
    const stop = Math.min(end.getTime(), Math.max(now, start.getTime()))
    let run = 0
    const pts = [{ t: start.getTime(), v: 0 }]
    for (const e of entries) {
      const t = Math.max(start.getTime(), Math.min(e.t.getTime(), end.getTime()))
      pts.push({ t, v: run })
      run = Math.max(0, run + e.minutes)
      pts.push({ t, v: run })
    }
    pts.push({ t: Math.max(stop, pts[pts.length - 1].t), v: run })
    const maxV = Math.max(30, ...pts.map((p) => p.v))
    const yMax = Math.ceil(maxV / 30) * 30
    const sx = (t) =>
      LINE_PAD.left +
      ((t - start.getTime()) / (end.getTime() - start.getTime())) *
        (LINE_W - LINE_PAD.left - LINE_PAD.right)
    const sy = (v) => LINE_H - LINE_PAD.bottom - (v / yMax) * (LINE_H - LINE_PAD.top - LINE_PAD.bottom)
    const path = pts.map((p, i) => `${i ? 'L' : 'M'}${sx(p.t).toFixed(1)} ${sy(p.v).toFixed(1)}`).join(' ')
    const last = pts[pts.length - 1]
    const area = `${path} L${sx(last.t).toFixed(1)} ${sy(0)} L${sx(start.getTime()).toFixed(1)} ${sy(0)} Z`
    const days = []
    for (let i = 1; i <= 7; i++) {
      const d = new Date(start)
      d.setDate(d.getDate() + i)
      d.setHours(0, 0, 0, 0)
      if (d.getTime() >= end.getTime()) break
      days.push({ x: sx(d.getTime()), label: d.toLocaleDateString('en-GB', { weekday: 'short' }) })
    }
    const yTicks = []
    for (let v = 0; v <= yMax; v += yMax > 120 ? 60 : 30) yTicks.push({ y: sy(v), label: fmt(v) })
    const meta = report.weekends.find((w) => w.weekend === buildWeekend)
    return {
      path,
      area,
      days,
      yTicks,
      dots: entries.map((e) => ({
        x: sx(Math.max(start.getTime(), Math.min(e.t.getTime(), end.getTime()))),
        minutes: e.minutes,
      })),
      baseline: sy(0),
      total: run,
      label: meta?.label || buildWeekend,
      status: meta?.status,
      count: entries.length,
    }
  })

  const weekdayMax = $derived(
    report ? Math.max(1, ...report.by_weekday.map((d) => Math.max(d.added, d.removed))) : 1,
  )
</script>

{#snippet stepButtons(sign)}
  <div class="grid grid-cols-3 gap-2 sm:grid-cols-6">
    {#each STEPS as step}
      <button
        type="button"
        class="btn mc-step {sign > 0 ? 'mc-step-add' : 'mc-step-remove'}"
        disabled={busy}
        onclick={() => adjust(sign * step)}
        aria-label="{sign > 0 ? 'Add' : 'Remove'} {step} minutes"
      >
        {sign > 0 ? '+' : '−'}{step}
      </button>
    {/each}
  </div>
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
          Minecraft
        </h1>
        <p class="mt-1 max-w-md text-sm font-semibold text-muted">
          Minutes earned in the week, played at the weekend. The total locks on Friday at noon.
        </p>
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

  {#if loading && !report}
    <div class="flex flex-col items-center gap-3 py-20 text-muted">
      <FrameMark size={48} />
      <p class="font-display text-xl text-ink">Counting blocks…</p>
    </div>
  {:else if report}
    <section class="card mb-5 p-5 sm:p-6" aria-label="Allowance">
      <div class="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 class="text-xs font-extrabold tracking-wide text-muted uppercase">
            {isWeekend ? 'This weekend' : 'Coming weekend'} · {report.headline.label}
          </h2>
          <p class="mt-2 font-display text-7xl leading-none tracking-tight text-ink">
            {report.headline.time}
          </p>
          {#if isWeekend}
            <p class="mt-2 text-sm font-semibold text-muted">Locked since Friday noon.</p>
          {/if}
        </div>
        {#if isWeekend && report.next}
          <div class="debug-stat min-w-40">
            <p class="debug-stat-label">Next weekend · {report.next.label}</p>
            <p class="debug-stat-value">{report.next.time}</p>
            <p class="debug-stat-hint">Building up now</p>
          </div>
        {/if}
      </div>

      <div class="mt-6 flex flex-col gap-3">
        <p class="text-sm font-bold text-ink">
          Buttons change the
          <span class="text-terracotta-dark">{report.target_label.toLowerCase()}</span>
          ({targetLabel}).
        </p>
        {@render stepButtons(1)}
        {@render stepButtons(-1)}
      </div>

      {#if flash}
        <p
          class="mt-4 rounded-2xl px-4 py-3 text-sm font-semibold {flash.tone === 'add'
            ? 'border border-sage/30 bg-[#e5f3ea] text-[#2f6b46]'
            : flash.tone === 'remove'
              ? 'border border-terracotta/30 bg-[#fff1ea] text-terracotta-dark'
              : 'border border-ink/10 bg-plaster/60 text-ink'}"
          role="status"
        >
          {flash.text}
        </p>
      {/if}
    </section>

    <section class="card mb-5 p-5 sm:p-6" aria-label="Totals">
      <h2 class="mb-4 text-xs font-extrabold tracking-wide text-muted uppercase">Totals</h2>
      <div class="debug-stats debug-stats-3">
        <div class="debug-stat">
          <p class="debug-stat-label">Earned, all time</p>
          <p class="debug-stat-value text-sage">{fmt(report.totals.added)}</p>
          <p class="debug-stat-hint">{report.totals.changes} changes</p>
        </div>
        <div class="debug-stat">
          <p class="debug-stat-label">Lost, all time</p>
          <p class="debug-stat-value text-terracotta-dark">{fmt(report.totals.removed)}</p>
          <p class="debug-stat-hint">
            {report.totals.added
              ? Math.round((100 * report.totals.removed) / report.totals.added)
              : 0}% of what was earned
          </p>
        </div>
        <div class="debug-stat">
          <p class="debug-stat-label">Average weekend</p>
          <p class="debug-stat-value">{report.totals.average_time}</p>
          <p class="debug-stat-hint">
            {#if report.totals.best}
              Best {report.totals.best.time} ({report.totals.best.label})
            {:else}
              Over {report.totals.locked_weekends} weekends
            {/if}
          </p>
        </div>
      </div>
    </section>

    {#if bars}
      <section class="card mb-5 p-5 sm:p-6" aria-label="Weekends">
        <h2 class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">
          Weekend by weekend
        </h2>
        <svg
          class="block h-auto w-full"
          viewBox="0 0 {BAR_W} {BAR_H}"
          role="img"
          aria-label="Minutes earned and lost per weekend"
        >
          <line
            x1={BAR_PAD.left}
            x2={BAR_W - BAR_PAD.right}
            y1={bars.axis}
            y2={bars.axis}
            stroke="rgba(58,42,36,0.25)"
          />
          {#each bars.items as b (b.weekend)}
            <g
              class="cursor-pointer"
              role="button"
              tabindex="0"
              aria-label="{b.label}: {b.time}"
              onclick={() => (selected = b.weekend)}
              onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (selected = b.weekend)}
            >
              <rect
                x={b.x}
                y={b.addTop}
                width={b.w}
                height={Math.max(b.addH, 0.5)}
                rx="4"
                fill={b.status === 'past' ? '#4f8f68' : '#9ecfc9'}
                stroke={b.weekend === buildWeekend ? '#3a2a24' : 'none'}
                stroke-width="2"
                stroke-dasharray={b.status === 'next' || b.status === 'coming' ? '4 3' : 'none'}
              />
              {#if b.remH > 0}
                <rect x={b.x} y={bars.axis} width={b.w} height={b.remH} rx="4" fill="#d4654a" />
              {/if}
              <text
                x={b.cx}
                y={b.addTop - 7}
                text-anchor="middle"
                font-size="12"
                font-weight="800"
                fill="#3a2a24">{b.time}</text
              >
              <text
                x={b.cx}
                y={BAR_H - 8}
                text-anchor="middle"
                font-size="11"
                font-weight={b.status === 'now' || b.status === 'coming' ? 800 : 600}
                fill="#7a645c">{b.day}</text
              >
            </g>
          {/each}
        </svg>
        <p class="mt-3 text-sm font-semibold text-muted">
          Green is earned, red is lost, the label is what was left to play. Dashed is still
          building. Tap a weekend to see how it built up.
        </p>
      </section>
    {/if}

    {#if buildUp}
      <section class="card mb-5 p-5 sm:p-6" aria-label="Build-up">
        <div class="mb-3 flex items-start justify-between gap-3">
          <h2 class="text-xs font-extrabold tracking-wide text-muted uppercase">
            How {buildUp.label} built up
          </h2>
          {#if buildUp.status}
            <span class="debug-pill {statusClass[buildUp.status]}">
              {statusLabel[buildUp.status]}
            </span>
          {/if}
        </div>
        <svg
          class="block h-auto w-full"
          viewBox="0 0 {LINE_W} {LINE_H}"
          role="img"
          aria-label="Running total for {buildUp.label}"
        >
          {#each buildUp.yTicks as t}
            <line
              x1={LINE_PAD.left}
              x2={LINE_W - LINE_PAD.right}
              y1={t.y}
              y2={t.y}
              stroke="rgba(58,42,36,0.08)"
            />
            <text x={LINE_PAD.left - 6} y={t.y + 4} text-anchor="end" font-size="11" fill="#7a645c"
              >{t.label}</text
            >
          {/each}
          {#each buildUp.days as d}
            <line
              x1={d.x}
              x2={d.x}
              y1={LINE_PAD.top}
              y2={buildUp.baseline}
              stroke="rgba(58,42,36,0.08)"
            />
            <text x={d.x + 4} y={LINE_H - 8} font-size="11" font-weight="700" fill="#7a645c"
              >{d.label}</text
            >
          {/each}
          <path d={buildUp.area} fill="rgba(79,143,104,0.15)" />
          <path d={buildUp.path} fill="none" stroke="#3a2a24" stroke-width="2.5" />
          {#each buildUp.dots as dot}
            <circle
              cx={dot.x}
              cy={buildUp.baseline + 8}
              r="3.5"
              fill={dot.minutes > 0 ? '#4f8f68' : '#d4654a'}
            />
          {/each}
        </svg>
        <p class="mt-3 text-sm font-semibold text-muted">
          From the Friday noon before (when banking starts) to its own Friday noon. {buildUp.count}
          {buildUp.count === 1 ? 'change' : 'changes'}, {fmt(buildUp.total)} in total.
        </p>
      </section>
    {/if}

    <section class="card mb-5 p-5 sm:p-6" aria-label="By day of the week">
      <h2 class="mb-4 text-xs font-extrabold tracking-wide text-muted uppercase">
        By day of the week
      </h2>
      <div class="grid grid-cols-7 gap-2">
        {#each report.by_weekday as d}
          <div class="flex flex-col items-center gap-1">
            <div class="flex h-28 w-full items-end justify-center gap-1">
              <span
                class="w-3 rounded-t-md bg-sage"
                style="height: {(100 * d.added) / weekdayMax}%"
                title="{d.day}: earned {fmt(d.added)}"
              ></span>
              <span
                class="w-3 rounded-t-md bg-terracotta"
                style="height: {(100 * d.removed) / weekdayMax}%"
                title="{d.day}: lost {fmt(d.removed)}"
              ></span>
            </div>
            <p class="text-xs font-extrabold text-ink">{d.day}</p>
            <p class="text-[0.7rem] font-bold text-muted tabular-nums">+{d.added}</p>
          </div>
        {/each}
      </div>
      <p class="mt-3 text-sm font-semibold text-muted">
        Minutes earned (green) and lost (red) on each day, all time.
      </p>
    </section>

    <section class="card mb-5 p-5 sm:p-6" aria-label="Weekends table">
      <h2 class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">Weekends</h2>
      <div class="overflow-x-auto">
        <table class="w-full text-left text-sm">
          <thead>
            <tr class="text-xs font-extrabold tracking-wide text-muted uppercase">
              <th class="py-2 pr-3">Weekend</th>
              <th class="py-2 pr-3">Status</th>
              <th class="py-2 pr-3 text-right">Earned</th>
              <th class="py-2 pr-3 text-right">Lost</th>
              <th class="py-2 text-right">To play</th>
            </tr>
          </thead>
          <tbody>
            {#each report.weekends as w (w.weekend)}
              <tr
                class="cursor-pointer border-t border-ink/10 font-semibold hover:bg-plaster/50 {w.weekend ===
                buildWeekend
                  ? 'bg-plaster/60'
                  : ''}"
                onclick={() => (selected = w.weekend)}
              >
                <td class="py-2 pr-3 text-ink">{w.label}</td>
                <td class="py-2 pr-3">
                  <span class="debug-pill {statusClass[w.status]}">{statusLabel[w.status]}</span>
                </td>
                <td class="py-2 pr-3 text-right text-sage tabular-nums">+{w.added}</td>
                <td class="py-2 pr-3 text-right text-terracotta-dark tabular-nums">−{w.removed}</td>
                <td class="py-2 text-right font-extrabold text-ink tabular-nums">{w.time}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <section class="card p-5 sm:p-6" aria-label="Log">
      <h2 class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">Log</h2>
      {#if report.entries.length}
        <ul class="divide-y divide-ink/10">
          {#each log as e (e.at + e.minutes)}
            <li class="flex items-center justify-between gap-3 py-2 text-sm font-semibold">
              <span class="text-ink">{e.local}</span>
              <span class="text-muted">for {e.weekend_label}</span>
              <span
                class="w-14 text-right font-extrabold tabular-nums {e.minutes > 0
                  ? 'text-sage'
                  : 'text-terracotta-dark'}">{signed(e.minutes)}</span
              >
            </li>
          {/each}
        </ul>
        {#if report.entries.length > LOG_PREVIEW}
          <button type="button" class="btn btn-ghost mt-3" onclick={() => (showAllLog = !showAllLog)}>
            {showAllLog ? 'Show fewer' : `Show all ${report.entries.length}`}
          </button>
        {/if}
      {:else}
        <p class="text-sm font-semibold text-muted">No minutes yet. Tap a + button to start.</p>
      {/if}
    </section>
  {/if}
</main>
