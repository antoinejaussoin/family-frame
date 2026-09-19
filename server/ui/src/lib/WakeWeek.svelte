<script>
  const DAYS = [
    { id: 'mon', label: 'Monday', short: 'Mon', weekend: false },
    { id: 'tue', label: 'Tuesday', short: 'Tue', weekend: false },
    { id: 'wed', label: 'Wednesday', short: 'Wed', weekend: false },
    { id: 'thu', label: 'Thursday', short: 'Thu', weekend: false },
    { id: 'fri', label: 'Friday', short: 'Fri', weekend: false },
    { id: 'sat', label: 'Saturday', short: 'Sat', weekend: true },
    { id: 'sun', label: 'Sunday', short: 'Sun', weekend: true },
  ]

  let { week = $bindable(), timezone = 'Europe/London' } = $props()

  let newWake = $state('07:00')
  let selectedOverride = $state(null)
  let copyFrom = $state(null)
  let localError = $state('')
  const today = $derived(todayId(timezone))
  const selected = $derived(selectedOverride ?? today)

  const WEEKDAYS = ['mon', 'tue', 'wed', 'thu', 'fri']
  const WEEKEND = ['sat', 'sun']
  const ALL = DAYS.map((d) => d.id)

  $effect(() => {
    if (!copyFrom) return
    const onDoc = (e) => {
      if (!e.target.closest?.('[data-copy-menu]')) copyFrom = null
    }
    const onKey = (e) => {
      if (e.key === 'Escape') copyFrom = null
    }
    document.addEventListener('click', onDoc)
    window.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('click', onDoc)
      window.removeEventListener('keydown', onKey)
    }
  })

  function todayId(tz) {
    try {
      const short = new Intl.DateTimeFormat('en-GB', {
        weekday: 'short',
        timeZone: tz || 'Europe/London',
      }).format(new Date())
      const map = {
        Mon: 'mon',
        Tue: 'tue',
        Wed: 'wed',
        Thu: 'thu',
        Fri: 'fri',
        Sat: 'sat',
        Sun: 'sun',
      }
      return map[short] || 'mon'
    } catch {
      return ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'][new Date().getDay()]
    }
  }

  function dayMeta(id) {
    return DAYS.find((d) => d.id === id)
  }

  function normalizeTime(value) {
    if (!/^\d{1,2}:\d{2}$/.test(value)) return null
    const [h, m] = value.split(':').map(Number)
    if (h > 23 || m > 59) return null
    return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
  }

  function addToDays(ids) {
    localError = ''
    const norm = normalizeTime(newWake)
    if (!norm) {
      localError = 'Wake time must look like HH:MM'
      return
    }
    const next = { ...week }
    for (const id of ids) {
      if (!next[id].includes(norm)) {
        next[id] = [...next[id], norm].sort()
      }
    }
    week = next
    if (ids[0]) selectedOverride = ids[0]
  }

  function removeFrom(id, time) {
    week = { ...week, [id]: week[id].filter((x) => x !== time) }
  }

  function copyTo(fromId, toIds) {
    const times = [...(week[fromId] || [])]
    const next = { ...week }
    for (const id of toIds) next[id] = [...times]
    week = next
    copyFrom = null
  }

  function toggleCopy(id, ev) {
    ev.stopPropagation()
    copyFrom = copyFrom === id ? null : id
  }

  const selectedLabel = $derived(dayMeta(selected)?.label || 'this day')
</script>

<p class="mt-3 text-xs font-semibold text-muted">
  Set one day, then copy it to weekdays, the weekend, or every day.
</p>

<div class="wake-week mt-3" role="list">
  {#each DAYS as day}
    {@const times = week[day.id] || []}
    {@const isToday = day.id === today}
    {@const isSelected = day.id === selected}
    <div
      class="wake-day {day.weekend ? 'weekend' : ''} {isSelected ? 'on' : ''}"
      role="listitem"
      data-copy-menu={copyFrom === day.id ? '' : undefined}
    >
      <div class="wake-day-main">
        <button
          type="button"
          class="wake-day-name"
          aria-pressed={isSelected}
          onclick={() => {
            selectedOverride = day.id
            copyFrom = null
          }}
        >
          <span class="font-extrabold">{day.label}</span>
          {#if isToday}
            <span class="wake-today">today</span>
          {/if}
        </button>
        <div class="wake-day-times">
          {#each times as t}
            <span class="chip chip-sm">
              {t}
              <button
                type="button"
                class="chip-x"
                aria-label="Remove {t} from {day.label}"
                onclick={() => {
                  selectedOverride = day.id
                  removeFrom(day.id, t)
                }}
              >
                ×
              </button>
            </span>
          {:else}
            <button
              type="button"
              class="text-xs font-semibold text-muted"
              onclick={() => {
                selectedOverride = day.id
                copyFrom = null
              }}
            >
              Sleeps
            </button>
          {/each}
        </div>
      </div>
      <div class="wake-day-actions">
        <button
          type="button"
          class="wake-plus"
          aria-label="Add {newWake} to {day.label}"
          title="Add {newWake} to {day.label}"
          onclick={(e) => {
            e.stopPropagation()
            selectedOverride = day.id
            addToDays([day.id])
          }}
        >
          +
        </button>
        <button
          type="button"
          class="wake-copy {copyFrom === day.id ? 'on' : ''}"
          aria-expanded={copyFrom === day.id}
          aria-haspopup="true"
          onclick={(e) => toggleCopy(day.id, e)}
        >
          Copy
        </button>
      </div>
      {#if copyFrom === day.id}
        <div class="wake-copy-panel">
          <p class="mb-2 text-xs font-extrabold text-ink">
            Copy {day.label} to
          </p>
          <div class="flex flex-wrap gap-1.5">
            <button
              type="button"
              class="btn btn-ghost wake-copy-btn"
              onclick={() => copyTo(day.id, WEEKDAYS)}
            >
              Weekdays
            </button>
            <button
              type="button"
              class="btn btn-ghost wake-copy-btn"
              onclick={() => copyTo(day.id, WEEKEND)}
            >
              Weekend
            </button>
            <button
              type="button"
              class="btn btn-ghost wake-copy-btn"
              onclick={() => copyTo(day.id, ALL)}
            >
              Every day
            </button>
          </div>
          <div class="mt-2 flex flex-wrap gap-1">
            {#each DAYS.filter((d) => d.id !== day.id) as other}
              <button
                type="button"
                class="wake-copy-day"
                onclick={() => copyTo(day.id, [other.id])}
              >
                {other.short}
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/each}
</div>

{#if localError}
  <p class="mt-2 text-xs font-extrabold text-terracotta-dark" role="alert">
    {localError}
  </p>
{/if}

<div class="mt-3 flex flex-wrap items-center gap-2">
  <input type="time" class="field" bind:value={newWake} />
  <button type="button" class="btn btn-ink" onclick={() => addToDays([selected])}>
    Add to {selectedLabel}
  </button>
  <button type="button" class="btn btn-ghost" onclick={() => addToDays(ALL)}>
    Add to every day
  </button>
</div>
