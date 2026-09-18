<script>
  import { onMount } from 'svelte'
  import FrameMark from '../lib/FrameMark.svelte'
  import PageNav from '../lib/PageNav.svelte'
  import { deleteDebug, getDebug } from '../lib/api.js'

  const builtVersion = import.meta.env.APP_VERSION
  let page = $state(null)
  let pollPage = $state(1)
  let error = $state('')
  let loading = $state(true)
  let clearing = $state(false)
  let fetchGen = 0

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
          Debug
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
    <section class="card mb-5 p-5 sm:p-6" aria-label="Current battery">
      <p class="font-display text-7xl leading-none tracking-tight text-ink">
        {page.last_pct}<span class="text-3xl text-muted">%</span>
      </p>
      <p class="mt-1 text-base font-semibold text-muted">{page.last_mv} mV</p>
      <dl class="mt-4 grid gap-3 text-sm">
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Power</dt>
          <dd class="mt-0.5 font-semibold">{page.power_label}</dd>
        </div>
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Last seen</dt>
          <dd class="mt-0.5 font-semibold">{page.last_seen} · {page.last_seen_rel}</dd>
        </div>
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Last poll</dt>
          <dd class="mt-0.5 font-semibold">
            {page.last_status_label} · wake {page.last_wake}{#if page.last_sleep_s}
              {' · '}sleep {page.last_sleep_s}s{/if}
          </dd>
        </div>
        {#if page.has_next_refresh}
          <div>
            <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Next refresh</dt>
            <dd class="mt-0.5 font-semibold">{page.next_refresh} · {page.next_refresh_rel}</dd>
          </div>
        {/if}
        {#if page.pico_drift_label}
          <div>
            <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Pico drift</dt>
            <dd class="mt-0.5 font-semibold">{page.pico_drift_label}</dd>
          </div>
        {/if}
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Debug storage</dt>
          <dd class="mt-0.5 font-semibold">{page.debug_dir_label}</dd>
        </div>
      </dl>
      <p class="mt-4 text-base font-semibold {etaClass[page.eta_kind] || 'text-muted'}">
        {page.eta_text}
      </p>
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

    <section id="pico-requests" class="card p-5 sm:p-6" aria-label="Pico requests">
      <h2 class="mb-1 text-xs font-extrabold tracking-wide text-muted uppercase">Pico requests</h2>
      <p class="mb-3 text-sm font-semibold text-muted">
        {page.poll_count} poll{page.poll_count === 1 ? '' : 's'}
      </p>
      {@render pager()}
      <ol class="m-0 list-none p-0 {page.page_count > 1 ? 'mt-3' : ''}">
        {#each page.polls as p}
          <li class="border-t border-ink/10 py-4 first:border-t-0 first:pt-0">
            <div class="mb-2.5 flex flex-wrap gap-x-3 gap-y-1 text-sm font-semibold text-muted">
              <time class="font-extrabold text-ink">{p.when}</time>
              <span
                class="tabular-nums {p.status === 200
                  ? 'text-sage'
                  : p.status === 204 || p.status === 304
                    ? 'text-[#b45309]'
                    : ''}"
              >
                {p.status_label}
              </span>
              <span>
                {p.pct}% · {p.mv} mV · {p.usb ? 'USB' : 'battery'} · {p.wake}{#if p.sleep_s}
                  {' · '}sleep {p.sleep_s}s{/if}
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
            <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Debug storage</dt>
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
