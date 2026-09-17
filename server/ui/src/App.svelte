<script>
  import { route } from './lib/route.svelte.js'
  import Debug from './pages/Debug.svelte'
  import Home from './pages/Home.svelte'
  import Preview from './pages/Preview.svelte'

  $effect(() => {
    const titles = {
      '/preview': 'Layout simulator',
      '/debug': 'Frame debug',
    }
    document.title = titles[route.path] || 'Family Frame'
  })
</script>

{#if route.path === '/preview'}
  <Preview />
{:else if route.path === '/debug'}
  <Debug />
{:else}
  <Home />
{/if}
