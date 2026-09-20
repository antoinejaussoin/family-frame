<script>
  import { onMount } from 'svelte'
  import FrameMark from '../lib/FrameMark.svelte'
  import PageNav from '../lib/PageNav.svelte'
  import { getFrameJson, getSettings } from '../lib/api.js'

  let familyName = $state('Family')
  let dateLong = $state('')
  let sourceNote = $state('')
  let checksumHint = $state('Checksum loads after the first raster…')
  let stageEl = $state(null)
  let scale = $state(0.5)
  let rasterOpen = $state(false)
  let rasterLoading = $state(false)
  let rasterError = $state('')
  let rasterSrc = $state('')

  function formatDateLong(timeZone) {
    try {
      return new Intl.DateTimeFormat('en-GB', {
        timeZone: timeZone || undefined,
        day: 'numeric',
        month: 'long',
        year: 'numeric',
      }).format(new Date())
    } catch {
      return new Intl.DateTimeFormat('en-GB', {
        day: 'numeric',
        month: 'long',
        year: 'numeric',
      }).format(new Date())
    }
  }

  $effect(() => {
    const el = stageEl
    if (!el) return
    const apply = () => {
      scale = Math.min(1, Math.max(0.22, (el.clientWidth - 8) / 1648))
    }
    apply()
    const ro = new ResizeObserver(apply)
    ro.observe(el)
    return () => ro.disconnect()
  })

  $effect(() => {
    if (!rasterOpen) return
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    const onKey = (e) => {
      if (e.key === 'Escape') closeRaster()
    }
    window.addEventListener('keydown', onKey)
    return () => {
      document.body.style.overflow = prevOverflow
      window.removeEventListener('keydown', onKey)
    }
  })

  onMount(() => {
    loadMeta()
  })

  function openRaster() {
    rasterError = ''
    rasterLoading = true
    rasterSrc = `/api/frame-dither.png?fresh=1&v=${Date.now()}`
    rasterOpen = true
  }

  function closeRaster() {
    rasterOpen = false
    rasterSrc = ''
    rasterLoading = false
    rasterError = ''
  }

  async function loadMeta() {
    try {
      const settings = await getSettings()
      familyName = settings.family_name || familyName
      dateLong = formatDateLong(settings.timezone)
    } catch {
      dateLong = formatDateLong()
    }
    try {
      const data = await getFrameJson()
      sourceNote = data.source_note || ''
      checksumHint = `Checksum ${String(data.checksum).slice(0, 16)}… · ${data.bytes} bytes · Pico sends this back to skip a matching frame.`
    } catch {
      checksumHint =
        'Raster is not ready yet (Chrome needed for /api/frame.bin). The HTML panel is still the layout you edit.'
    }
  }
</script>

