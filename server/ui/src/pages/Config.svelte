<script>
  import { onMount } from 'svelte'
  import FrameMark from '../lib/FrameMark.svelte'
  import PageNav from '../lib/PageNav.svelte'
  import { getSettings, patchSettings } from '../lib/api.js'

  const TIMEZONES = [
    'Europe/London',
    'Europe/Dublin',
    'Europe/Paris',
    'Europe/Berlin',
    'Europe/Amsterdam',
    'Europe/Brussels',
    'Europe/Madrid',
    'Europe/Rome',
    'Europe/Lisbon',
    'Europe/Zurich',
    'Europe/Stockholm',
    'Europe/Oslo',
    'Europe/Copenhagen',
    'Europe/Helsinki',
    'Europe/Warsaw',
    'Europe/Prague',
    'Europe/Vienna',
    'Europe/Athens',
    'Atlantic/Reykjavik',
    'America/New_York',
    'America/Chicago',
    'America/Denver',
    'America/Los_Angeles',
    'America/Toronto',
    'America/Vancouver',
    'America/Mexico_City',
    'America/Sao_Paulo',
    'America/Argentina/Buenos_Aires',
    'Africa/Johannesburg',
    'Africa/Cairo',
    'Asia/Dubai',
    'Asia/Kolkata',
    'Asia/Singapore',
    'Asia/Hong_Kong',
    'Asia/Shanghai',
    'Asia/Tokyo',
    'Asia/Seoul',
    'Australia/Perth',
    'Australia/Adelaide',
    'Australia/Sydney',
    'Australia/Melbourne',
    'Australia/Brisbane',
    'Pacific/Auckland',
    'UTC',
  ]

  let loading = $state(true)
  let busy = $state(false)
  let error = $state('')
  let saved = $state('')
  let showToken = $state(false)

  let familyName = $state('')
  let timezone = $state('Europe/London')
  let batteryMah = $state(10000)
  let calendars = $state([])
  let birthdays = $state([])
  let todoistToken = $state('')
  let todoistProject = $state('Family')
  let weatherId = $state('')

  let nextKey = 1
  function rowKey() {
    nextKey += 1
    return nextKey
  }

  const timezoneOptions = $derived(
    TIMEZONES.includes(timezone) ? TIMEZONES : [timezone, ...TIMEZONES],
  )

  function applySettings(s) {
    familyName = s.family_name || ''
    timezone = s.timezone || 'Europe/London'
    batteryMah = s.battery_mah || 10000
    const urls = s.calendar?.ics_urls || []
    calendars = urls.length
      ? urls.map((url) => ({ key: rowKey(), url }))
      : [{ key: rowKey(), url: '' }]
    birthdays = sortBirthdays(
      (s.birthdays || []).map((person) => ({
        key: rowKey(),
        name: person.name || '',
        dob: person.dob || '',
      })),
    )
    todoistToken = s.todoist?.token || ''
    todoistProject = s.todoist?.project || 'Family'
    weatherId = s.weather?.location_id || ''
  }

  function sortBirthdays(list) {
    return [...list].sort((a, b) => {
      const [, am, ad] = (a.dob || '').split('-').map(Number)
      const [, bm, bd] = (b.dob || '').split('-').map(Number)
      return (am || 13) - (bm || 13) || (ad || 32) - (bd || 32) || a.name.localeCompare(b.name)
    })
  }

  onMount(async () => {
    try {
      applySettings(await getSettings())
    } catch (e) {
      error = e.message || String(e)
    } finally {
      loading = false
    }
  })

  function addCalendar() {
    calendars = [...calendars, { key: rowKey(), url: '' }]
  }

  function removeCalendar(key) {
    const next = calendars.filter((row) => row.key !== key)
    calendars = next.length ? next : [{ key: rowKey(), url: '' }]
  }

  function addBirthday() {
    birthdays = [...birthdays, { key: rowKey(), name: '', dob: '' }]
  }

  function removeBirthday(key) {
    birthdays = birthdays.filter((row) => row.key !== key)
  }

  async function save() {
    if (busy) return
    busy = true
    error = ''
    saved = ''
    try {
      const people = birthdays
        .map((person) => ({
          name: person.name.trim(),
          dob: person.dob.trim(),
        }))
        .filter((person) => person.name || person.dob)
      const incomplete = people.find((person) => !person.name || !person.dob)
      if (incomplete) {
        throw new Error('Each birthday needs a name and a date.')
      }
      const next = await patchSettings({
        family_name: familyName.trim(),
        timezone: timezone.trim(),
        battery_mah: Math.max(1, Number(batteryMah) || 10000),
        calendar: {
          ics_urls: calendars.map((row) => row.url.trim()).filter(Boolean),
        },
        birthdays: people,
        todoist: {
          token: todoistToken.trim(),
          project: todoistProject.trim() || 'Family',
        },
        weather: { location_id: weatherId.trim() },
      })
      applySettings(next)
      saved = 'Saved. The next wake will paint the board with these settings.'
    } catch (e) {
      error = e.message || String(e)
    } finally {
      busy = false
    }
  }
