function normalize(path) {
  if (!path || path === '/') return '/'
  return path.replace(/\/+$/, '') || '/'
}

export const route = $state({
  path: typeof window === 'undefined' ? '/' : normalize(window.location.pathname),
})

if (typeof window !== 'undefined') {
  window.addEventListener('popstate', () => {
    route.path = normalize(window.location.pathname)
  })
}

export function navigate(href) {
  const url = href.startsWith('/') ? href : `/${href}`
  if (normalize(url) === route.path && url === window.location.pathname) return
  history.pushState({}, '', url)
  route.path = normalize(window.location.pathname)
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
