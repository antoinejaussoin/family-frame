<script>
  import { onMount } from 'svelte'
  import {
    deletePicture,
    formatSleep,
    getSettings,
    listPictures,
    normalizeUpload,
    patchSettings,
    putRotate,
    uploadPicture,
  } from './lib/api.js'

  let settings = $state(null)
  let pictures = $state([])
  let rotate = $state([])
  let loading = $state(true)
  let busy = $state(false)
  let error = $state('')
  let scheduleMode = $state('interval') // interval | wake
  let intervalMins = $state(60)
  let wakeTimes = $state([])
  let newWake = $state('07:00')
  let modal = $state(null) // picture item
  let modalTab = $state('dither')
  let uploading = $state(false)
  let dashPreviewKey = $state(0)
  let dashPreviewLoading = $state(false)
  let rotateSaving = $state(false)
  let pendingRotate = $state(null)
  // Editor panel. Independent of `settings.mode` so the album (and upload)
  // is reachable before anything is hung on the frame.
  let view = $state('dashboard')

  function scheduleKindOf(sch) {
    const k = sch?.schedule_kind
    if (k === 'times' || k === 'wake' || k === 'wake-up') return 'wake'
    if (k === 'interval') return 'interval'
    return (sch?.wake_up || []).length > 0 ? 'wake' : 'interval'
  }

  function scheduleOf(s, mode) {
    if (!s) {
      return { poll_interval_secs: 3600, wake_up: [], schedule_kind: 'interval' }
    }
    const sch = mode === 'picture' ? s.pictures_schedule : s.dashboard_schedule
    return {
      poll_interval_secs: sch?.poll_interval_secs ?? s.poll_interval_secs ?? 3600,
      wake_up: sch?.wake_up ?? s.wake_up ?? [],
      schedule_kind: sch?.schedule_kind ?? s.schedule_kind,
    }
  }

  function applyScheduleFrom(s, mode) {
    const sch = scheduleOf(s, mode)
    intervalMins = Math.max(1, Math.round((sch.poll_interval_secs || 3600) / 60))
    wakeTimes = [...(sch.wake_up || [])]
    scheduleMode = scheduleKindOf(sch)
  }

  function knownRotate(ids, pics = pictures) {
    const known = new Set((pics || []).map((p) => p.id))
    return (ids || []).filter((id) => known.has(id))
  }

  function applySettings(s) {
    settings = s
    applyScheduleFrom(s, view)
    if (Array.isArray(s.rotate)) rotate = knownRotate(s.rotate)
  }

  async function refresh() {
    error = ''
    const [s, p] = await Promise.all([getSettings(), listPictures()])
    pictures = p.pictures || []
    applySettings(s)
    rotate = knownRotate(p.rotate || s.rotate || [], pictures)
    if (s.mode === 'picture' && rotate.length === 0) {
      applySettings(await patchSettings({ mode: 'dashboard', rotate: [] }))
      rotate = []
    }
  }

  onMount(async () => {
    try {
      await refresh()
      view = settings?.mode === 'picture' ? 'picture' : 'dashboard'
      applyScheduleFrom(settings, view)
    } catch (e) {
      error = e.message || String(e)
    } finally {
      loading = false
      if (view === 'dashboard') dashPreviewLoading = true
    }
  })

  $effect(() => {
    if (!modal) return
    const onKey = (e) => {
      if (e.key === 'Escape') modal = null
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  async function setMode(mode) {
    if (busy || !settings) return
    if (view === mode && settings.mode === mode) return
    const viewChanged = view !== mode
    view = mode
    if (viewChanged) applyScheduleFrom(settings, mode)
    if (viewChanged && mode === 'dashboard') {
      dashPreviewKey += 1
      dashPreviewLoading = true
    }
    error = ''
    if (settings.mode === mode) return
    // Keep the album visible with no photos; the e-ink stays on the dashboard
    // until something is hung in the rotation.
    if (mode === 'picture' && knownRotate(rotate).length === 0) return
    busy = true
    try {
      applySettings(await patchSettings({ mode }))
    } catch (e) {
      error = e.message || String(e)
    } finally {
      busy = false
    }
  }

  async function putPicturesOnFrame() {
    if (!settings || settings.mode === 'picture' || knownRotate(rotate).length === 0) return
    applySettings(await patchSettings({ mode: 'picture' }))
  }

  async function saveSchedule() {
    if (busy || !settings) return
    busy = true
    error = ''
    try {
      applySettings(
        await patchSettings({
          wake_up: wakeTimes,
          poll_interval_secs: intervalMins * 60,
          schedule_kind: scheduleMode === 'wake' ? 'times' : 'interval',
          schedule_for: view,
        }),
      )
    } catch (e) {
      error = e.message || String(e)
    } finally {
      busy = false
    }
  }

  function addWake() {
    if (!/^\d{1,2}:\d{2}$/.test(newWake)) {
      error = 'Wake time must look like HH:MM'
      return
    }
    const [h, m] = newWake.split(':').map(Number)
    if (h > 23 || m > 59) {
      error = 'Invalid wake time'
      return
    }
    const norm = `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
    if (!wakeTimes.includes(norm)) {
      wakeTimes = [...wakeTimes, norm].sort()
    }
    scheduleMode = 'wake'
  }

  function removeWake(t) {
    wakeTimes = wakeTimes.filter((x) => x !== t)
  }

  function toggleRotate(id) {
    const next = rotate.includes(id)
      ? rotate.filter((x) => x !== id)
      : [...rotate, id]
    persistRotate(next)
  }

  function moveRotate(id, dir) {
    const i = rotate.indexOf(id)
    if (i < 0) return
    const j = i + dir
    if (j < 0 || j >= rotate.length) return
    const next = [...rotate]
    ;[next[i], next[j]] = [next[j], next[i]]
    persistRotate(next)
  }

  async function persistRotate(next) {
    if (settings?.mode === 'picture' && next.length === 0) {
      error = 'Keep at least one photo on the frame while Pictures mode is on.'
      return
    }
    rotate = next
    pendingRotate = next
    error = ''
    if (rotateSaving) return
    rotateSaving = true
    try {
      while (pendingRotate) {
        const payload = pendingRotate
        pendingRotate = null
        const s = await putRotate(payload)
        settings = s
        if (!pendingRotate) {
          rotate = knownRotate(s.rotate || payload)
        }
      }
      if (view === 'picture') await putPicturesOnFrame()
    } catch (e) {
      error = e.message || String(e)
      try {
        const p = await listPictures()
        pictures = p.pictures || pictures
        rotate = knownRotate(p.rotate || rotate, pictures)
      } catch {
        /* keep optimistic rotate */
      }
    } finally {
      rotateSaving = false
    }
  }

  async function onFiles(ev) {
    const files = [...(ev.target.files || [])]
    ev.target.value = ''
    if (!files.length) return
    uploading = true
    error = ''
    try {
      const added = []
      for (const raw of files) {
        const file = await normalizeUpload(raw)
        const meta = await uploadPicture(file)
        if (meta?.id) added.push(meta.id)
      }
      await refresh()
      if (rotate.length === 0 && added.length) {
        await persistRotate(added)
      }
    } catch (e) {
      error = e.message || String(e)
    } finally {
      uploading = false
    }
  }

  async function removePicture(id) {
    if (!confirm('Delete this photo from the family library?')) return
    busy = true
    error = ''
    try {
      await deletePicture(id)
      if (modal?.id === id) modal = null
      await refresh()
    } catch (e) {
      error = e.message || String(e)
    } finally {
      busy = false
    }
  }

  function openModal(pic) {
    modal = pic
    modalTab = 'dither'
  }
</script>

{#snippet frameMark(size = 56)}
  <svg
    width={size}
    height={size}
    viewBox="0 0 64 64"
    aria-hidden="true"
    class="shrink-0 drop-shadow-sm"
  >
    <rect x="4" y="6" width="56" height="52" rx="8" fill="#d4654a" />
    <rect x="12" y="14" width="40" height="32" fill="#fff6e4" />
    <circle cx="42" cy="24" r="5.5" fill="#efc15a" />
    <path d="M12 46V34.5l8.5-8.5 8 7 7.5-9.5L52 34v12z" fill="#4f8f68" />
  </svg>
{/snippet}

<main class="mx-auto max-w-7xl px-4 py-8 sm:px-6 lg:px-8 lg:py-12">
  <header
    class="mb-8 flex flex-col gap-5 lg:mb-10 lg:flex-row lg:items-end lg:justify-between"
  >
    <div class="flex items-center gap-4">
      {@render frameMark(58)}
      <div>
        <p
          class="font-display text-base text-terracotta italic"
          style="font-variation-settings: 'opsz' 72"
        >
          Family Frame
        </p>
        <h1
          class="font-display text-4xl font-semibold tracking-tight text-ink sm:text-5xl lg:text-[3.25rem]"
          style="font-variation-settings: 'opsz' 96"
        >
          {settings?.family_name || 'Home'}
        </h1>
        <p class="mt-1 max-w-md text-sm font-semibold text-muted">
          Photos, calendars, and a little bit of home.
        </p>
      </div>
    </div>
    {#if settings}
      <div class="status-pill text-sm font-semibold text-muted">
        <span
          class="inline-flex items-center gap-1.5 rounded-full bg-sun/70 px-2.5 py-0.5 font-extrabold text-ink capitalize"
        >
          {#if settings.mode === 'picture'}
            <svg class="h-3.5 w-3.5" viewBox="0 0 16 16" aria-hidden="true">
              <rect x="1" y="3" width="14" height="11" rx="1.5" fill="currentColor" />
              <rect x="3" y="5" width="10" height="7" fill="#fff6e4" />
            </svg>
          {:else}
            <svg class="h-3.5 w-3.5" viewBox="0 0 16 16" aria-hidden="true">
              <rect x="2" y="3" width="12" height="11" rx="1.5" fill="currentColor" />
              <rect x="2" y="3" width="12" height="3.5" fill="#3a2a24" />
            </svg>
          {/if}
          {settings.mode === 'picture' ? 'Pictures' : 'Dashboard'}
        </span>
        <span>wakes in {formatSleep(settings.next_sleep_secs)}</span>
      </div>
    {/if}
  </header>

  {#if error}
    <div
      class="mb-6 rounded-2xl border border-terracotta/30 bg-[#fff1ea] px-4 py-3 text-sm font-semibold text-terracotta-dark"
      role="alert"
    >
      {error}
    </div>
  {/if}

  {#if loading}
    <div class="flex flex-col items-center gap-3 py-20 text-muted">
      {@render frameMark(48)}
      <p class="font-display text-xl text-ink">Warming up the frame…</p>
    </div>
  {:else}
    <div
      class="grid min-w-0 items-start gap-6 lg:grid-cols-[minmax(17.5rem,22rem)_minmax(0,1fr)] lg:gap-8 xl:grid-cols-[24rem_minmax(0,1fr)]"
    >
      <aside class="flex flex-col gap-5 lg:sticky lg:top-6">
        <section class="card p-5 sm:p-6">
          <h2 class="font-display text-2xl font-semibold text-ink">
            What’s on the frame?
          </h2>
          <p class="mt-1 text-sm font-semibold text-muted">
            Switch between the house dashboard and a rotating family album.
          </p>
          <div class="mt-4 grid grid-cols-2 gap-2.5 lg:grid-cols-1">
            <button
              type="button"
              class="choice {view === 'dashboard' ? 'on' : ''}"
              disabled={busy}
              aria-pressed={view === 'dashboard'}
              onclick={() => setMode('dashboard')}
            >
              <span class="choice-icon text-terracotta">
                <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <rect x="3" y="4" width="18" height="17" rx="3" fill="currentColor" />
                  <rect x="3" y="4" width="18" height="5" fill="#3a2a24" />
                  <rect x="6" y="12" width="3" height="3" fill="#fff6e4" />
                  <rect x="10.5" y="12" width="3" height="3" fill="#efc15a" />
                  <rect x="15" y="12" width="3" height="3" fill="#fff6e4" />
                  <rect x="6" y="16.5" width="3" height="3" fill="#fff6e4" />
                  <rect x="10.5" y="16.5" width="3" height="3" fill="#fff6e4" />
                </svg>
              </span>
              <span class="text-sm font-extrabold">Dashboard</span>
              <span class="text-xs font-semibold text-muted">
                Calendars, weather, the house
              </span>
            </button>
            <button
              type="button"
              class="choice {view === 'picture' ? 'on' : ''}"
              disabled={busy}
              aria-pressed={view === 'picture'}
              onclick={() => setMode('picture')}
            >
              <span class="choice-icon text-sage">
                <svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <rect x="2" y="3" width="20" height="18" rx="3" fill="currentColor" />
                  <rect x="5" y="6" width="14" height="10" fill="#fff6e4" />
                  <circle cx="14.5" cy="9.5" r="1.6" fill="#efc15a" />
                  <path d="M5 16v-3l3.2-3.2 2.8 2.5 2.6-3.2L19 13.2V16z" fill="#4f8f68" />
                </svg>
              </span>
              <span class="text-sm font-extrabold">Pictures</span>
              <span class="text-xs font-semibold text-muted">
                A slideshow of family photos
              </span>
            </button>
          </div>
        </section>

        <section class="card p-5 sm:p-6">
          <h2 class="font-display text-2xl font-semibold text-ink">
            When to wake
          </h2>
          <p class="mt-1 text-sm font-semibold text-muted">
            For
            <span class="text-ink">
              {view === 'picture' ? 'Pictures' : 'Dashboard'}
            </span>
            {#if settings?.timezone}
              , in {settings.timezone}
            {/if}. Each mode keeps its own schedule.
          </p>

          <div class="seg mt-4">
            <button
              type="button"
              class={scheduleMode === 'interval' ? 'on' : ''}
              onclick={() => (scheduleMode = 'interval')}
            >
              Every so often
            </button>
            <button
              type="button"
              class={scheduleMode === 'wake' ? 'on' : ''}
              onclick={() => (scheduleMode = 'wake')}
            >
              At these times
            </button>
          </div>

          {#if scheduleMode === 'interval'}
            <label class="mt-4 flex items-center gap-3 text-sm font-semibold text-ink">
              <span class="w-20 shrink-0 text-muted">Every</span>
              <input
                type="number"
                min="1"
                class="field w-24"
                bind:value={intervalMins}
              />
              <span class="text-muted">minutes</span>
            </label>
          {:else}
            <div class="mt-4 flex flex-wrap gap-2">
              {#each wakeTimes as t}
                <button
                  type="button"
                  class="chip"
                  onclick={() => removeWake(t)}
                  title="Remove {t}"
                >
                  {t}
                  <span aria-hidden="true" class="text-base leading-none">×</span>
                </button>
              {:else}
                <p class="text-sm font-semibold text-muted">No wake times yet.</p>
              {/each}
            </div>
            <div class="mt-3 flex flex-wrap items-center gap-2">
              <input type="time" class="field" bind:value={newWake} />
              <button type="button" class="btn btn-ink" onclick={addWake}>
                Add time
              </button>
            </div>
          {/if}

          <button
            type="button"
            class="btn btn-primary mt-5"
            disabled={busy}
            onclick={saveSchedule}
          >
            Save schedule
          </button>
        </section>
      </aside>

      {#if view === 'picture'}
      <section class="board min-w-0 p-4 sm:p-6 lg:p-7">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 class="font-display text-2xl font-semibold text-ink sm:text-3xl">
              Family photos
            </h2>
            <p class="mt-1 max-w-xl text-sm font-semibold text-muted">
              Hang landscape photos on the board. Tap one to preview the e-ink
              dither. Hearts pick what rotates on the frame — saved as you go.
            </p>
          </div>
          <label class="btn btn-ink cursor-pointer">
            {uploading ? 'Adding…' : 'Add photos'}
            <input
              type="file"
              class="sr-only"
              accept="image/*,.heic,.heif"
              multiple
              disabled={uploading}
              onchange={onFiles}
            />
          </label>
        </div>

        {#if rotate.length}
          <div class="mt-5 rounded-2xl bg-paper/80 p-3 sm:p-4">
            <p class="mb-3 text-xs font-extrabold tracking-wide text-muted uppercase">
              On the frame · {rotate.length}
            </p>
            <ol class="film">
              {#each rotate as id, i}
                {@const pic = pictures.find((p) => p.id === id)}
                <li class="film-card">
                  <p class="mb-1 flex items-center justify-between gap-0.5 text-[10px] font-extrabold text-muted sm:mb-1.5 sm:text-xs">
                    <span>{i + 1}</span>
                    <span class="flex shrink-0">
                      <button
                        type="button"
                        class="rounded-md px-1 py-0.5 hover:bg-plaster disabled:opacity-30 sm:px-1.5"
                        onclick={() => moveRotate(id, -1)}
                        disabled={i === 0}
                        aria-label="Move earlier"
                      >
                        ←
                      </button>
                      <button
                        type="button"
                        class="rounded-md px-1 py-0.5 hover:bg-plaster disabled:opacity-30 sm:px-1.5"
                        onclick={() => moveRotate(id, 1)}
                        disabled={i === rotate.length - 1}
                        aria-label="Move later"
                      >
                        →
                      </button>
                    </span>
                  </p>
                  {#if pic}
                    <img
                      src={pic.thumb_url}
                      alt=""
                      class="aspect-[4/3] w-full rounded-md object-cover"
                    />
                  {/if}
                </li>
              {/each}
            </ol>
          </div>
        {/if}

        <div
          class="mt-5 grid grid-cols-3 gap-2 sm:gap-4 sm:grid-cols-3 md:grid-cols-4 xl:grid-cols-5"
        >
          {#each pictures as pic}
            <article class="polaroid {rotate.includes(pic.id) ? 'on' : ''}">
              {#if rotate.includes(pic.id)}
                <span class="pin" title="On the frame"></span>
              {/if}
              <button
                type="button"
                class="block w-full"
                aria-label="Open photo preview"
                onclick={() => openModal(pic)}
              >
                <img
                  src={pic.thumb_url}
                  alt=""
                  class="aspect-[4/3] w-full rounded-[2px] object-cover"
                  loading="lazy"
                />
              </button>
              <button
                type="button"
                class="heart {rotate.includes(pic.id) ? 'on' : ''}"
                aria-pressed={rotate.includes(pic.id)}
                aria-label={rotate.includes(pic.id)
                  ? 'Remove this photo from the frame'
                  : 'Hang this photo on the frame'}
                onclick={() => toggleRotate(pic.id)}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    fill="currentColor"
                    d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"
                  />
                </svg>
              </button>
              <button
                type="button"
                class="absolute bottom-1.5 right-1.5 z-[2] rounded-full bg-paper/95 px-1.5 py-0.5 text-[10px] font-extrabold text-terracotta shadow-sm hover:bg-[#fff1ea] sm:bottom-2 sm:right-2 sm:px-2 sm:py-1 sm:text-[11px]"
                onclick={() => removePicture(pic.id)}
              >
                Delete
              </button>
            </article>
          {:else}
            <label
              class="col-span-full flex cursor-pointer flex-col items-center justify-center rounded-2xl border-2 border-dashed border-ink/20 bg-paper/50 px-6 py-14 text-center"
            >
              {@render frameMark(44)}
              <p class="font-display mt-3 text-xl text-ink">The board is empty</p>
              <p class="mt-1 max-w-sm text-sm font-semibold text-muted">
                Add a landscape photo and we’ll hang it up for the family.
              </p>
              <input
                type="file"
                class="sr-only"
                accept="image/*,.heic,.heif"
                multiple
                disabled={uploading}
                onchange={onFiles}
              />
            </label>
          {/each}
        </div>

        {#if pictures.length && rotate.length === 0}
          <p class="mt-4 text-sm font-extrabold text-[#8a5a12]">
            Tap a heart to hang a photo on the frame.
          </p>
        {/if}
      </section>
      {:else}
      <section class="card overflow-hidden p-4 sm:p-6 lg:p-7">
        <div class="mb-4">
          <h2 class="font-display text-2xl font-semibold text-ink sm:text-3xl">
            Dashboard
          </h2>
          <p class="mt-1 max-w-xl text-sm font-semibold text-muted">
            Calendars, weather, and the house — as it looks on the e-ink.
          </p>
        </div>
        {#if dashPreviewLoading}
          <p class="mb-3 text-sm font-semibold text-muted">
            Getting the latest dashboard…
          </p>
        {/if}
        <img
          src="/api/frame-dither.png?v={dashPreviewKey}"
          alt="Dashboard currently on the frame"
          class="w-full rounded-2xl border border-ink/10 bg-plaster aspect-[4/3] object-contain"
          onload={() => (dashPreviewLoading = false)}
          onerror={() => (dashPreviewLoading = false)}
        />
      </section>
      {/if}
    </div>

    <footer class="mt-10 flex flex-wrap gap-x-6 gap-y-2 text-sm font-bold text-muted">
      <a class="hover:text-ink" href="/preview">Layout simulator</a>
      <a class="hover:text-ink" href="/debug">Debug</a>
    </footer>
  {/if}
</main>

{#if modal}
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
    <button
      type="button"
      class="absolute inset-0 bg-ink/50 backdrop-blur-sm"
      aria-label="Close preview"
      onclick={() => (modal = null)}
    ></button>
    <div
      class="relative z-10 max-h-[90dvh] w-full max-w-4xl overflow-auto rounded-[1.75rem] bg-paper p-4 shadow-2xl sm:p-6"
      role="dialog"
      aria-modal="true"
      aria-label="Photo preview"
    >
      <div class="mb-4 flex items-start justify-between gap-3">
        <div>
          <h3 class="font-display text-2xl font-semibold text-ink">Preview</h3>
          <p class="text-sm font-semibold text-muted">
            How it will look on the Spectra 6 panel
          </p>
        </div>
        <button type="button" class="btn btn-ghost" onclick={() => (modal = null)}>
          Close
        </button>
      </div>
      <div class="seg mb-3">
        <button
          type="button"
          class={modalTab === 'dither' ? 'on' : ''}
          onclick={() => (modalTab = 'dither')}
        >
          Dithered
        </button>
        <button
          type="button"
          class={modalTab === 'original' ? 'on' : ''}
          onclick={() => (modalTab = 'original')}
        >
          Original
        </button>
      </div>
      <img
        src={modalTab === 'dither' ? modal.dither_url : modal.original_url}
        alt=""
        class="w-full rounded-2xl border border-ink/10 bg-plaster"
      />
    </div>
  </div>
{/if}