<main class="mx-auto max-w-[96rem] px-4 py-8 sm:px-6 lg:px-8 lg:py-10">
  <header class="mb-6 flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
    <div class="flex items-center gap-4">
      <FrameMark size={52} />
      <div>
        <p
          class="font-display text-base text-terracotta italic"
          style="font-variation-settings: 'opsz' 72"
        >
          13.3″ Inky simulator
        </p>
        <h1
          class="font-display text-3xl font-semibold tracking-tight text-ink sm:text-4xl"
          style="font-variation-settings: 'opsz' 96"
        >
          Layout
        </h1>
        <p class="mt-1 max-w-xl text-sm font-semibold text-muted">
          The framed panel is the same 1600×1200 page the server screenshots for
          the Pico. Edit <code class="text-ink">templates/dashboard.html</code>
          and <code class="text-ink">static/dashboard.css</code>, then refresh.
        </p>
      </div>
    </div>
    <PageNav />
  </header>

  <div class="grid gap-6 xl:grid-cols-[18rem_minmax(0,1fr)]">
    <aside class="card h-fit p-5 sm:p-6">
      <dl class="grid gap-3 text-sm">
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Family</dt>
          <dd class="mt-0.5 font-semibold text-ink">{familyName}</dd>
        </div>
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Date on panel</dt>
          <dd class="mt-0.5 font-semibold text-ink">{dateLong || '—'}</dd>
        </div>
        <div>
          <dt class="text-xs font-extrabold tracking-wide text-muted uppercase">Data</dt>
          <dd class="mt-0.5 font-semibold text-ink">{sourceNote || '—'}</dd>
        </div>
      </dl>

      <div class="mt-5 flex gap-1.5" aria-label="Spectra 6 palette">
        <span class="h-7 w-7 border border-ink bg-black"></span>
        <span class="h-7 w-7 border border-ink bg-white"></span>
        <span class="h-7 w-7 border border-ink bg-yellow-300"></span>
        <span class="h-7 w-7 border border-ink bg-red-600"></span>
        <span class="h-7 w-7 border border-ink bg-blue-600"></span>
        <span class="h-7 w-7 border border-ink bg-green-500"></span>
      </div>

      <div class="mt-5 flex flex-col gap-2 text-sm font-bold">
        <a class="text-terracotta hover:text-terracotta-dark" href="/dashboard" target="_blank"
          >Open dashboard only</a
        >
        <a class="text-terracotta hover:text-terracotta-dark" href="/api/frame.png">Screenshot PNG</a>
        <a class="text-terracotta hover:text-terracotta-dark" href="/api/frame-dither.png"
          >Dithered PNG</a
        >
        <a class="text-terracotta hover:text-terracotta-dark" href="/api/frame.bin">Packed .bin</a>
        <a class="text-terracotta hover:text-terracotta-dark" href="/api/frame.json">Checksum JSON</a>
      </div>
      <p class="mt-5 text-sm font-semibold text-muted">{checksumHint}</p>
    </aside>

    <section bind:this={stageEl} class="min-w-0 overflow-hidden">
      <div class="relative" style="height: {Math.round(1248 * scale)}px">
        <div
          class="absolute top-0 left-0 origin-top-left bg-[#1f1a14] p-6 shadow-2xl"
          style="width: 1648px; height: 1248px; transform: scale({scale})"
        >
          <iframe
            title="Family dashboard at panel resolution"
            src="/dashboard?v=crisp-type"
            width="1600"
            height="1200"
            class="pointer-events-none block border-0 bg-white"
          ></iframe>
          <button
            type="button"
            class="absolute inset-6 cursor-zoom-in"
            aria-label="Open 1600 by 1200 Spectra raster sent to the Pico"
            onclick={openRaster}
          ></button>
        </div>
      </div>
      <p class="mt-3 text-xs font-extrabold tracking-wide text-muted uppercase">
        Pimoroni Inky Impression 13.3″ · 1600×1200 · Spectra 6 · click for 1:1
        raster
      </p>
    </section>
  </div>
</main>

{#if rasterOpen}
  <div class="fixed inset-0 z-50 overflow-auto bg-ink">
    <button
      type="button"
      class="fixed inset-0 cursor-default"
      aria-label="Close e-ink raster"
      onclick={closeRaster}
    ></button>
    <div class="relative z-10 mx-auto w-[1600px] px-0 pb-8">
      <div
        class="sticky top-0 z-20 mb-0 flex items-center justify-between gap-4 bg-ink px-1 py-3"
      >
        <div>
          <p class="font-display text-lg font-semibold text-paper">E-ink raster</p>
          <p class="text-sm font-semibold text-plaster">
            1600×1200 · Spectra 6 · same pixels as /api/frame.bin
          </p>
        </div>
        <button type="button" class="btn btn-ghost bg-paper" onclick={closeRaster}>
          Close
        </button>
      </div>
      <div
        class="relative bg-white shadow-2xl"
        style="width: 1600px; height: 1200px"
        role="dialog"
        aria-modal="true"
        aria-label="1600 by 1200 Spectra 6 raster"
      >
        {#if rasterLoading}
          <p
            class="absolute inset-0 grid place-items-center text-sm font-semibold text-muted"
          >
            Rasterising with Chrome…
          </p>
        {/if}
        {#if rasterError}
          <p
            class="absolute inset-0 grid place-items-center p-8 text-center text-sm font-semibold text-terracotta"
          >
            {rasterError}
          </p>
        {/if}
        {#if rasterSrc}
          <img
            src={rasterSrc}
            alt="Spectra 6 frame packed for the Pico"
            width="1600"
            height="1200"
            draggable="false"
            class="block"
            style="width: 1600px; height: 1200px; image-rendering: pixelated"
            class:opacity-0={rasterLoading || rasterError}
            onload={() => {
              rasterLoading = false
              rasterError = ''
            }}
            onerror={() => {
              rasterLoading = false
              rasterError =
                'Could not rasterise. Chrome is required for /api/frame-dither.png.'
            }}
          />
        {/if}
      </div>
    </div>
  </div>
{/if}
