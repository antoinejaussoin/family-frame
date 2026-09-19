<script>
  import { link, route } from './route.svelte.js'

  let { class: className = '' } = $props()

  const items = [
    { href: '/', label: 'Family' },
    { href: '/preview', label: 'Layout' },
    { href: '/stats', label: 'Stats' },
    { href: '/config', label: 'Setup' },
  ]
</script>

<nav class="flex flex-wrap gap-x-4 gap-y-1 text-sm font-bold {className}">
  {#each items as item}
    <a
      href={item.href}
      use:link
      class="hover:text-ink {route.path === item.href ? 'text-ink' : 'text-muted'}"
      aria-current={route.path === item.href ? 'page' : undefined}
    >
      {item.label}
    </a>
  {/each}
</nav>
