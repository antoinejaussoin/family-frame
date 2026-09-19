function normalize(path) {
  if (!path || path === '/') return '/'
  return path.replace(/\/+$/, '') || '/'
}

function canonicalize(path) {
  const n = normalize(path)
  if (n === '/debug') return '/stats'
  return n
}

function syncLocation(path) {
  if (typeof window === 'undefined') return path
  if (path !== window.location.pathname) {
    history.replaceState({}, '', path)
  }
  return path
}

export const route = $state({
  path:
    typeof window === 'undefined' ? '/' : syncLocation(canonicalize(window.location.pathname)),
})

if (typeof window !== 'undefined') {
  window.addEventListener('popstate', () => {
    route.path = syncLocation(canonicalize(window.location.pathname))
  })
}

export function navigate(href) {
  const url = href.startsWith('/') ? href : `/${href}`
  const dest = canonicalize(url)
  if (dest === route.path && dest === window.location.pathname) return
  history.pushState({}, '', dest)
  route.path = dest
}

/** Svelte action: in-app navigation for same-origin paths. */
export function link(node) {
  function onClick(e) {
    if (e.defaultPrevented || e.button !== 0) return
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
    const href = node.getAttribute('href')
    if (!href || !href.startsWith('/') || href.startsWith('//')) return
    e.preventDefault()
    navigate(href)
  }
  node.addEventListener('click', onClick)
  return {
    destroy() {
      node.removeEventListener('click', onClick)
    },
  }
}