</script>

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
          Setup
        </h1>
        <p class="mt-1 max-w-md text-sm font-semibold text-muted">
          Name the board, plug in calendars and to-dos, and tell it where you live.
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

  {#if saved}
    <div
      class="mb-6 rounded-2xl border border-sage/30 bg-[#e5f3ea] px-4 py-3 text-sm font-semibold text-[#2f6b46]"
      role="status"
    >
      {saved}
    </div>
  {/if}

  {#if loading}
    <div class="flex flex-col items-center gap-3 py-20 text-muted">
      <FrameMark size={48} />
      <p class="font-display text-xl text-ink">Opening the cupboard…</p>
    </div>
  {:else}
    <form
      class="flex flex-col gap-5"
      onsubmit={(e) => {
        e.preventDefault()
        save()
      }}
    >
      <section class="card p-5 sm:p-6">
        <h2 class="font-display text-2xl font-semibold text-ink">Basics</h2>
        <p class="mt-1 text-sm font-semibold text-muted">
          What the mast calls you, which clock it uses, and how big the pack is.
        </p>

        <div class="mt-5 grid gap-4">
          <div class="field-block">
            <label class="field-label" for="family_name">Name of the frame</label>
            <input
              id="family_name"
              class="field"
              name="family_name"
              autocomplete="organization"
              bind:value={familyName}
              placeholder="Family"
            />
            <p class="field-hint">Shown on the family UI and in the browser tab for the panel.</p>
          </div>

          <div class="field-block">
            <label class="field-label" for="timezone">Timezone</label>
            <input
              id="timezone"
              class="field"
              name="timezone"
              list="frame-timezones"
              autocomplete="off"
              spellcheck="false"
              bind:value={timezone}
              placeholder="Europe/London"
            />
            <datalist id="frame-timezones">
              {#each timezoneOptions as zone}
                <option value={zone}></option>
              {/each}
            </datalist>
            <p class="field-hint">
              IANA name such as <code class="text-ink">Europe/London</code>. Wake-up times on the
              Family page use this zone.
            </p>
          </div>

          <div class="field-block">
            <label class="field-label" for="battery_mah">Battery size</label>
            <span class="flex items-center gap-3">
              <input
                id="battery_mah"
                class="field w-32"
                name="battery_mah"
                type="number"
                min="1"
                step="100"
                bind:value={batteryMah}
              />
              <span class="text-sm font-semibold text-muted">mAh</span>
            </span>
            <p class="field-hint">
              Nameplate of the 1S LiPo. Defaults to 10000, which matches the usual pouch on the
              shopping list. Stats uses this for “months left”.
            </p>
          </div>
        </div>
      </section>

      <section class="card p-5 sm:p-6">
        <h2 class="font-display text-2xl font-semibold text-ink">Calendars</h2>
        <p class="mt-1 text-sm font-semibold text-muted">
          Public webcal or ICS links only. The frame cannot sign in to iCloud.
        </p>

        <details class="howto mt-4" open>
          <summary>How to get a calendar link</summary>
          <p>
            Sharing a calendar with family members by Apple ID is <strong>not</strong> enough.
            The server fetches a <strong>public, read-only</strong> feed. You have to publish the
            calendar, then paste the <code>webcal://</code> URL here.
          </p>

          <h3>iCloud Family calendar (what most households want)</h3>
          <p>
            Family Sharing already creates a <strong>Family</strong> calendar. Everyone in the
            group can add recitals and school plays. To put that same calendar on the frame,
            the calendar owner publishes it:
          </p>

          <h3>On a Mac (Calendar app)</h3>
          <ol>
            <li>Open <strong>Calendar</strong>. Show the sidebar if it is hidden (View → Show Calendar List).</li>
            <li>Find the calendar under <strong>Family</strong> or iCloud — not a local “On My Mac” calendar.</li>
            <li>
              Hover the name and click the <strong>Share Calendar</strong> button, or
              Control-click the name and choose <strong>Share Calendar…</strong>.
            </li>
            <li>
              Tick <strong>Public Calendar</strong>. Inviting people in “Share With” only helps
              Apple IDs; it does not give the frame a URL.
            </li>
            <li>
              Copy the link (Share → Copy Link, or the URL shown). It looks like
              <code>webcal://p12-caldav.icloud.com/published/2/…</code>.
            </li>
            <li>Paste it below. <code>webcal://</code> is fetched as <code>https://</code>.</li>
          </ol>

          <h3>On iCloud.com (any computer)</h3>
          <ol>
            <li>Go to <a href="https://www.icloud.com/calendar" target="_blank" rel="noreferrer">icloud.com/calendar</a> and sign in.</li>
            <li>Hover the Family calendar in the sidebar and click the share / person icon.</li>
            <li>Turn on <strong>Public Calendar</strong>.</li>
            <li>Choose <strong>Copy</strong> and paste the webcal link here.</li>
          </ol>

          <h3>On iPhone or iPad</h3>
          <ol>
            <li>Open <strong>Calendar</strong>, then tap <strong>Calendars</strong> at the bottom.</li>
            <li>Tap the info button (ⓘ) next to the iCloud or Family calendar.</li>
            <li>Turn on <strong>Public Calendar</strong>.</li>
            <li>Tap <strong>Share Link</strong> and copy the URL (Notes, then paste here from a computer if that is easier).</li>
          </ol>

          <p>
            Anyone with the link can read event titles, times, and notes. Treat it like a
            household secret. Turn Public Calendar off in the same place if you want a new
            link. You can add more than one calendar — school, sports, a second iCloud calendar.
          </p>

          <h3>Google Calendar</h3>
          <ol>
            <li>On the web, open the calendar’s settings (gear → Settings, then the calendar name).</li>
            <li>Scroll to <strong>Integrate calendar</strong>.</li>
            <li>Copy the <strong>Secret address in iCal format</strong> (not the public HTML address).</li>
          </ol>

          <h3>Outlook</h3>
          <p>
            Settings → Calendar → Shared calendars → Publish a calendar, then copy the ICS link.
          </p>
        </details>

        <div class="mt-4 grid gap-3">
          {#each calendars as row (row.key)}
            <div class="config-row">
              <label class="field-block min-w-0">
                <span class="sr-only">Calendar link</span>
                <textarea
                  class="field field-url"
                  rows="2"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder="webcal://p12-caldav.icloud.com/published/2/…"
                  bind:value={row.url}
                ></textarea>
              </label>
              <button
                type="button"
                class="btn btn-ghost shrink-0"
                onclick={() => removeCalendar(row.key)}
              >
                Remove
              </button>
            </div>
          {/each}
        </div>
        <button type="button" class="btn btn-ghost mt-3" onclick={addCalendar}>
          Add another calendar
        </button>
        <p class="field-hint mt-3">
          Leave the list empty to drop calendars. Birthdays and school hours can still fill
          Today; otherwise the board shows the demo week.
        </p>
      </section>

      <section class="card p-5 sm:p-6">
        <h2 class="font-display text-2xl font-semibold text-ink">Birthdays</h2>
        <p class="mt-1 text-sm font-semibold text-muted">
          Anyone whose next birthday is today or within two weeks appears as “Name turns N”,
          with a present icon. Leap-day birthdays show on 28 February in non-leap years.
        </p>

        <div class="mt-4 grid gap-3">
          {#each birthdays as person (person.key)}
            <div class="config-birthday">
              <label class="field-block min-w-0">
                <span class="sr-only">Name</span>
                <input
                  class="field"
                  autocomplete="off"
                  placeholder="Name"
                  bind:value={person.name}
                />
              </label>
              <label class="field-block">
                <span class="sr-only">Date of birth</span>
                <input class="field" type="date" bind:value={person.dob} />
              </label>
              <button
                type="button"
                class="btn btn-ghost"
                onclick={() => removeBirthday(person.key)}
              >
                Remove
              </button>
            </div>
          {:else}
            <p class="text-sm font-semibold text-muted">
              No birthdays yet — add the household so they show up on the board.
            </p>
          {/each}
        </div>
        <button type="button" class="btn btn-ghost mt-3" onclick={addBirthday}>
          Add a birthday
        </button>
      </section>

      <section class="card p-5 sm:p-6">
        <h2 class="font-display text-2xl font-semibold text-ink">Todoist</h2>
        <p class="mt-1 text-sm font-semibold text-muted">
          Open tasks from one shared project fill the sidebar. Leave the token blank to keep
          the demo list.
        </p>

        <details class="howto mt-4">
          <summary>How to get the API token and project</summary>
          <h3>1. Make a family project</h3>
          <ol>
            <li>Open <a href="https://todoist.com" target="_blank" rel="noreferrer">todoist.com</a> (the web app is easiest).</li>
            <li>Create a project — <strong>Family</strong> is the usual name — and invite the household.</li>
            <li>
              The name you type below must match exactly (capitalisation included), or you can
              paste the project id from the project URL
              (<code>https://app.todoist.com/app/project/…</code>).
            </li>
          </ol>

          <h3>2. Copy your personal API token</h3>
          <ol>
            <li>Stay in the web app and click your <strong>avatar</strong> at the top-left.</li>
            <li>Choose <strong>Settings</strong>.</li>
            <li>Open the <strong>Integrations</strong> tab.</li>
            <li>Open the <strong>Developer</strong> tab at the top of that pane.</li>
            <li>Click <strong>Copy API token</strong>.</li>
          </ol>
          <p>
            Direct link, once you are signed in:
            <a
              href="https://app.todoist.com/app/settings/integrations/developer"
              target="_blank"
              rel="noreferrer">app.todoist.com/app/settings/integrations/developer</a
            >.
          </p>
          <p>
            This is a <strong>personal</strong> token: it can read your whole Todoist account,
            not only the family project. The frame is trusted-LAN only and has no login, so
            treat the token like a password.
          </p>
          <p>
            If a token leaks, use <strong>Issue a new API token</strong> on that same Developer
            page. Anything still using the old token will stop working.
          </p>
        </details>

        <div class="mt-5 grid gap-4">
          <div class="field-block">
            <label class="field-label" for="todoist_token">API token</label>
            <span class="flex flex-wrap items-center gap-2">
              <input
                id="todoist_token"
                class="field min-w-0 flex-1"
                name="todoist_token"
                autocomplete="off"
                spellcheck="false"
                type={showToken ? 'text' : 'password'}
                bind:value={todoistToken}
                placeholder="Paste the token from Integrations → Developer"
              />
              <button
                type="button"
                class="btn btn-ghost"
                onclick={() => (showToken = !showToken)}
              >
                {showToken ? 'Hide' : 'Show'}
              </button>
            </span>
          </div>
          <div class="field-block">
            <label class="field-label" for="todoist_project">Project</label>
            <input
              id="todoist_project"
              class="field"
              name="todoist_project"
              autocomplete="off"
              bind:value={todoistProject}
              placeholder="Family"
            />
            <p class="field-hint">Project name as it appears in Todoist, or the project id.</p>
          </div>
        </div>
      </section>

      <section class="card p-5 sm:p-6">
        <h2 class="font-display text-2xl font-semibold text-ink">Weather</h2>
        <p class="mt-1 text-sm font-semibold text-muted">
          Morning, afternoon, and evening icons sit in the Today / Coming next headings, with
          sunrise, sunset, and pollen from BBC Weather.
        </p>

        <details class="howto mt-4">
          <summary>How to get the BBC location id</summary>
          <ol>
            <li>
              Open
              <a href="https://www.bbc.co.uk/weather" target="_blank" rel="noreferrer"
                >bbc.co.uk/weather</a
              >.
            </li>
            <li>Search for your town, village, or city and open that forecast.</li>
            <li>
              Look at the address bar. The number at the end is the location id:
              <code>https://www.bbc.co.uk/weather/2643743</code> is London,
              so the id is <code>2643743</code>.
            </li>
            <li>
              Paste either the number or the whole BBC URL. The server keeps the id only.
            </li>
          </ol>
          <p>
            Pick the closest named place, not a whole region, so the icons match the school
            run. Leave the field empty for the demo forecast.
          </p>
        </details>

        <div class="field-block mt-5">
          <label class="field-label" for="weather_location_id">Location id</label>
          <input
            id="weather_location_id"
            class="field"
            name="weather_location_id"
            inputmode="numeric"
            autocomplete="off"
            spellcheck="false"
            bind:value={weatherId}
            placeholder="2643743 or a bbc.co.uk/weather URL"
          />
        </div>
      </section>

      <div
        class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-ink/10 bg-paper px-4 py-3 shadow-card"
      >
        <p class="text-sm font-semibold text-muted">
          Photos and wake times stay on the Family page.
        </p>
        <button type="submit" class="btn btn-primary" disabled={busy}>
          {busy ? 'Saving…' : 'Save setup'}
        </button>
      </div>
    </form>
  {/if}
</main>
