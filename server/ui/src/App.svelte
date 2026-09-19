<script>
  import { route } from './lib/route.svelte.js'
  import Home from './pages/Home.svelte'
  import Preview from './pages/Preview.svelte'
  import Stats from './pages/Stats.svelte'
  import Config from './pages/Config.svelte'

  $effect(() => {
    const titles = {
      '/preview': 'Layout simulator',
      '/stats': 'Frame stats',
      '/config': 'Board setup',
    }
    document.title = titles[route.path] || 'Family Frame'
  })
</script>

{#if route.path === '/preview'}
  <Preview />
{:else if route.path === '/stats'}
  <Stats />
{:else if route.path === '/config'}
  <Config />
{:else}
  <Home />
{/if}
